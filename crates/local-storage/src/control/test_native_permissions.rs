//! Test-only native permissions. Missing objects are created with an explicit
//! TokenUser owner; existing objects are verified without seizing ownership.
#[cfg(unix)]
use std::fs;

pub(crate) fn private_fixture_permissions(path: &std::path::Path, directory: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if directory { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            mode
        );
    }
    #[cfg(windows)]
    {
        use hiroute_platform::windows::security;
        if !path.exists() {
            let created = if directory {
                security::create_owner_only_directory(path).unwrap()
            } else {
                security::create_owner_only_file(path).unwrap()
            };
            security::verify_owner_only_handle(&created, directory).unwrap();
            drop(created);
        }
        let held = security::open_identity(path, directory).unwrap();
        security::verify_owner_only_handle(&held, directory).unwrap();
    }
}
