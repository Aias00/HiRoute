#![cfg(windows)]
//! Native handle-relative I/O, settings commits and diagnostic writer regression gates.

use std::time::{Duration, Instant};

use hiroute_diagnostics::event::{DiagnosticEvent, ProcessRole, StageBegin, StartupStage};
use hiroute_diagnostics::files::{CURRENT_LOG_FILE, FileSafetyError, PrivateDir, SETTINGS_FILE};
use hiroute_diagnostics::level::DiagnosticLevel;
use hiroute_diagnostics::record::Component;
use hiroute_diagnostics::runtime::{DiagnosticRuntime, RuntimeConfig};
use hiroute_diagnostics::settings::SettingsStore;
use hiroute_platform::windows::security;

#[path = "../../platform/tests/support/windows_junction.rs"]
mod junction;

#[test]
fn held_directory_read_write_enumerate_rename_and_remove_ignore_a_path_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("directory 空格");
    let held = PrivateDir::open_or_create(&path).unwrap();
    let original_identity = held.identity();
    let mut record = held.create_new("record").unwrap();
    record.replace_contents(b"held-original").unwrap();
    record.sync().unwrap();
    let identity = record.identity();
    drop(record);
    let renamed = temp.path().join("renamed");
    std::fs::rename(&path, &renamed).unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("record"), b"replacement-must-not-be-touched").unwrap();
    std::fs::write(path.join("sentinel"), b"replacement-only").unwrap();
    assert_eq!(held.identity(), original_identity);
    let mut read = held.open_read("record").unwrap().unwrap();
    assert_eq!(read.read_prefix(64).unwrap(), b"held-original");
    drop(read);
    assert_eq!(held.list_names().unwrap(), ["record"]);
    held.rename_verified("record", "after-rename", identity)
        .unwrap();
    let mut writer = held.create_new("new").unwrap();
    writer.append(b"new-in-original").unwrap();
    writer.sync().unwrap();
    held.remove_verified("after-rename", identity).unwrap();
    assert_eq!(
        std::fs::read(path.join("record")).unwrap(),
        b"replacement-must-not-be-touched"
    );
    assert_eq!(
        std::fs::read(path.join("sentinel")).unwrap(),
        b"replacement-only"
    );
    assert!(!path.join("new").exists());
    assert_eq!(
        std::fs::read(renamed.join("new")).unwrap(),
        b"new-in-original"
    );
    held.sync().unwrap();
}

#[test]
fn existing_accessible_native_acl_is_preserved_and_new_objects_are_private() {
    let temp = tempfile::tempdir().unwrap();
    // Ordinary inherited TEMP ACLs intentionally are not owner-only. The adapter must
    // admit this native directory without tightening or replacing its ACL.
    let dir = PrivateDir::open_existing(temp.path()).unwrap();
    std::fs::write(temp.path().join("native"), b"existing-native").unwrap();
    assert!(dir.open_read("native").unwrap().is_some());
    let new = dir.create_new("created-private").unwrap();
    security::verify_owner_only(&temp.path().join(new.name()), false).unwrap();
    let child = dir.child_dir("created-child").unwrap();
    security::verify_owner_only(child.path(), true).unwrap();
}

#[test]
fn junction_ancestor_and_child_cannot_redirect_diagnostic_operations() {
    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("sentinel"), b"outside-must-be-untouched").unwrap();
    let root = temp.path().join("diagnostics");
    let held = PrivateDir::open_or_create(&root).unwrap();
    let alias = root.join("redirect");
    junction::create(&alias, &outside).expect("owned junction fixture must be created");
    assert!(PrivateDir::open_existing(&alias).is_err());
    assert!(PrivateDir::open_or_create(&alias.join("created")).is_err());
    assert!(held.child_dir("redirect").is_err());
    assert!(held.open_read("redirect").is_err());
    let source = held.create_new("source").unwrap();
    assert!(
        held.rename_verified("source", "redirect", source.identity())
            .is_err()
    );
    assert_eq!(
        std::fs::read(outside.join("sentinel")).unwrap(),
        b"outside-must-be-untouched"
    );
    assert!(!outside.join("created").exists());
    std::fs::remove_dir(alias).unwrap();
}

