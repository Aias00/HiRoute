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
    // Observe default BA ownership of a new, known fixture; a standard
    // product user does not need this fixture-creation permission.
    let foreign = fixture.path().join("administrators-owned.json");
    std::fs::write(&foreign, b"foreign-owner-original").unwrap();
    let before =
        file_security::capture(&security::open_identity(&foreign, false).unwrap()).unwrap();
    assert!(
        before
            .descriptor
            .strip_prefix("O:BA")
            .is_some_and(|rest| rest.is_empty()
                || rest.starts_with("G:")
                || rest.starts_with("D:")
                || rest.starts_with("S:")),
        "administrator native fixture requires actual BA owner: {}",
        before.descriptor
    );
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

fn shared_native_file(path: &std::path::Path, access: u32) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    std::fs::OpenOptions::new()
        .read(true)
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
}

fn held_bytes(file: &mut std::fs::File) -> Vec<u8> {
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    bytes
}

// Teardown modifies attributes only through an admitted held fixture object.
// The tested publication never clears read-only by pathname.
fn unseal_fixture(file: &std::fs::File) {
    set_fixture_read_only(file, false);
}

fn set_fixture_read_only(file: &std::fs::File, read_only: bool) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_BASIC_INFO, FileBasicInfo,
        SetFileInformationByHandle,
    };
    let information = FILE_BASIC_INFO {
        FileAttributes: if read_only {
            FILE_ATTRIBUTE_READONLY
        } else {
            FILE_ATTRIBUTE_NORMAL
        },
        ..Default::default()
    };
    assert_ne!(
        unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&information as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0,
        "owned native fixture attribute update"
    );
}

fn assert_readonly_user_and_system_fixture(file: &std::fs::File, user_sid: &str) {
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, GetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
        GetSecurityDescriptorControl, OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ALL_ACCESS, FILE_ATTRIBUTE_READONLY, FILE_GENERIC_READ,
    };
    struct Allocation(*mut std::ffi::c_void);
    impl Drop for Allocation {
        fn drop(&mut self) {
            // SAFETY: both SID conversion and GetSecurityInfo use LocalAlloc.
            unsafe { LocalFree(self.0) };
        }
    }
    let decode_sid = |sid: &str| {
        let wide: Vec<u16> = sid.encode_utf16().chain([0]).collect();
        let mut decoded = std::ptr::null_mut();
        // SAFETY: bounded fixture SID and output remain live for conversion.
        assert_ne!(
            unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut decoded) },
            0
        );
        Allocation(decoded)
    };
    let user = decode_sid(user_sid);
    let system = decode_sid("S-1-5-18");
    let mut owner = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: all returned pointers are retained by the descriptor allocation.
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    let descriptor = Allocation(descriptor);
    assert!(!owner.is_null() && !dacl.is_null());
    // SAFETY: both SIDs are validated Windows outputs held above.
    assert_ne!(
        unsafe { EqualSid(owner, user.0) },
        0,
        "fixture owner must remain TokenUser"
    );
    let mut control = 0;
    let mut revision = 0;
    assert_ne!(
        unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) },
        0
    );
    assert_ne!(control & SE_DACL_PROTECTED, 0);
    assert_eq!(
        unsafe { (*dacl).AceCount },
        2,
        "fixture must have exactly two ACEs"
    );
    let mut user_full = false;
    let mut system_read = false;
    for index in 0..2 {
        let mut ace = std::ptr::null_mut();
        assert_ne!(unsafe { GetAce(dacl, index, &mut ace) }, 0);
        assert!(!ace.is_null());
        // SAFETY: GetSecurityInfo and GetAce supplied a native, validated ACL.
        let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        assert_eq!(ace.Header.AceType, 0, "ACCESS_ALLOWED_ACE_TYPE required");
        assert_eq!(
            ace.Header.AceFlags, 0,
            "no inherited or propagating fixture ACEs"
        );
        let sid = (&ace.SidStart as *const u32).cast_mut().cast();
        if unsafe { EqualSid(sid, user.0) } != 0 {
            assert!(!user_full, "duplicate user ACE");
            assert_eq!(ace.Mask, FILE_ALL_ACCESS);
            user_full = true;
        } else {
            assert_ne!(unsafe { EqualSid(sid, system.0) }, 0, "foreign fixture ACE");
            assert!(!system_read, "duplicate SYSTEM ACE");
            assert_eq!(ace.Mask, FILE_GENERIC_READ);
            system_read = true;
        }
    }
    assert!(user_full && system_read);
    assert_ne!(
        file.metadata().unwrap().file_attributes() & FILE_ATTRIBUTE_READONLY,
        0
    );
}

