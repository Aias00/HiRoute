use super::*;
use crate::{LocalStorageSet, StorageStartupOptions};

fn fixture() -> (crate::TestTempDir, PathBuf, StorageStartupOptions) {
    let temporary = crate::test_tempdir().unwrap();
    let root = temporary.path().join("storage");
    drop(LocalStorageSet::open_for_daemon_startup(&root).unwrap());
    let observation = root.join("observation");
    crate::prepare_owner_directory(&observation).unwrap();
    let db = Connection::open(observation.join("activity.db")).unwrap();
    db.execute_batch("CREATE TABLE observation_meta(key TEXT PRIMARY KEY,value TEXT); INSERT INTO observation_meta VALUES('schema_version','4'); CREATE TABLE retained(id INTEGER PRIMARY KEY,body BLOB); INSERT INTO retained VALUES(17,X'00ff616263');").unwrap();
    drop(db);
    for (name, body) in [
        ("activity.db-wal", b"retained sidecar".as_slice()),
        ("activity.db-shm", b"retained shared memory".as_slice()),
        (
            "conversation-content/blobs/body",
            b"original body\0\xff".as_slice(),
        ),
        (
            "conversation-content/staging/pending",
            b"pending capture".as_slice(),
        ),
    ] {
        let path = observation.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    let package = temporary.path().join("old-package");
    crate::prepare_owner_directory(&package).unwrap();
    fs::write(package.join("old-daemon"), b"matching program fixture").unwrap();
    (
        temporary,
        root,
        StorageStartupOptions {
            preserve_observation_source: true,
            recovery_package: Some(package),
            ..Default::default()
        },
    )
}

fn backup(root: &Path) -> PathBuf {
    let link: Value =
        serde_json::from_slice(&fs::read(root.join("observation-source-backup.json")).unwrap())
            .unwrap();
    crate::upgrade_backup_root(root)
        .unwrap()
        .parent()
        .unwrap()
        .join(link["backup_name"].as_str().unwrap())
}

#[test]
fn sealing_keeps_the_original_directory_and_never_opens_historical_files() {
    let (_temporary, root, options) = fixture();
    let before = identity(&root.join("observation")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        // A recursive reader/copy/hash must fail here; sealing preserves the
        // stopped opaque tree without following this old unknown entry.
        fs::set_permissions(
            root.join("observation/conversation-content/blobs/body"),
            fs::Permissions::from_mode(0o0),
        )
        .unwrap();
        symlink("missing-target", root.join("observation/unopened-link")).unwrap();
    }
    drop(LocalStorageSet::open_for_daemon_startup_with_options(&root, &options).unwrap());
    let backup = backup(&root);
    let sealed = backup.join("source/storage/observation");
    assert!(!root.join("observation").exists());
    assert!(LocalStorageSet::observation_cutover_pending(&root));
    assert_eq!(identity(&sealed).unwrap(), before);
    assert_eq!(
        fs::read(sealed.join("activity.db-wal")).unwrap(),
        b"retained sidecar"
    );
    assert_eq!(
        fs::read(sealed.join("activity.db-shm")).unwrap(),
        b"retained shared memory"
    );
    assert_eq!(
        fs::read(sealed.join("conversation-content/staging/pending")).unwrap(),
        b"pending capture"
    );
    super::super::verify_sealed(&backup).unwrap();
    let manifest = super::super::manifest(&backup).unwrap();
    assert!(
        !manifest["files"]
            .as_object()
            .unwrap()
            .keys()
            .any(|key| key.starts_with("storage/observation/"))
    );
    assert_eq!(
        manifest["sealed_observation"]["contents"],
        "preserved_without_recursive_verification"
    );
    assert!(backup.join("source/storage/live/control.db").is_file());
    assert!(backup.join("source/storage/master-key").is_file());
    assert!(backup.join("source/recovery-package/old-daemon").is_file());
}

#[test]
fn restart_finishes_the_same_cutover_at_each_namespace_checkpoint() {
    for phase in ["prepared", "renamed", "published"] {
        let (_temporary, root, options) = fixture();
        let before = identity(&root.join("observation")).unwrap();
        FAULT.with(|fault| fault.set(Some(phase)));
        let result = LocalStorageSet::open_for_daemon_startup_with_options(&root, &options);
        FAULT.with(|fault| fault.set(None));
        assert!(result.is_err(), "{phase}");
        let link = fs::read(root.join("observation-source-backup.json")).unwrap();
        drop(LocalStorageSet::open_for_daemon_startup_with_options(&root, &options).unwrap());
        let backup = backup(&root);
        assert_eq!(
            identity(&backup.join("source/storage/observation")).unwrap(),
            before
        );
        assert_eq!(
            fs::read(root.join("observation-source-backup.json")).unwrap(),
            link
        );
        assert!(!backup.join("source-staging").exists());
        super::super::verify_sealed(&backup).unwrap();
    }
}

#[test]
fn conflicting_live_root_or_replaced_sealed_identity_never_resets_either() {
    let (_temporary, root, options) = fixture();
    FAULT.with(|fault| fault.set(Some("renamed")));
    assert!(LocalStorageSet::open_for_daemon_startup_with_options(&root, &options).is_err());
    FAULT.with(|fault| fault.set(None));
    let backup = backup(&root);
    fs::create_dir(root.join("observation")).unwrap();
    fs::write(
        root.join("observation/foreign"),
        b"preserve conflicting generation",
    )
    .unwrap();
    assert!(LocalStorageSet::open_for_daemon_startup_with_options(&root, &options).is_err());
    assert_eq!(
        fs::read(root.join("observation/foreign")).unwrap(),
        b"preserve conflicting generation"
    );
    fs::rename(root.join("observation"), root.join("conflicting-preserved")).unwrap();
    let sealed = backup.join("source-staging/storage/observation");
    fs::rename(&sealed, sealed.with_file_name("retained-original")).unwrap();
    fs::create_dir(&sealed).unwrap();
    assert!(LocalStorageSet::open_for_daemon_startup_with_options(&root, &options).is_err());
    assert!(
        sealed
            .with_file_name("retained-original")
            .join("activity.db")
            .exists()
    );
}
