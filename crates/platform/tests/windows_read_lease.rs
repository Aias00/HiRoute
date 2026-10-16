#![cfg(windows)]

use hiroute_platform::{file_security, windows::security};

#[path = "support/windows_junction.rs"]
mod junction;

#[test]
fn read_lease_preserves_borrowed_acl_and_pins_file_and_all_ancestors() {
    let root = tempfile::tempdir().unwrap();
    let top = root.path().join("top");
    let parent = top.join("nested");
    std::fs::create_dir_all(&parent).unwrap();
    let location = parent.join("borrowed.json");
    std::fs::write(&location, b"borrowed bytes").unwrap();
    let before = security::open_identity(&location, false).unwrap();
    let identity = security::file_identity(&before).unwrap();
    let permissions = file_security::capture(&before).unwrap();
    drop(before);

    let lease = security::open_read_lease(&location).unwrap();
    assert_eq!(security::file_identity(lease.file()).unwrap(), identity);
    assert_eq!(file_security::capture(lease.file()).unwrap(), permissions);
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .open(&location)
            .is_err()
    );
    assert!(std::fs::rename(&location, parent.join("replacement")).is_err());
    assert!(std::fs::rename(&parent, top.join("moved")).is_err());
    assert!(std::fs::rename(&top, root.path().join("moved-top")).is_err());
    lease.verify().unwrap();
    assert_eq!(
        security::file_identity(&lease.reopen_file().unwrap()).unwrap(),
        identity
    );
    drop(lease);

    let after = security::open_identity(&location, false).unwrap();
    assert_eq!(file_security::capture(&after).unwrap(), permissions);
    drop(after);
    std::fs::rename(&parent, top.join("moved")).unwrap();
    std::fs::write(top.join("moved/borrowed.json"), b"writer resumes").unwrap();
}

#[test]
fn read_lease_rejects_aliases_without_mutating_their_targets() {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let target = outside.join("target.json");
    std::fs::write(&target, b"preserved").unwrap();
    let held = security::open_identity(&target, false).unwrap();
    let identity = security::file_identity(&held).unwrap();
    let permissions = file_security::capture(&held).unwrap();
    drop(held);
    let alias = root.path().join("alias");
    junction::create(&alias, &outside).unwrap();
    assert!(security::open_read_lease(&alias.join("target.json")).is_err());
    assert!(security::open_read_lease(&alias).is_err());
    let linked = root.path().join("linked.json");
    std::fs::hard_link(&target, &linked).unwrap();
    assert!(security::open_read_lease(&linked).is_err());
    assert!(security::open_read_lease(&target).is_err());
    let held = security::open_identity(&target, false).unwrap();
    assert_eq!(security::file_identity(&held).unwrap(), identity);
    assert_eq!(file_security::capture(&held).unwrap(), permissions);
    assert_eq!(std::fs::read(&target).unwrap(), b"preserved");
    drop(held);
    std::fs::remove_dir(alias).unwrap();
}
