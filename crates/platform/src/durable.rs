//! Durable file transitions used inside the existing operation/backup journals.
//! Windows uses write-through handles and handle-relative rename; directory
//! barriers validate identity there, rather than attempting unsupported directory
//! FlushFileBuffers. Callers retain writing/committed/final recovery checkpoints.

#[cfg(not(windows))]
use std::fs::{File, OpenOptions};
#[cfg(any(not(windows), test))]
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub fn sync_file(path: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(windows)]
    let file = crate::windows::security::open_write_through(path.as_ref(), false)?;
    #[cfg(not(windows))]
    let file = File::open(path)?;
    file.sync_all()
}

pub fn directory_barrier(path: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(windows)]
    {
        // Namespace transitions use the write-through handle-relative operation
        // below. There is no user-mode directory fsync equivalent on Windows.
        let _held = crate::windows::security::open_identity(path.as_ref(), true)?;
        Ok(())
    }
    #[cfg(not(windows))]
    { File::open(path)?.sync_all() }
}

pub fn rename(source: impl AsRef<Path>, target: impl AsRef<Path>) -> io::Result<()> {
    rename_inner(source.as_ref(), target.as_ref(), true)
}

fn rename_inner(source: &Path, target: &Path, replace: bool) -> io::Result<()> {
    #[cfg(windows)]
    { crate::windows::durable::rename(source, target, replace) }
    #[cfg(not(windows))]
    {
        if replace { fs::rename(source, target)?; }
        else {
            // Atomic no-clobber publication for complete initial key files.
            fs::hard_link(source, target)?;
            directory_barrier(target.parent().ok_or_else(|| io::Error::other("missing parent"))?)?;
            fs::remove_file(source)?;
        }
        directory_barrier(target.parent().ok_or_else(|| io::Error::other("missing parent"))?)
    }
}

pub fn write_private_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_private(path, bytes, false)
}

pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_private(path, bytes, true)
}

fn write_private(path: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    write_with_permissions(path, bytes, replace, None)
}

#[cfg(windows)]
pub fn write_windows_atomic(path: &Path, bytes: &[u8], permissions: &crate::file_security::WindowsPermissions) -> io::Result<()> {
    write_with_permissions(path, bytes, true, Some(permissions))
}

fn write_with_permissions(path: &Path, bytes: &[u8], replace: bool,
    permissions: Option<&crate::file_security::WindowsPermissions>) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("missing parent"))?;
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).map_err(|error| io::Error::other(error.to_string()))?;
    let suffix: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let temporary = parent.join(format!(".hiroute-write-{suffix}.tmp"));
    #[cfg(windows)]
    let mut file = match permissions {
        Some(permissions) => crate::windows::file_security::create(&temporary, permissions)?,
        None => crate::windows::security::create_owner_only_file(&temporary)?,
    };
    #[cfg(not(windows))]
    let mut file = {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&temporary)?
    };
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(windows)]
    {
        if let Some(permissions) = permissions {
            crate::windows::file_security::apply_read_only(&file, permissions.read_only)?;
            if crate::file_security::capture(&file)? != *permissions {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "native permissions differ"));
            }
        }
        crate::windows::durable::rename_file(&file, path, replace)?;
        file.sync_all()?;
    }
    #[cfg(not(windows))]
    {
        let _ = permissions;
        drop(file);
        rename_inner(&temporary, path, replace)?;
    }
    // A failed write leaves an uncommitted uniquely named private stage. Existing
    // journals never interpret that stage as a committed key or target.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_private_publication_never_replaces_a_committed_key() {
        let fixture = tempfile::tempdir().unwrap();
        let key = fixture.path().join("key");
        write_private_new(&key, b"complete-key-one").unwrap();
        assert!(write_private_new(&key, b"different-key").is_err());
        assert_eq!(fs::read(&key).unwrap(), b"complete-key-one");
        write_private_atomic(&key, b"complete-replacement").unwrap();
        assert_eq!(fs::read(&key).unwrap(), b"complete-replacement");
    }
}
