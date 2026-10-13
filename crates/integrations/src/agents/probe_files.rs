//! Private files for admitted native checks; no borrowed user root is adopted.
#[cfg(not(windows))]
use std::fs;
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

pub(super) struct PrivateProbeRoot {
    cleanup: tempfile::TempDir,
    path: PathBuf,
}

impl PrivateProbeRoot {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn keep(self) -> PathBuf {
        self.cleanup.keep()
    }

    fn from_cleanup(cleanup: tempfile::TempDir) -> io::Result<Self> {
        #[cfg(windows)]
        let path = {
            let path = cleanup.path().join("private");
            hiroute_platform::windows::security::create_owner_only_directory(&path)?;
            path
        };
        #[cfg(not(windows))]
        let path = cleanup.path().to_owned();
        Ok(Self { cleanup, path })
    }
}

pub(super) fn temporary(prefix: &str) -> io::Result<PrivateProbeRoot> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    PrivateProbeRoot::from_cleanup(builder.tempdir()?)
}

pub(super) fn directory(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        hiroute_platform::windows::security::create_owner_only_directory(path).map(drop)
    }
    #[cfg(not(windows))]
    {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)
    }
}

pub(super) fn file(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        hiroute_platform::windows::security::create_owner_only_file(path)
    }
    #[cfg(not(windows))]
    {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path)
    }
}

pub(super) fn executable(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use hiroute_platform::{file_security, windows::security};

    #[test]
    fn windows_probe_files_attach_private_user_owner_without_adopting_outer_owner() {
        let cleanup = tempfile::tempdir().unwrap();
        let outer = security::open_identity(cleanup.path(), true).unwrap();
        let before = file_security::capture(&outer).unwrap();
        let root = PrivateProbeRoot::from_cleanup(cleanup).unwrap();
        let private = security::open_owner_only(root.path(), true).unwrap();
        security::verify_owner_only_handle(&private, true).unwrap();
        let child = file(&root.path().join("capture")).unwrap();
        security::verify_owner_only_handle(&child, false).unwrap();
        assert_eq!(
            file_security::capture(&outer).unwrap(),
            before,
            "creating private materials must preserve the cleanup container's owner and ACL"
        );
        assert_ne!(root.path(), root.cleanup.path());
    }
}
