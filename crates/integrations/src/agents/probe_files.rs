//! Private files for admitted native checks; no borrowed user root is adopted.
#[cfg(not(windows))]
use std::fs;
use std::{fs::File, io, path::Path};

pub(super) fn temporary(prefix: &str) -> io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let root = builder.tempdir()?;
    #[cfg(windows)]
    hiroute_platform::windows::security::set_owner_only(root.path(), true)?;
    Ok(root)
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