#[test]
fn hardlinks_and_replaced_file_identities_are_refused_before_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let dir = PrivateDir::open_existing(temp.path()).unwrap();
    let mut file = dir.create_new("owned").unwrap();
    file.append(b"original").unwrap();
    let identity = file.identity();
    std::fs::hard_link(temp.path().join("owned"), temp.path().join("alias")).unwrap();
    assert!(matches!(
        file.append(b"forbidden"),
        Err(FileSafetyError::UnsafeFile)
    ));
    assert!(dir.open_read("owned").is_err());
    assert!(dir.remove_verified("owned", identity).is_err());
    std::fs::remove_file(temp.path().join("alias")).unwrap();
    drop(file);
    std::fs::rename(temp.path().join("owned"), temp.path().join("moved")).unwrap();
    std::fs::write(temp.path().join("owned"), b"replacement").unwrap();
    assert!(matches!(
        dir.rename_verified("owned", "target", identity),
        Err(FileSafetyError::IdentityChanged)
    ));
    assert!(matches!(
        dir.remove_verified("owned", identity),
        Err(FileSafetyError::IdentityChanged)
    ));
    assert_eq!(
        std::fs::read(temp.path().join("owned")).unwrap(),
        b"replacement"
    );
    std::fs::create_dir(temp.path().join("target-dir")).unwrap();
    let moved = dir.open_read("moved").unwrap().unwrap();
    assert!(
        dir.rename_verified("moved", "target-dir", moved.identity())
            .is_err()
    );
    assert_eq!(
        std::fs::read(temp.path().join("moved")).unwrap(),
        b"original"
    );
}

#[test]
fn held_writer_can_atomically_replace_and_delete_without_share_mode_self_lock() {
    let temp = tempfile::tempdir().unwrap();
    let dir = PrivateDir::open_existing(temp.path()).unwrap();
    let mut target = dir.create_new(SETTINGS_FILE).unwrap();
    target.replace_contents(b"old").unwrap();
    target.sync().unwrap();
    let mut source = dir.create_new("settings.tmp").unwrap();
    source.replace_contents(b"new").unwrap();
    source.sync().unwrap();
    let identity = source.identity();
    dir.rename_verified("settings.tmp", SETTINGS_FILE, identity)
        .unwrap();
    assert_eq!(
        std::fs::read(temp.path().join(SETTINGS_FILE)).unwrap(),
        b"new"
    );
    dir.remove_verified(SETTINGS_FILE, identity).unwrap();
    // POSIX handle unlink must free the namespace while `source` remains held.
    let mut replacement = dir.create_new(SETTINGS_FILE).unwrap();
    replacement.replace_contents(b"replacement").unwrap();
    assert_eq!(
        std::fs::read(temp.path().join(SETTINGS_FILE)).unwrap(),
        b"replacement"
    );
}

#[test]
fn lock_contention_settings_commit_and_real_runtime_writer_are_supported() {
    let temp = tempfile::tempdir().unwrap();
    let dir = PrivateDir::open_or_create(&temp.path().join("diagnostics")).unwrap();
    let first_lock = dir.open_lock("test.lock").unwrap();
    first_lock.try_lock_exclusive().unwrap();
    let second_lock = dir.open_lock("test.lock").unwrap();
    assert!(matches!(
        second_lock.try_lock_exclusive(),
        Err(FileSafetyError::Locked)
    ));
    first_lock.unlock().unwrap();
    second_lock.try_lock_exclusive().unwrap();
    second_lock.unlock().unwrap();
    let root = dir.path().to_owned();
    let settings = SettingsStore::new(dir);
    let initial = settings.load();
    assert!(!initial.present && initial.error.is_none());
    let saved = settings.save(0, DiagnosticLevel::Debug).unwrap();
    assert_eq!(saved.revision, 1);
    let second = settings.save(1, DiagnosticLevel::Info).unwrap();
    assert_eq!(second.revision, 2);
    let loaded = settings.load();
    assert_eq!(loaded.settings, second);
    assert!(loaded.error.is_none());
    let runtime = DiagnosticRuntime::start(RuntimeConfig {
        root: root.clone(),
        role: ProcessRole::Daemon,
        component: Component::Daemon,
        parent_session_id: None,
        level_override: Some(DiagnosticLevel::Debug),
    });
    runtime.emit(DiagnosticEvent::StageBegin(StageBegin {
        stage: StartupStage::ReadyWait,
    }));
    runtime.shutdown();
    let path = root.join("daemon").join(CURRENT_LOG_FILE);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let bytes =
        std::fs::read(path).expect("real runtime must write diagnostics, not degrade Unsupported");
    assert!(!bytes.is_empty());
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let _: serde_json::Value = serde_json::from_slice(line).unwrap();
    }
}
