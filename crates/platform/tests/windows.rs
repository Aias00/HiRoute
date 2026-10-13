#![cfg(windows)]

use hiroute_platform::windows::{pipe, security};
#[path = "support/windows_junction.rs"]
mod junction;

#[test]
fn ancestor_junction_cannot_redirect_private_read_or_creation() {
    let fixture = tempfile::tempdir().unwrap();
    let outside = fixture.path().join("outside");
    let held = security::create_owner_only_directory(&outside).unwrap();
    let original = outside.join("original");
    let file = security::create_owner_only_file(&original).unwrap();
    drop(file);
    // Child creation above must succeed while its DELETE-capable parent is held.
    // Release that pin only for the independent junction fixture setup, then
    // exercise rejection of the published alias.
    drop(held);
    let alias = fixture.path().join("alias");
    junction::create(&alias, &outside).expect("owned junction fixture must be created");
    assert!(security::open_owner_only(&alias.join("original"), false).is_err());
    assert!(security::create_owner_only_file(&alias.join("new")).is_err());
    assert!(pipe::name(&alias.join("control.sock")).is_err());
    assert!(!outside.join("new").exists());
    assert!(original.exists());
    std::fs::remove_dir(&alias).unwrap();
}
use std::io::{Read, Write};
use std::time::Duration;

#[test]
fn private_file_identity_is_held_and_reparse_free() {
    let fixture = tempfile::tempdir().unwrap();
    let directory_path = fixture.path().join("held parent");
    let directory = security::create_owner_only_directory(&directory_path).unwrap();
    let mut child = security::create_owner_only_file_at(&directory, "child".as_ref()).unwrap();
    child.write_all(b"owned-child").unwrap();
    assert!(std::fs::rename(&directory_path, fixture.path().join("moved parent")).is_err());
    security::delete_by_handle(&child).unwrap();
    drop(child);
    security::delete_by_handle(&directory).unwrap();
    drop(directory);
    assert!(!directory_path.exists());
    // An existing traversal handle may permit rename; relative operations must
    // still address that object, never a replacement at its original path.
    std::fs::create_dir(&directory_path).unwrap();
    let identity = security::open_identity(&directory_path, true).unwrap();
    let moved_path = fixture.path().join("moved parent");
    std::fs::rename(&directory_path, &moved_path).unwrap();
    std::fs::create_dir(&directory_path).unwrap();
    let child = security::create_owner_only_file_at(&identity, "bound-child".as_ref()).unwrap();
    assert!(moved_path.join("bound-child").exists());
    assert!(!directory_path.join("bound-child").exists());
    drop(child);
    drop(identity);
    let path = fixture.path().join("私有 文件");
    let mut file = security::create_owner_only_file(&path).unwrap();
    file.write_all(b"synthetic-private-data").unwrap();
    file.sync_all().unwrap();
    security::verify_owner_only_handle(&file, false).unwrap();
    let identity = security::file_identity(&file).unwrap();
    assert_eq!(
        identity,
        security::file_identity(&file.try_clone().unwrap()).unwrap()
    );
    let renamed = fixture.path().join("renamed");
    assert!(std::fs::rename(&path, &renamed).is_err());
    let other = security::create_owner_only_file(&fixture.path().join("other")).unwrap();
    assert_ne!(identity, security::file_identity(&other).unwrap());
    drop(file);
    let file = security::open_owner_only(&path, false).unwrap();
    assert_eq!(identity, security::file_identity(&file).unwrap());
    assert!(security::open_identity(fixture.path(), false).is_err());
}

#[test]
fn pipe_exclusivity_bidirectional_frames_and_peer_identity() {
    let fixture = tempfile::tempdir().unwrap();
    let endpoint = fixture.path().join("control.sock");
    let listener = pipe::Listener::bind(&endpoint).unwrap();
    assert!(pipe::Listener::bind(&endpoint).is_err());
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        listener.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
    listener.set_nonblocking(false).unwrap();
    let client_endpoint = endpoint.clone();
    let client = std::thread::spawn(move || {
        let mut client = pipe::Stream::connect(&client_endpoint).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        assert_eq!(pipe::peer_pid(&client, false).unwrap(), std::process::id());
        client.write_all(b"hello\n").unwrap();
        let mut response = [0; 6];
        client.read_exact(&mut response).unwrap();
        assert_eq!(&response, b"reply\n");
        client.write_all(b"ack\n").unwrap();
    });
    let (mut stream, ()) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert_eq!(pipe::peer_pid(&stream, true).unwrap(), std::process::id());
    let mut request = [0; 6];
    stream.read_exact(&mut request).unwrap();
    assert_eq!(&request, b"hello\n");
    stream.write_all(b"reply\n").unwrap();
    let mut ack = [0; 4];
    stream.read_exact(&mut ack).unwrap();
    assert_eq!(&ack, b"ack\n");
    client.join().unwrap();
    drop(stream);
    drop(listener);
    assert!(!endpoint.exists());
    let _rebound = pipe::Listener::bind(&endpoint).unwrap();
}

#[test]
fn pipe_deadline_cancels_read_without_consuming_the_next_frame() {
    let fixture = tempfile::tempdir().unwrap();
    let endpoint = fixture.path().join("deadline.sock");
    let listener = pipe::Listener::bind(&endpoint).unwrap();
    let mut client = pipe::Stream::connect(&endpoint).unwrap();
    let (mut server, ()) = listener.accept().unwrap();
    server
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let mut byte = [0];
    assert_eq!(
        server.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    client.write_all(b"x").unwrap();
    server.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"x");
}
#[test]
fn native_permissions_survive_atomic_restore_and_detect_acl_changes() {
    use hiroute_platform::{durable, file_security};
    let fixture = tempfile::tempdir().unwrap();
    let target = fixture.path().join("native.json");
    std::fs::write(&target, b"original").unwrap();
    let original =
        file_security::capture(&security::open_identity(&target, false).unwrap()).unwrap();
    durable::write_private_atomic(&target, b"managed").unwrap();
    let managed =
        file_security::capture(&security::open_owner_only(&target, false).unwrap()).unwrap();
    assert_eq!(managed, file_security::private_defaults(false).unwrap());
    assert_ne!(managed.descriptor, original.descriptor);
    durable::write_windows_atomic(&target, b"original", &original).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
    assert_eq!(
        file_security::capture(&security::open_identity(&target, false).unwrap()).unwrap(),
        original
    );

    // This native component gate already runs with an administrator token.
    // Deliberately assign BA ownership to a new, known fixture; a standard
    // product user does not need this fixture-creation permission.
    let foreign = fixture.path().join("administrators-owned.json");
    let user = hiroute_platform::windows::background::current_sid_string().unwrap();
    let permissions = file_security::WindowsPermissions {
        descriptor: format!("O:BAD:P(A;;FA;;;{user})(A;;FR;;;WD)"),
        read_only: false,
    };
    durable::write_windows_atomic(&foreign, b"foreign-owner-original", &permissions)
        .expect("administrator native fixture must be allowed to assign BA ownership");
    let before =
        file_security::capture(&security::open_identity(&foreign, false).unwrap()).unwrap();
    assert_eq!(
        security::set_owner_only(&foreign, false)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied,
        "a DACL-tightening setter cannot adopt the BA-owned fixture"
    );
    assert_eq!(
        file_security::capture(&security::open_identity(&foreign, false).unwrap()).unwrap(),
        before,
        "refusing a foreign owner must leave its exact native descriptor unchanged"
    );
    assert_eq!(std::fs::read(foreign).unwrap(), b"foreign-owner-original");
}
