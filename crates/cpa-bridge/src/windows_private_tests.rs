use super::*;
use crate::owner::{OwnerClaim, OwnerLease, OwnerRecord};
use hiroute_platform::file_security;
use hiroute_platform::windows::job::{Child, Launch, Stream};
use std::ffi::OsString;
use std::time::{Duration, Instant};

fn private_root() -> (tempfile::TempDir, PathBuf) {
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("private");
    directory(&root).unwrap();
    (outer, root)
}

#[test]
fn private_publication_and_owner_claim_use_actual_native_owner_and_acl() {
    let (_outer, root) = private_root();
    let path = root.join("private.json");
    write(&path, b"first").unwrap();
    write(&path, b"replacement").unwrap();
    assert_eq!(read(&path, 64).unwrap(), b"replacement");
    let lock_dir = root.join("instance.lock");
    let nonce = "a".repeat(43);
    let record = OwnerRecord::claim(
        std::process::id(),
        nonce.clone(),
        "8.0.4-hiroute.4".into(),
        "a".repeat(64),
    )
    .unwrap();
    let OwnerClaim::Acquired(lease) =
        OwnerLease::acquire(&lock_dir, &record, nonce.clone()).unwrap()
    else {
        panic!("fresh owner")
    };
    assert!(matches!(
        OwnerLease::acquire(&lock_dir, &record, nonce).unwrap(),
        OwnerClaim::Existing(_)
    ));
    for object in [
        path,
        root.join(".owner-reclaim.lock"),
        lock_dir.join("owner.json"),
    ] {
        security::verify_owner_only_handle(&file(&object).unwrap(), false).unwrap();
    }
    lease.release().unwrap();
    assert!(!lock_dir.exists());
}

#[test]
fn private_io_rejects_foreign_acl_hardlink_and_junction_without_adoption() {
    let (outer, root) = private_root();
    let path = root.join("foreign.json");
    write(&path, b"foreign-original").unwrap();
    hiroute_platform_test_support::grant_everyone_read(&path, false).unwrap();
    let before = file_security::capture(&security::open_identity(&path, false).unwrap()).unwrap();
    assert!(file(&path).is_err());
    assert!(write(&path, b"must-not-publish").is_err());
    assert!(remove_file(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"foreign-original");
    assert_eq!(
        file_security::capture(&security::open_identity(&path, false).unwrap()).unwrap(),
        before
    );
    let linked = root.join("linked.json");
    write(&linked, b"linked-original").unwrap();
    std::fs::hard_link(&linked, root.join("alias.json")).unwrap();
    assert!(file(&linked).is_err());
    assert!(write(&linked, b"must-not-publish").is_err());
    assert!(remove_file(&linked).is_err());
    let external = outer.path().join("external");
    std::fs::create_dir(&external).unwrap();
    std::fs::write(external.join("retained.json"), b"external-original").unwrap();
    let junction = root.join("junction");
    hiroute_platform_test_support::junction::create(&junction, &external).unwrap();
    assert!(directory(&junction).is_err());
    assert!(write(&junction.join("retained.json"), b"must-not-publish").is_err());
    assert_eq!(
        std::fs::read(external.join("retained.json")).unwrap(),
        b"external-original"
    );
    std::fs::remove_dir(&junction).unwrap();
}

#[test]
fn generated_auth_files_keep_user_owner_and_inherited_private_acl() {
    const CASE: &str =
        "windows_private::tests::generated_auth_files_keep_user_owner_and_inherited_private_acl";
    const ROOT: &str = "HIROUTE_TEST_GENERATED_AUTH_ROOT";
    if let Some(path) = std::env::var_os(ROOT) {
        let root = PathBuf::from(path);
        // This is a no-network storage fixture, not provider authorization.
        std::fs::write(root.join("one.json"), br#"{"type":"codex","access_token":"fixture-access","refresh_token":"fixture-refresh","account_id":"fixture-account"}"#).unwrap();
        return;
    }
    let (_outer, container) = private_root();
    let root = container.join("auth");
    new_directory(&root).unwrap();
    let binary = std::env::current_exe().unwrap();
    let args = ["--exact", CASE, "--nocapture"].map(OsString::from);
    let mut env = vec![(ROOT.into(), root.as_os_str().to_owned())];
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            env.push((key.into(), value));
        }
    }
    let child = Child::spawn(Launch {
        program: &binary,
        args: &args,
        env: &env,
        cwd: Some(&root),
        stdin: Stream::Null,
        stdout: Stream::Null,
        stderr: Stream::Null,
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.exit_code().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(child.exit_code().unwrap(), Some(0));
    let parent = existing_directory(&root).unwrap();
    let generated = security::open_inherited_private_file_at(&parent, "one.json".as_ref()).unwrap();
    assert!(
        security::verify_owner_only_handle(&generated, false).is_err(),
        "strict files still require protected DACL"
    );
    drop(generated);
    let source = crate::managed_oauth::ManagedOAuthCredentialSource {
        auth_dir: root.clone(),
        kind: crate::CpaAccountKind::Codex,
    };
    assert!(source.inspect().is_ok());
    hiroute_platform_test_support::grant_everyone_read(&root.join("one.json"), false).unwrap_err(); // fixture helper itself refuses inherited input
    std::fs::hard_link(root.join("one.json"), root.join("alias.json")).unwrap();
    assert!(source.inspect().is_err());
    std::fs::remove_file(root.join("alias.json")).unwrap();
    // The verification handle deliberately retains the directory object. A
    // successful Windows deletion remains pending until this final test-owned
    // handle closes; do not mistake that lifetime for failed session cleanup.
    drop(parent);
    remove_session_tree(&root, true).unwrap();
    assert!(!root.exists());
}

#[test]
fn damaged_private_record_is_preserved_and_never_rebuilt() {
    let (_outer, root) = private_root();
    let lock_dir = root.join("instance.lock");
    new_directory(&lock_dir).unwrap();
    write(&lock_dir.join("owner.json"), b"corrupt-record").unwrap();
    let nonce = "a".repeat(43);
    let record = OwnerRecord::claim(
        std::process::id(),
        nonce.clone(),
        "8.0.4-hiroute.4".into(),
        "a".repeat(64),
    )
    .unwrap();
    assert!(OwnerLease::acquire(&lock_dir, &record, nonce).is_err());
    assert_eq!(
        read(&lock_dir.join("owner.json"), 64).unwrap(),
        b"corrupt-record"
    );
}
