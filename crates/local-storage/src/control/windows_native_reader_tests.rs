//! Actual Windows snapshot admission; no native account or provider is involved.
use super::*;
use hiroute_domain::NativeAgentArtifactPort;
use hiroute_platform::{durable, file_security, windows::security};
use std::cell::Cell;
use std::io::Write;

const TARGET: &str = "agents/native";

fn artifact_store(root: &Path, target: &Path) -> ManagedArtifactStore {
    ManagedArtifactStore::open_with_external_target(
        &crate::test_storage_authority(),
        root.join("artifacts"),
        root.join("restore"),
        TARGET,
        target,
    )
    .unwrap()
}

fn native_permissions(path: &Path) -> hiroute_platform::file_security::WindowsPermissions {
    file_security::capture(&security::open_identity(path, false).unwrap()).unwrap()
}

fn borrowed(path: &Path, bytes: &[u8]) {
    let mut permissions = file_security::private_defaults(false).unwrap();
    // This test owns the new object. An additional read ACE makes its DACL
    // observably different from a managed private descriptor, without changing
    // any existing object or claiming permission to read another user's file.
    permissions.descriptor.push_str("(A;;FR;;;BU)");
    durable::write_windows_atomic(path, bytes, &permissions).unwrap();
    assert!(security::verify_owner_only(path, false).is_err());
}

#[test]
fn windows_native_reader_ports_preserve_missing_and_borrowed_content_permissions() {
    let root = crate::test_tempdir().unwrap();
    let path = root.path().join("native.json");
    let store = artifact_store(root.path(), &path);
    assert!(store.read_native_target(TARGET).unwrap().is_none());
    assert!(store.read_private_native_target(TARGET).unwrap().is_none());
    borrowed(&path, b"borrowed native configuration");
    let permissions = native_permissions(&path);
    let identity =
        security::file_identity(&security::open_identity(&path, false).unwrap()).unwrap();
    for bytes in [
        store.read_native_target(TARGET).unwrap().unwrap(),
        store.read_private_native_target(TARGET).unwrap().unwrap(),
    ] {
        assert_eq!(bytes.as_slice(), b"borrowed native configuration");
    }
    assert_eq!(native_permissions(&path), permissions);
    assert_eq!(
        security::file_identity(&security::open_identity(&path, false).unwrap()).unwrap(),
        identity
    );
    let mut attributes = fs::metadata(&path).unwrap().permissions();
    attributes.set_readonly(true);
    fs::set_permissions(&path, attributes.clone()).unwrap();
    let read_only = native_permissions(&path);
    assert!(read_only.read_only);
    assert_eq!(
        store
            .read_private_native_target(TARGET)
            .unwrap()
            .unwrap()
            .as_slice(),
        b"borrowed native configuration"
    );
    assert_eq!(native_permissions(&path), read_only);
    attributes.set_readonly(false);
    fs::set_permissions(&path, attributes).unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(&path, b"ordinary inherited configuration").unwrap();
    let ordinary = native_permissions(&path);
    assert_eq!(
        store
            .read_native_target(TARGET)
            .unwrap()
            .unwrap()
            .as_slice(),
        b"ordinary inherited configuration"
    );
    assert_eq!(native_permissions(&path), ordinary);
}

#[test]
fn windows_native_reader_enforces_cap_before_read_and_accepts_exact_bound() {
    let root = crate::test_tempdir().unwrap();
    let path = root.path().join("bounded");
    let file = fs::File::create(&path).unwrap();
    file.set_len(1024 * 1024 + 1).unwrap();
    drop(file);
    let reached_read = Cell::new(false);
    assert!(matches!(
        native_permissions::read_windows_artifact_after_open(&path, 1024 * 1024, || {
            reached_read.set(true);
        }),
        Err(LocalStorageError::Permission)
    ));
    assert!(
        !reached_read.get(),
        "oversized admission must precede reading"
    );
    assert!(
        artifact_store(root.path(), &path)
            .read_native_target(TARGET)
            .is_err()
    );
    fs::write(&path, vec![b'x'; 4096]).unwrap();
    let snapshot = native_permissions::read_windows_artifact_bounded(&path, 4096)
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.bytes.len(), 4096);
    assert!(snapshot.bytes.iter().all(|byte| *byte == b'x'));
    assert!(native_permissions::read_windows_artifact_bounded(&path, 4095).is_err());
    fs::write(&path, []).unwrap();
    assert!(
        native_permissions::read_windows_artifact_bounded(&path, 0)
            .unwrap()
            .unwrap()
            .bytes
            .is_empty()
    );
}

