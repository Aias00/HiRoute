use super::*;
use hiroute_host_runtime::ReleaseNotes;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};

fn directory(path: &Path, mode: u32) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
fn fixture(root: &Path) -> (PathBuf, Handoff) {
    let install = root.join("Applications");
    directory(&install, 0o775);
    let current = install.join("HiRoute.app");
    let staged = install.join(".HiRoute-update-fixture/HiRoute.app");
    directory(&current, 0o755);
    directory(staged.parent().unwrap(), 0o700);
    directory(&staged, 0o755);
    let data_root = root.join("data");
    directory(&data_root, 0o700);
    let handoff = data_root.join("updates/install-fixture/handoff.json");
    directory(handoff.parent().unwrap(), 0o700);
    let identity = |app_path: PathBuf| DesktopPackageIdentity {
        app_path,
        version: "0.3.0".into(),
        revision: "c".repeat(40),
        architecture: "arm64".into(),
        distribution: "self-signed".into(),
        inventory_sha256: "a".repeat(64),
        binaries: BTreeMap::new(),
    };
    let record = Handoff {
        schema: "hiroute.desktop-upgrade-handoff/v1".into(),
        parent_pid: std::process::id(),
        old_identity: identity(current.clone()),
        new_identity: identity(staged.clone()),
        current,
        staged,
        previous: install.join(".HiRoute-previous-fixture.app"),
        data_root,
        release: DesktopRelease {
            version: "0.3.0".into(),
            notes: ReleaseNotes {
                zh: String::new(),
                en: String::new(),
            },
            architecture: "arm64".into(),
            distribution: "self-signed".into(),
            filename: "HiRoute-0.3.0-cccccccccccc-macos-arm64-trial.dmg".into(),
            sha256: "b".repeat(64),
            size: 1,
        },
    };
    write_private(&handoff, &serde_json::to_vec(&record).unwrap()).unwrap();
    (handoff, record)
}

#[test]
fn admin_writable_install_parent_is_not_a_diagnostics_private_root() {
    let temporary = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temporary.path()).unwrap();
    let (path, record) = fixture(&root);
    // Frozen 0.2.0 rejected non-sticky ancestors with group/other write bits.
    // Current diagnostics has a different policy; do not use it as an old reader.
    let mode = fs::metadata(record.current.parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_ne!(mode & 0o022, 0);
    assert_eq!(mode & 0o1000, 0);
    let loaded = load_for_app(&path, &record.current).unwrap();
    assert_eq!(loaded.staged, record.staged);
    assert_eq!(
        fs::metadata(record.staged.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn handoff_still_binds_the_current_app_and_refuses_symlinked_handoff() {
    let temporary = tempfile::tempdir().unwrap();
    let (path, record) = fixture(&fs::canonicalize(temporary.path()).unwrap());
    assert_eq!(
        load_for_app(&path, &record.staged).err().unwrap(),
        "UPGRADE_INSTALLER_INVALID"
    );
    fs::write(&record.previous, b"existing backup").unwrap();
    assert_eq!(
        load_for_app(&path, &record.current).err().unwrap(),
        "UPGRADE_INSTALLER_INVALID"
    );
    fs::remove_file(&record.previous).unwrap();
    let saved = path.with_file_name("saved.json");
    fs::rename(&path, &saved).unwrap();
    std::os::unix::fs::symlink(&saved, &path).unwrap();
    assert!(load_for_app(&path, &record.current).is_err());
}

#[test]
fn markers_publish_complete_private_bytes_without_overwriting() {
    let temporary = tempfile::tempdir().unwrap();
    let marker = temporary.path().join("decision");
    write_private(&marker, b"complete decision\n").unwrap();
    assert_eq!(fs::read(&marker).unwrap(), b"complete decision\n");
    assert_eq!(
        fs::metadata(&marker).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        write_private(&marker, b"replacement").unwrap_err(),
        "UPGRADE_INSTALLER_WRITE_FAILED"
    );
    assert_eq!(fs::read(&marker).unwrap(), b"complete decision\n");
    assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 1);
}
