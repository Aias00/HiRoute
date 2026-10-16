//! Fixture admission and disposition; never an owner-state deletion authority.
use std::io;
use std::path::Path;

#[derive(Debug, Eq, PartialEq)]
pub struct StateDisposition {
    pub removed: bool,
    /// The fixture leaves state untouched; this is not a state-absence proof.
    pub preserved: bool,
}

pub fn require_absent(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "background fixture requires an absent object",
        )),
    }
}

/// The marker must be absent, but no state path is accepted or inspected. A
/// successful uninstall deliberately preserves user data, including additions,
/// replaced objects, inaccessible objects and reparse points unknown to this run.
pub fn after_uninstall(marker: &Path) -> io::Result<StateDisposition> {
    require_absent(marker)?;
    Ok(StateDisposition {
        removed: false,
        preserved: true,
    })
}

#[cfg(test)]
mod tests {
    use super::{after_uninstall, require_absent};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    // Only newly created test paths belong to this guard. This is unrelated to
    // the product's KnownFolder/profile and never adopts a user state directory.
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let name = format!(
                "hiroute-state-disposition-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(name);
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn directory(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            #[cfg(windows)]
            drop(hiroute_platform::windows::security::create_owner_only_directory(&path).unwrap());
            #[cfg(not(windows))]
            fs::create_dir(&path).unwrap();
            path
        }
        fn file(path: &Path, bytes: &[u8]) {
            #[cfg(windows)]
            hiroute_platform::durable::write_private_new(path, bytes).unwrap();
            #[cfg(not(windows))]
            fs::write(path, bytes).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn post_uninstall_disposition_preserves_existing_state_and_unknown_additions() {
        let fixture = Fixture::new();
        let state = fixture.directory("state");
        let data = state.join("existing-user-data");
        let addition = state.join("unknown-addition");
        Fixture::file(&data, b"existing-user-data");
        Fixture::file(&addition, b"unknown-addition");
        let marker = fixture.0.join("absent-marker");
        // Existing state is rejected by the Lua-only fresh-state admission.
        assert_eq!(
            require_absent(&state).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        let disposition = after_uninstall(&marker).unwrap();
        assert!(disposition.preserved && !disposition.removed);
        assert_eq!(fs::read(data).unwrap(), b"existing-user-data");
        assert_eq!(fs::read(addition).unwrap(), b"unknown-addition");
        assert!(state.is_dir(), "preserved does not mean state was removed");
    }

    #[test]
    fn post_uninstall_disposition_refuses_present_marker_without_touching_state() {
        let fixture = Fixture::new();
        let state = fixture.directory("state");
        let data = state.join("user-data");
        Fixture::file(&data, b"user-data");
        let marker = fixture.0.join("marker");
        Fixture::file(&marker, b"installed-marker");
        assert_eq!(
            after_uninstall(&marker).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&data).unwrap(), b"user-data");
        fs::remove_file(&marker).unwrap();
        fs::create_dir(&marker).unwrap();
        assert!(after_uninstall(&marker).is_err());
        fs::remove_dir(&marker).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(fixture.0.join("missing-target"), &marker).unwrap();
            assert!(!marker.exists());
            assert!(after_uninstall(&marker).is_err());
        }
        #[cfg(windows)]
        {
            let target = fixture.directory("marker-target");
            hiroute_platform_test_support::junction::create(&marker, &target).unwrap();
            assert!(after_uninstall(&marker).is_err());
            fs::remove_dir(&marker).unwrap();
        }
        assert_eq!(fs::read(data).unwrap(), b"user-data");
    }

    #[test]
    fn former_recursive_state_cleanup_fails_the_preservation_oracle() {
        let fixture = Fixture::new();
        let state = fixture.directory("state");
        let data = state.join("preexisting-user-data");
        Fixture::file(&data, b"preexisting-user-data");
        // The previous cleanup-state action recursively deleted this directory.
        // This controlled negative uses only our fresh fixture and demonstrates
        // why absence/removal can never satisfy the user-data preservation oracle.
        fs::remove_dir_all(&state).unwrap();
        assert!(fs::read(data).is_err());
        assert!(!state.exists());
    }

    #[cfg(windows)]
    #[test]
    fn native_post_uninstall_preserves_file_ids_permissions_links_and_reparse_state() {
        use hiroute_platform::file_security::{WindowsPermissions, capture};
        use hiroute_platform::windows::security;
        use hiroute_platform_test_support::{grant_everyone_read, junction};
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

        fn facts(path: &Path, directory: bool) -> ((u32, u64), WindowsPermissions, u32) {
            // Fresh test-owned objects only. OPEN_REPARSE_POINT inspects the alias
            // itself; no reads ever pass through it to the separate target.
            let held = fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x0020_0000 | if directory { 0x0200_0000 } else { 0 })
                .open(path)
                .unwrap();
            (
                security::file_identity(&held).unwrap(),
                capture(&held).unwrap(),
                held.metadata().unwrap().file_attributes(),
            )
        }

        let fixture = Fixture::new();
        let state = fixture.directory("state");
        let target = fixture.directory("unrelated-target");
        let data = state.join("unknown-nonprivate-data");
        Fixture::file(&data, b"user-data-with-foreign-reader");
        grant_everyone_read(&data, false).unwrap();
        let extra_link = state.join("unknown-hardlink");
        fs::hard_link(&data, &extra_link).unwrap();
        let target_data = target.join("target-nonce");
        Fixture::file(&target_data, b"unrelated-target-nonce");
        let alias = fixture.0.join("unknown-state-alias");
        junction::create(&alias, &target).unwrap();

        let before = [
            facts(&state, true),
            facts(&data, false),
            facts(&extra_link, false),
            facts(&alias, true),
            facts(&target, true),
            facts(&target_data, false),
        ];
        assert_eq!(before[1].0, before[2].0);
        assert_ne!(before[3].2 & 0x400, 0);
        assert!(security::open_owner_only(&data, false).is_err());
        assert!(require_absent(&alias).is_err());
        let disposition = after_uninstall(&fixture.0.join("absent-marker")).unwrap();
        assert!(disposition.preserved && !disposition.removed);
        let after = [
            facts(&state, true),
            facts(&data, false),
            facts(&extra_link, false),
            facts(&alias, true),
            facts(&target, true),
            facts(&target_data, false),
        ];
        assert!(
            before == after,
            "unknown native objects must remain unchanged"
        );
        assert_eq!(fs::read(&data).unwrap(), b"user-data-with-foreign-reader");
        assert_eq!(fs::read(&target_data).unwrap(), b"unrelated-target-nonce");
        // Dispose only the exact newly created fixture alias, never an alias
        // encountered in a real owner profile or an existing state directory.
        fs::remove_dir(&alias).unwrap();
        assert_eq!(fs::read(target_data).unwrap(), b"unrelated-target-nonce");
    }
}