fn set_fixture_dacl(file: &std::fs::File, sddl: &str) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SE_FILE_OBJECT, SetSecurityInfo,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    let wide: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
    let mut descriptor = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        },
        0
    );
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = std::ptr::null_mut();
    let decoded =
        unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) };
    let result = if decoded != 0 && present != 0 && !dacl.is_null() {
        unsafe {
            SetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null_mut(),
            )
        }
    } else {
        u32::MAX
    };
    unsafe {
        LocalFree(descriptor);
    }
    assert_eq!(result, 0, "explicit-owner fixture DACL update");
}

fn remove_uncommitted_fixture_stages(directory: &std::path::Path) {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, READ_CONTROL,
    };
    // Definite failed publication may retain its unique uncommitted stage.
    // Only clean those fixture-owned objects, after all acceptance assertions.
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|name| name.starts_with(".hiroute-write-") && name.ends_with(".tmp"))
        {
            let file = shared_native_file(
                &entry.path(),
                READ_CONTROL | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
            )
            .unwrap();
            security::verify_owner_only_handle(&file, false).unwrap();
            unseal_fixture(&file);
            drop(file);
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
}

#[test]
fn sealed_readonly_artifact_rename_preserves_bytes_identity_and_native_permissions() {
    use hiroute_platform::{durable, file_security};
    use windows_sys::Win32::Foundation::GENERIC_READ;
    use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("native sealed artifact");
    let _parent = security::create_owner_only_directory(&directory).unwrap();
    let source = directory.join("completed source");
    let target = directory.join("published artifact");
    let original = file_security::private_defaults(true).unwrap();
    durable::write_windows_atomic(&source, b"complete-sealed-artifact", &original).unwrap();
    let mut held_source = shared_native_file(&source, GENERIC_READ).unwrap();
    let original_identity = security::file_identity(&held_source).unwrap();
    assert_eq!(file_security::capture(&held_source).unwrap(), original);
    assert!(!target.exists());

    // The producer above has already synced the complete bytes and descriptor.
    // A sealed source must not be reopened with GENERIC_WRITE or unsealed first.
    durable::rename_windows_sealed_file(&source, &target).unwrap();
    assert!(!source.exists());
    let published = security::open_owner_only(&target, false).unwrap();
    assert_eq!(
        security::file_identity(&published).unwrap(),
        original_identity
    );
    assert_eq!(file_security::capture(&published).unwrap(), original);
    assert_eq!(std::fs::read(&target).unwrap(), b"complete-sealed-artifact");
    drop(published);

    durable::write_windows_atomic(
        &source,
        b"different-object-at-former-source",
        &file_security::private_defaults(false).unwrap(),
    )
    .unwrap();
    let replacement = security::open_owner_only(&source, false).unwrap();
    assert_ne!(
        security::file_identity(&replacement).unwrap(),
        original_identity
    );
    assert_eq!(held_bytes(&mut held_source), b"complete-sealed-artifact");
    assert_eq!(file_security::capture(&held_source).unwrap(), original);
    assert_eq!(
        std::fs::read(&source).unwrap(),
        b"different-object-at-former-source"
    );
    drop(replacement);
    drop(held_source);
    unseal_fixture(&shared_native_file(&target, FILE_WRITE_ATTRIBUTES).unwrap());
    drop(_parent);
    fixture.close().unwrap();
}

