//! Actual native object regressions; controlled credential documents prove the
//! local admission boundary, never official account authorization or inference.

use super::*;
use crate::borrowed_claude::{BorrowedClaudeAuthSpec, ClaudeProfileReader};
use crate::private_test_support::tempdir;
use hiroute_integrations::ClaudeSubscriptionLocation;
use hiroute_platform::file_security;
use hiroute_platform::windows::security;
use hiroute_platform_test_support::{grant_everyone_read, junction};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn write(path: &Path, bytes: &[u8]) {
    let mut file = security::create_owner_only_file(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn codex_bytes() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "auth_mode":"chatgpt", "last_refresh":"controlled-read-admission",
        "tokens":{"access_token":"controlled-native-access", "account_id":"controlled-account",
                  "refresh_token":"REFRESH_MUST_REMAIN_NATIVE"}
    }))
    .unwrap()
}

fn claude_bytes() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"claudeAiOauth":{
        "accessToken":"controlled-claude-access", "expiresAt":u64::MAX,
        "scopes":["user:inference"], "refreshToken":"REFRESH_MUST_REMAIN_NATIVE"
    }}))
    .unwrap()
}

struct ControlledProfile {
    calls: Arc<AtomicUsize>,
    mutate: Option<PathBuf>,
}

impl ClaudeProfileReader for ControlledProfile {
    fn fetch(
        &self,
        _: &str,
        _: &crate::proxy_environment::ProxyEnvironment,
    ) -> Result<String, CpaLifecycleError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(path) = &self.mutate {
            grant_everyone_read(path, false).unwrap();
        }
        Ok("controlled-account".into())
    }
}

fn claude(path: &Path, mutate: bool) -> (BorrowedClaudeAuthSpec, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut spec = BorrowedClaudeAuthSpec::new(ClaudeSubscriptionLocation::File(path.to_owned()));
    spec.profile_reader = Arc::new(ControlledProfile {
        calls: calls.clone(),
        mutate: mutate.then(|| path.to_owned()),
    });
    (spec, calls)
}

#[test]
fn windows_borrowed_native_rejects_same_size_time_parent_replacement_before_read() {
    let root = tempdir().unwrap();
    let directory = root.path().join("native");
    drop(security::create_owner_only_directory(&directory).unwrap());
    let source = directory.join("auth.json");
    let bytes = codex_bytes();
    write(&source, &bytes);
    let original = security::open_identity(&source, false).unwrap();
    let original_id = security::file_identity(&original).unwrap();
    let original_time = original.metadata().unwrap().modified().unwrap();
    let original_permissions = file_security::capture(&original).unwrap();
    drop(original);
    let moved = root.path().join("moved-native");
    let result = native_windows::ReadGuard::open_after_parent(&source, || {
        // A real directory replacement in the parent→leaf window. No live leaf
        // is renamed and no POSIX sharing bypass or fake FileID is involved.
        fs::rename(&directory, &moved).unwrap();
        drop(security::create_owner_only_directory(&directory).unwrap());
        let mut replacement = security::create_owner_only_file(&source).unwrap();
        replacement.write_all(&bytes).unwrap();
        replacement.set_modified(original_time).unwrap();
        replacement.sync_all().unwrap();
        assert_eq!(replacement.metadata().unwrap().len(), bytes.len() as u64);
        assert_eq!(
            replacement.metadata().unwrap().modified().unwrap(),
            original_time
        );
        assert_ne!(security::file_identity(&replacement).unwrap(), original_id);
    });
    assert!(
        result.is_err(),
        "a replaced native parent cannot grant a read"
    );
    let old = security::open_identity(&moved.join("auth.json"), false).unwrap();
    assert_eq!(security::file_identity(&old).unwrap(), original_id);
    assert_eq!(file_security::capture(&old).unwrap(), original_permissions);
    assert_eq!(fs::read(moved.join("auth.json")).unwrap(), bytes);
    // Replacement by the native owner between separate admissions remains a
    // valid new read; a stale held admission must not retarget it.
    BorrowedCodexAuthSpec::new(&source).inspect().unwrap();
    let guard = native_windows::ReadGuard::open(&source).unwrap();
    assert!(fs::rename(&source, directory.join("renamed.json")).is_err());
    guard.verify().unwrap();
}

