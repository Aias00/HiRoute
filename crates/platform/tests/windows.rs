#![cfg(windows)]

use hiroute_platform::windows::{pipe, security};

#[test]
fn ancestor_junction_cannot_redirect_private_read_or_creation() {
    let fixture = tempfile::tempdir().unwrap();
    let outside = fixture.path().join("outside");
    let held = security::create_owner_only_directory(&outside).unwrap();
    let original = outside.join("original");
    let file = security::create_owner_only_file(&original).unwrap();
    drop(file);
    let alias = fixture.path().join("alias");
    let cmd =
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe");
    let result = std::process::Command::new(cmd)
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&alias)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "owned junction fixture must be created"
    );
    assert!(security::open_owner_only(&alias.join("original"), false).is_err());
    assert!(security::create_owner_only_file(&alias.join("new")).is_err());
    assert!(pipe::name(&alias.join("control.sock")).is_err());
    assert!(!outside.join("new").exists());
    assert!(original.exists());
    std::fs::remove_dir(&alias).unwrap();
    drop(held);
}
use std::io::{Read, Write};
use std::time::Duration;

#[test]
fn private_file_identity_is_held_and_reparse_free() {
    let fixture = tempfile::tempdir().unwrap();
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