#[test]
fn windows_native_reader_rejects_directory_hardlink_and_junction_aliases() {
    let root = crate::test_tempdir().unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let path = outside.join("settings.json");
    borrowed(&path, b"preserved original");
    let permissions = native_permissions(&path);
    let alias = root.path().join("hardlink");
    fs::hard_link(&path, &alias).unwrap();
    assert!(matches!(
        native_permissions::read_windows_artifact_bounded(&path, 1024),
        Err(LocalStorageError::Permission)
    ));
    fs::remove_file(&alias).unwrap();
    assert!(native_permissions::read_windows_artifact_bounded(&outside, 1024).is_err());
    let junction = root.path().join("junction");
    hiroute_platform_test_support::junction::create(&junction, &outside).unwrap();
    assert!(
        native_permissions::read_windows_artifact_bounded(&junction.join("settings.json"), 1024)
            .is_err()
    );
    assert!(native_permissions::read_windows_artifact_bounded(&junction, 1024).is_err());
    assert!(
        native_permissions::read_windows_artifact_bounded(&junction.join("missing"), 1024).is_err(),
        "a missing leaf cannot hide an ancestor reparse"
    );
    assert_eq!(fs::read(&path).unwrap(), b"preserved original");
    assert_eq!(native_permissions(&path), permissions);
    fs::remove_dir(&junction).unwrap();
}

#[test]
fn windows_native_reader_lease_denies_data_write_and_file_or_ancestor_replacement() {
    let root = crate::test_tempdir().unwrap();
    let parent = root.path().join("native");
    fs::create_dir(&parent).unwrap();
    let path = parent.join("config.json");
    borrowed(&path, b"leased configuration");
    let snapshot = native_permissions::read_windows_artifact_after_open(&path, 1024, || {
        for error in [
            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .err()
                .unwrap(),
            fs::rename(&path, parent.join("moved.json")).err().unwrap(),
            fs::rename(&parent, root.path().join("moved-parent"))
                .err()
                .unwrap(),
        ] {
            assert_eq!(
                error.raw_os_error(),
                Some(32),
                "native sharing lease required"
            );
        }
    })
    .unwrap()
    .unwrap();
    assert_eq!(snapshot.bytes.as_slice(), b"leased configuration");
    // The independently observable effects are blocked only while this read
    // owns its leases. Ordinary user writes and parent moves remain possible.
    fs::write(&path, b"later user edit").unwrap();
    fs::rename(&parent, root.path().join("moved-parent")).unwrap();
}

#[test]
fn windows_native_reader_rejects_live_writer_without_changing_source() {
    let root = crate::test_tempdir().unwrap();
    let path = root.path().join("native");
    borrowed(&path, b"original native bytes");
    let permissions = native_permissions(&path);
    let mut writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
    assert!(native_permissions::read_windows_artifact_bounded(&path, 1024).is_err());
    writer.write_all(b"changed").unwrap();
    drop(writer);
    let actual = fs::read(&path).unwrap();
    assert_eq!(
        native_permissions::read_windows_artifact_bounded(&path, 1024)
            .unwrap()
            .unwrap()
            .bytes
            .as_slice(),
        actual.as_slice()
    );
    assert_eq!(native_permissions(&path), permissions);
}

