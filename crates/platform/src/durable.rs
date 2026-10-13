//! Durable file transitions used inside the existing operation/backup journals.
//! Windows uses write-through handles and handle-relative rename; directory
//! barriers validate identity there, rather than attempting unsupported directory
//! FlushFileBuffers. Callers retain writing/committed/final recovery checkpoints.

#[cfg(any(not(windows), test))]
use std::fs;
#[cfg(not(windows))]
use std::fs::{File, OpenOptions};
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
    {
        File::open(path)?.sync_all()
    }
}

pub fn rename(source: impl AsRef<Path>, target: impl AsRef<Path>) -> io::Result<()> {
    rename_inner(source.as_ref(), target.as_ref(), true)
}

fn rename_inner(source: &Path, target: &Path, replace: bool) -> io::Result<()> {
    #[cfg(windows)]
    {
        crate::windows::durable::rename(source, target, replace)
    }
    #[cfg(not(windows))]
    {
        if replace {
            fs::rename(source, target)?;
        } else {
            // Atomic no-clobber publication for complete initial key files.
            fs::hard_link(source, target)?;
            directory_barrier(
                target
                    .parent()
                    .ok_or_else(|| io::Error::other("missing parent"))?,
            )?;
            fs::remove_file(source)?;
        }
        directory_barrier(
            target
                .parent()
                .ok_or_else(|| io::Error::other("missing parent"))?,
        )
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
pub fn write_windows_atomic(
    path: &Path,
    bytes: &[u8],
    permissions: &crate::file_security::WindowsPermissions,
) -> io::Result<()> {
    write_with_permissions(path, bytes, true, Some(permissions))
}

/// Stream a stopped native snapshot into an unpublished stage. The producer
/// must validate its held source before returning success. An error cannot
/// replace the target; flushing and native permission verification precede rename.
#[cfg(windows)]
pub fn write_windows_atomic_with(
    path: &Path,
    permissions: &crate::file_security::WindowsPermissions,
    write: impl FnOnce(&mut std::fs::File) -> io::Result<()>,
) -> io::Result<()> {
    publish_with_permissions(path, true, Some(permissions), write)
}

/// Move an already flushed native artifact without reopening it for data writes.
/// Its authenticated publication owner has admitted the source/target facts.
/// The kernel checks target WRITE_ATTRIBUTES before replacing a read-only file;
/// target attributes are never cleared through a separately resolved pathname.
#[cfg(windows)]
pub fn rename_windows_sealed_file(source: &Path, target: &Path) -> io::Result<()> {
    let file = crate::windows::security::open_delete(source, false)?;
    crate::windows::durable::rename_native_file(&file, target)
}

fn write_with_permissions(
    path: &Path,
    bytes: &[u8],
    replace: bool,
    permissions: Option<&crate::file_security::WindowsPermissions>,
) -> io::Result<()> {
    publish_with_permissions(path, replace, permissions, |file| file.write_all(bytes))
}

fn publish_with_permissions(
    path: &Path,
    replace: bool,
    permissions: Option<&crate::file_security::WindowsPermissions>,
    write: impl FnOnce(&mut std::fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
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
    write(&mut file)?;
    file.sync_all()?;
    #[cfg(windows)]
    {
        if let Some(permissions) = permissions {
            crate::windows::file_security::apply_read_only(&file, permissions.read_only)?;
            if crate::file_security::capture(&file)? != *permissions {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "native permissions differ",
                ));
            }
        }
        let renamed = if permissions.is_some() && replace {
            crate::windows::durable::rename_native_file(&file, path)
        } else {
            crate::windows::durable::rename_file(&file, path, replace)
        };
        if let Err(error) = renamed {
            if error.kind() == io::ErrorKind::AlreadyExists {
                // Definite no-clobber failure: remove this exact uncommitted
                // stage through its retained handle, never a replacement path.
                crate::windows::security::delete_by_handle(&file)?;
            }
            return Err(error);
        }
        file.sync_all()?;
    }
    #[cfg(not(windows))]
    {
        let _ = permissions;
        if let Err(error) = rename_inner(&temporary, path, replace) {
            if error.kind() == io::ErrorKind::AlreadyExists {
                use std::os::unix::fs::MetadataExt;
                let held = file.metadata()?;
                let named = fs::symlink_metadata(&temporary)?;
                if held.dev() == named.dev() && held.ino() == named.ino() {
                    fs::remove_file(&temporary)?;
                    directory_barrier(parent)?;
                }
            }
            return Err(error);
        }
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
        assert_eq!(
            fs::read_dir(fixture.path()).unwrap().count(),
            1,
            "definite no-clobber conflict must remove only its own unpublished stage"
        );
        write_private_atomic(&key, b"complete-replacement").unwrap();
        assert_eq!(fs::read(&key).unwrap(), b"complete-replacement");
    }
}