#[test]
fn windows_borrowed_native_rejects_ancestor_junction_and_hardlinks() {
    let root = tempdir().unwrap();
    let native = root.path().join("native");
    drop(security::create_owner_only_directory(&native).unwrap());
    let codex_source = native.join("codex.json");
    let claude_source = native.join("claude.json");
    write(&codex_source, &codex_bytes());
    write(&claude_source, &claude_bytes());
    let alias = root.path().join("junction");
    junction::create(&alias, &native).unwrap();
    assert!(
        BorrowedCodexAuthSpec::new(alias.join("codex.json"))
            .inspect()
            .is_err()
    );
    let (spec, calls) = claude(&alias.join("claude.json"), false);
    assert!(spec.inspect().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    fs::remove_dir(&alias).unwrap();
    for source in [&codex_source, &claude_source] {
        let link = root.path().join(source.file_name().unwrap());
        fs::hard_link(source, &link).unwrap();
        assert_eq!(
            security::file_link_count(&security::open_identity(source, false).unwrap()).unwrap(),
            2
        );
        if source == &codex_source {
            assert!(matches!(
                BorrowedCodexAuthSpec::new(source).inspect(),
                Err(CpaLifecycleError::InvalidBorrowedCodexAuth)
            ));
            assert!(BorrowedCodexAuthSpec::new(&link).inspect().is_err());
        } else {
            let (spec, calls) = claude(source, false);
            assert!(spec.inspect().is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let (spec, calls) = claude(&link, false);
            assert!(spec.inspect().is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
        fs::remove_file(link).unwrap();
    }
    assert_eq!(fs::read(&codex_source).unwrap(), codex_bytes());
    assert_eq!(fs::read(&claude_source).unwrap(), claude_bytes());
}

#[test]
fn windows_borrowed_native_preserves_readable_acl_readonly_and_access_only_lease() {
    let root = tempdir().unwrap();
    let codex_source = root.path().join("codex.json");
    let claude_source = root.path().join("claude.json");
    write(&codex_source, &codex_bytes());
    write(&claude_source, &claude_bytes());
    let mut originals = Vec::new();
    for source in [&codex_source, &claude_source] {
        grant_everyone_read(source, false).unwrap();
        let mut attributes = fs::metadata(source).unwrap().permissions();
        attributes.set_readonly(true);
        fs::set_permissions(source, attributes).unwrap();
        assert!(security::open_owner_only(source, false).is_err());
        originals.push(
            file_security::capture(&security::open_identity(source, false).unwrap()).unwrap(),
        );
    }
    let codex_spec = BorrowedCodexAuthSpec::new(&codex_source)
        .with_executable(root.path().join("unselected-engine.exe"));
    let expected = codex_spec.inspect().unwrap();
    let codex_auth = root.path().join("codex-managed");
    drop(security::create_owner_only_directory(&codex_auth).unwrap());
    let lease = ManagedAuthLease::acquire_expected(&codex_auth, Some(&codex_spec), Some(&expected))
        .unwrap();
    assert_eq!(lease.codex_generation(), Some(1));
    let (claude_spec, calls) = claude(&claude_source, false);
    let expected = claude_spec.inspect().unwrap();
    let claude_auth = root.path().join("claude-managed");
    drop(security::create_owner_only_directory(&claude_auth).unwrap());
    let mut claude_lease = crate::borrowed_claude::BorrowedClaudeLease::acquire(
        &claude_auth,
        &claude_spec,
        Some(&expected),
    )
    .unwrap();
    assert_eq!(claude_lease.generation(), Some(1));
    claude_lease.refresh(Some(&expected), None).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    for (directory, file) in [
        (&codex_auth, "hiroute-managed-codex.json"),
        (&claude_auth, "hiroute-managed-claude.json"),
    ] {
        let path = directory.join(file);
        security::verify_owner_only(&path, false).unwrap();
        assert!(
            !String::from_utf8(fs::read(path).unwrap())
                .unwrap()
                .contains("REFRESH_MUST_REMAIN_NATIVE")
        );
    }
    for (source, original) in [&codex_source, &claude_source].into_iter().zip(originals) {
        assert_eq!(
            file_security::capture(&security::open_identity(source, false).unwrap()).unwrap(),
            original
        );
    }
    assert_eq!(fs::read(&codex_source).unwrap(), codex_bytes());
    assert_eq!(fs::read(&claude_source).unwrap(), claude_bytes());
    drop(claude_lease);
    drop(lease);
    // Remove only this fixture's read-only bit for the outer TempDir cleanup.
    for source in [&codex_source, &claude_source] {
        let mut attributes = fs::metadata(source).unwrap().permissions();
        attributes.set_readonly(false);
        fs::set_permissions(source, attributes).unwrap();
    }
}

#[test]
fn windows_borrowed_native_permission_change_rejects_before_lease_publication() {
    let root = tempdir().unwrap();
    let codex_source = root.path().join("codex.json");
    let claude_source = root.path().join("claude.json");
    write(&codex_source, &codex_bytes());
    write(&claude_source, &claude_bytes());
    let before = fs::metadata(&codex_source).unwrap();
    let result = read_nested_source_after_read(&codex_source, || {
        grant_everyone_read(&codex_source, false).unwrap();
    });
    assert!(matches!(
        result,
        Err(CpaLifecycleError::BorrowedCodexAuthSourceChanged)
    ));
    let after = fs::metadata(&codex_source).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    let (spec, calls) = claude(&claude_source, true);
    let managed = root.path().join("managed");
    drop(security::create_owner_only_directory(&managed).unwrap());
    assert!(matches!(
        crate::borrowed_claude::BorrowedClaudeLease::acquire(&managed, &spec, None),
        Err(CpaLifecycleError::BorrowedClaudeAuthSourceChanged)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!managed.join("hiroute-managed-claude.json").exists());
    assert_eq!(fs::read(&codex_source).unwrap(), codex_bytes());
    assert_eq!(fs::read(&claude_source).unwrap(), claude_bytes());
}

#[test]
fn windows_borrowed_native_rejects_post_write_change_before_identity_admission() {
    let root = tempdir().unwrap();
    let codex_source = root.path().join("codex.json");
    let claude_source = root.path().join("claude.json");
    write(&codex_source, &codex_bytes());
    write(&claude_source, &claude_bytes());

    let codex_auth = root.path().join("codex-managed");
    drop(security::create_owner_only_directory(&codex_auth).unwrap());
    let spec = BorrowedCodexAuthSpec::new(&codex_source)
        .with_executable(root.path().join("unselected-engine.exe"));
    let expected = spec.inspect().unwrap();
    let mut codex =
        ManagedAuthLease::acquire_expected(&codex_auth, Some(&spec), Some(&expected)).unwrap();
    let mut rotated: serde_json::Value = serde_json::from_slice(&codex_bytes()).unwrap();
    rotated["tokens"]["access_token"] = "controlled-rotated-access".into();
    private_atomic_write(&codex_source, &serde_json::to_vec(&rotated).unwrap()).unwrap();
    let mut written = false;
    let result = codex
        .codex
        .as_mut()
        .unwrap()
        .refresh_after_writes(None, None, || {
            // This hook is after actual flat and state publication. The test cannot
            // pass by bypassing writes or rejecting the source before publication.
            let state = load_state(&codex_auth.join(STATE_FILE_NAME))
                .unwrap()
                .unwrap();
            assert_eq!(state.generation, 2);
            let flat: serde_json::Value =
                serde_json::from_slice(&fs::read(codex_auth.join(MANAGED_FILE_NAME)).unwrap())
                    .unwrap();
            assert!(flat["access_token"].as_str() == Some("controlled-rotated-access"));
            grant_everyone_read(&codex_source, false).unwrap();
            written = true;
        });
    assert!(written);
    assert!(matches!(
        result,
        Err(CpaLifecycleError::BorrowedCodexAuthSourceChanged)
    ));
    assert_eq!(codex.codex_generation(), Some(1));
    // No destructive rollback: a fresh, exact admission can recover the cache.
    let recovered = codex.refresh().unwrap();
    assert_eq!(recovered[0].generation, 2);
    assert_eq!(codex.codex_generation(), Some(2));

    let claude_auth = root.path().join("claude-managed");
    drop(security::create_owner_only_directory(&claude_auth).unwrap());
    let (spec, _) = claude(&claude_source, false);
    let mut lease =
        crate::borrowed_claude::BorrowedClaudeLease::acquire(&claude_auth, &spec, None).unwrap();
    let mut rotated: serde_json::Value = serde_json::from_slice(&claude_bytes()).unwrap();
    rotated["claudeAiOauth"]["accessToken"] = "controlled-claude-access-2".into();
    let rotated = serde_json::to_vec(&rotated).unwrap();
    private_atomic_write(&claude_source, &rotated).unwrap();
    let mut written = false;
    let result = lease.refresh_after_writes(None, None, || {
        let flat: serde_json::Value = serde_json::from_slice(
            &fs::read(claude_auth.join("hiroute-managed-claude.json")).unwrap(),
        )
        .unwrap();
        assert!(flat["access_token"].as_str() == Some("controlled-claude-access-2"));
        let mut replacement: serde_json::Value = serde_json::from_slice(&rotated).unwrap();
        replacement["claudeAiOauth"]["accessToken"] = "controlled-claude-access-3".into();
        let replacement = serde_json::to_vec(&replacement).unwrap();
        assert_eq!(replacement.len(), rotated.len());
        let mut native = security::open_read_write(&claude_source).unwrap();
        let before = native.metadata().unwrap();
        let identity = security::file_identity(&native).unwrap();
        native.write_all(&replacement).unwrap();
        native.set_modified(before.modified().unwrap()).unwrap();
        native.sync_all().unwrap();
        let after = native.metadata().unwrap();
        assert_eq!(security::file_identity(&native).unwrap(), identity);
        assert_eq!(after.len(), before.len());
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        written = true;
    });
    assert!(written);
    assert!(matches!(
        result,
        Err(CpaLifecycleError::BorrowedClaudeAuthSourceChanged)
    ));
    assert_eq!(lease.generation(), Some(1));
    let recovered = lease.refresh(None, None).unwrap();
    assert_eq!(recovered.generation, 2);
    assert_eq!(lease.generation(), Some(2));
}