#[test]
fn windows_native_reader_rejects_acl_change_on_same_held_identity() {
    let root = crate::test_tempdir().unwrap();
    let path = root.path().join("native");
    borrowed(&path, b"unchanged configuration");
    let before = security::file_identity(&security::open_identity(&path, false).unwrap()).unwrap();
    let result = native_permissions::read_windows_artifact_after_open(&path, 1024, || {
        // WRITE_DAC changes permission facts on this same test-owned object;
        // the content and file ID remain equal, so byte comparison is insufficient.
        security::set_owner_only(&path, false).unwrap();
    });
    assert!(matches!(result, Err(LocalStorageError::InvalidData)));
    assert_eq!(fs::read(&path).unwrap(), b"unchanged configuration");
    assert_eq!(
        security::file_identity(&security::open_identity(&path, false).unwrap()).unwrap(),
        before
    );
    security::verify_owner_only(&path, false).unwrap();
}

#[test]
fn windows_native_reader_reopen_binds_identity_without_changing_fingerprint_contract() {
    let root = crate::test_tempdir().unwrap();
    let path = root.path().join("native");
    borrowed(&path, b"same published content");
    let first = native_permissions::read_windows_artifact_bounded(&path, 1024)
        .unwrap()
        .unwrap();
    let lease = security::open_read_lease(&path).unwrap();
    let identity = security::file_identity(lease.file()).unwrap();
    let reopened = lease.reopen_file().unwrap();
    assert_eq!(security::file_identity(&reopened).unwrap(), identity);
    lease.verify().unwrap();
    drop(reopened);
    drop(lease);
    durable::write_windows_atomic(
        &path,
        &first.bytes,
        first.windows_permissions.as_ref().unwrap(),
    )
    .unwrap();
    assert_ne!(
        security::file_identity(&security::open_identity(&path, false).unwrap()).unwrap(),
        identity
    );
    let second = native_permissions::read_windows_artifact_bounded(&path, 1024)
        .unwrap()
        .unwrap();
    assert_eq!(
        first.fingerprint, second.fingerprint,
        "released fingerprint owns bytes and native permissions"
    );
}

#[test]
fn windows_native_reader_private_restore_and_native_cipher_reopen_preserve_authority() {
    let root = crate::test_tempdir().unwrap();
    let path = root.path().join("native");
    borrowed(&path, b"original configuration");
    assert!(native_permissions::read_private_restore_file(&path).is_err());
    let store = artifact_store(root.path(), &path);
    let intent = ExternalEffectIntentV1::from_registered_adapter(
        "agent-setup",
        OwnedEffectKind::AgentArtifact,
        TARGET,
        store.current_external_fingerprint(TARGET).unwrap(),
        json!({"configured": true}),
        0o600,
        false,
    )
    .unwrap();
    let operation = OperationId::parse("op_44444444444444444444444444444444").unwrap();
    let protected = b"test-owned original native field";
    store
        .save_native_restore(&operation, &intent, protected)
        .unwrap();
    // Locate the single payload published into this fresh test-owned restore
    // root. Its filename remains the private producer's responsibility.
    let sealed = fs::read_dir(root.path().join("restore"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "native-restore")
        })
        .collect::<Vec<_>>();
    assert_eq!(sealed.len(), 1);
    let sealed = &sealed[0];
    let ciphertext = fs::read(sealed).unwrap();
    assert!(
        !ciphertext
            .windows(protected.len())
            .any(|window| window == protected)
    );
    drop(store);
    let reopened = artifact_store(root.path(), &path);
    assert_eq!(
        reopened
            .load_native_restore(&operation, &intent)
            .unwrap()
            .unwrap()
            .as_slice(),
        protected
    );
    assert_eq!(fs::read(sealed).unwrap(), ciphertext);
    let alias = root.path().join("restore-alias");
    fs::hard_link(sealed, &alias).unwrap();
    assert!(reopened.load_native_restore(&operation, &intent).is_err());
    fs::remove_file(alias).unwrap();
}