#[test]
fn readonly_atomic_restore_preserves_permissions_and_requires_target_write_attributes() {
    use hiroute_platform::{durable, file_security};
    use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, GENERIC_READ};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, READ_CONTROL, WRITE_DAC,
    };
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("native readonly restoration");
    let _parent = security::create_owner_only_directory(&directory).unwrap();
    let sid = hiroute_platform::windows::background::current_sid_string().unwrap();
    for read_only in [false, true] {
        let target = directory.join(format!("restore-{read_only}"));
        // Use an actual, independently checked native descriptor as recovery
        // input. Handwritten SDDL is not a promised encoder spelling: Windows
        // may serialize SID aliases, rights and control bits differently.
        let mut initial = security::create_owner_only_file(&target).unwrap();
        initial.write_all(b"old-native-bytes").unwrap();
        initial.sync_all().unwrap();
        security::verify_owner_only_handle(&initial, false).unwrap();
        let initial_identity = security::file_identity(&initial).unwrap();
        drop(initial);
        let fixture_control = shared_native_file(
            &target,
            READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
        )
        .unwrap();
        assert_eq!(
            security::file_identity(&fixture_control).unwrap(),
            initial_identity
        );
        set_fixture_dacl(&fixture_control, &format!("D:P(A;;FA;;;{sid})(A;;FR;;;SY)"));
        set_fixture_read_only(&fixture_control, true);
        assert_readonly_user_and_system_fixture(&fixture_control, &sid);
        let old_permissions = file_security::capture(&fixture_control).unwrap();
        assert!(old_permissions.read_only);
        drop(fixture_control);
        durable::write_windows_atomic(&target, b"old-native-bytes", &old_permissions).unwrap();
        let mut old_reader = shared_native_file(&target, GENERIC_READ).unwrap();
        assert_readonly_user_and_system_fixture(&old_reader, &sid);
        let old_identity = security::file_identity(&old_reader).unwrap();
        assert_eq!(
            file_security::capture(&old_reader).unwrap(),
            old_permissions
        );
        let new_permissions = file_security::private_defaults(read_only).unwrap();
        let replaced =
            durable::write_windows_atomic(&target, b"new-native-bytes", &new_permissions);
        if let Err(error) = &replaced {
            // Ex does not enable POSIX replacement. A native sharing rejection
            // must preserve the old object; releasing the witness then retries
            // the exact same public operation, without clearing read-only.
            assert!(matches!(error.raw_os_error(), Some(5 | 32)));
            let still_old = security::open_identity(&target, false).unwrap();
            assert_eq!(security::file_identity(&still_old).unwrap(), old_identity);
            assert_eq!(file_security::capture(&still_old).unwrap(), old_permissions);
            assert_eq!(std::fs::read(&target).unwrap(), b"old-native-bytes");
        }
        assert_eq!(held_bytes(&mut old_reader), b"old-native-bytes");
        assert_eq!(
            file_security::capture(&old_reader).unwrap(),
            old_permissions
        );
        drop(old_reader);
        if replaced.is_err() {
            durable::write_windows_atomic(&target, b"new-native-bytes", &new_permissions).unwrap();
        }
        let published = security::open_owner_only(&target, false).unwrap();
        assert_eq!(file_security::capture(&published).unwrap(), new_permissions);
        assert_ne!(security::file_identity(&published).unwrap(), old_identity);
        assert_ne!(new_permissions.descriptor, old_permissions.descriptor);
        assert_eq!(std::fs::read(&target).unwrap(), b"new-native-bytes");
        drop(published);
        if read_only {
            unseal_fixture(&shared_native_file(&target, FILE_WRITE_ATTRIBUTES).unwrap());
        }
    }

    let denied = directory.join("denied-write-attributes");
    let private_readonly = file_security::private_defaults(true).unwrap();
    durable::write_windows_atomic(&denied, b"protected-original", &private_readonly).unwrap();
    let fixture_control = shared_native_file(
        &denied,
        READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
    )
    .unwrap();
    // This object was explicitly user-owned during creation. Its retained
    // fixture control handle can restore the test DACL during teardown; that
    // held capability is never passed to the publication being tested.
    set_fixture_dacl(
        &fixture_control,
        &format!("O:{sid}D:P(D;;0x100;;;{sid})(A;;FA;;;{sid})"),
    );
    assert_eq!(
        shared_native_file(&denied, FILE_WRITE_ATTRIBUTES)
            .unwrap_err()
            .raw_os_error(),
        Some(ERROR_ACCESS_DENIED as i32),
        "the real token must lack target WRITE_ATTRIBUTES even on the admin runner"
    );
    let before = file_security::capture(&fixture_control).unwrap();
    let before_identity = security::file_identity(&fixture_control).unwrap();
    assert!(before.read_only);
    assert_eq!(
        durable::write_windows_atomic(
            &denied,
            b"unauthorized-replacement",
            &file_security::private_defaults(false).unwrap(),
        )
        .unwrap_err()
        .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    let unchanged = security::open_identity(&denied, false).unwrap();
    assert_eq!(
        security::file_identity(&unchanged).unwrap(),
        before_identity
    );
    assert_eq!(file_security::capture(&unchanged).unwrap(), before);
    assert_eq!(std::fs::read(&denied).unwrap(), b"protected-original");
    drop(unchanged);
    set_fixture_dacl(&fixture_control, &private_readonly.descriptor);
    unseal_fixture(&fixture_control);
    drop(fixture_control);
    remove_uncommitted_fixture_stages(&directory);
    drop(_parent);
    fixture.close().unwrap();
}
