//! Windows artifact permission facts participate in ownership and encrypted
//! restoration. Unix keeps its released marker and fingerprint byte contracts.
use super::*;
use hiroute_platform::file_security::WindowsPermissions;

pub(super) const fn current_artifact_schema() -> &'static str {
    if cfg!(windows) {
        "hiroute.windows-managed-artifact-marker/v5"
    } else {
        "hiroute.managed-artifact-marker/v4"
    }
}

pub(super) const fn current_artifact_aad() -> ArtifactBackupAad {
    if cfg!(windows) {
        ArtifactBackupAad::CurrentWindowsV5
    } else {
        ArtifactBackupAad::CurrentV4
    }
}

pub(super) fn desired_windows_permissions(
    mode: u32,
) -> Result<Option<WindowsPermissions>, LocalStorageError> {
    #[cfg(windows)]
    {
        Ok(Some(hiroute_platform::file_security::private_defaults(
            mode & 0o200 == 0,
        )?))
    }
    #[cfg(not(windows))]
    {
        let _ = mode;
        Ok(None)
    }
}

#[cfg(windows)]
pub(super) fn windows_artifact_fingerprint(
    bytes: &[u8],
    permissions: &WindowsPermissions,
) -> Result<CanonicalDigest, LocalStorageError> {
    CanonicalDigest::of(&json!({
        "schema": "hiroute.windows-artifact-fingerprint/v1",
        "content": CanonicalDigest::of_bytes(bytes),
        "permissions": permissions,
    }))
    .map_err(|_| LocalStorageError::InvalidData)
}

#[cfg(windows)]
pub(super) fn read_windows_artifact(
    path: &Path,
) -> Result<Option<ArtifactSnapshot>, LocalStorageError> {
    // The existing generic artifact contract has no fixed payload ceiling.
    // Freeze the opened length, then read at most that length plus one; native
    // configuration callers additionally provide their existing explicit cap.
    read_windows_snapshot(path, None, false, || {})
}

#[cfg(windows)]
pub(super) fn read_windows_artifact_bounded(
    path: &Path,
    limit: usize,
) -> Result<Option<ArtifactSnapshot>, LocalStorageError> {
    read_windows_snapshot(path, Some(limit), false, || {})
}

#[cfg(windows)]
#[derive(Eq, PartialEq)]
struct WindowsArtifactFacts {
    identity: (u32, u64),
    length: u64,
    creation: u64,
    modified: u64,
    attributes: u32,
    permissions: WindowsPermissions,
}

#[cfg(windows)]
impl WindowsArtifactFacts {
    fn read(file: &std::fs::File) -> Result<Self, LocalStorageError> {
        use hiroute_platform::{file_security, windows::security};
        use std::os::windows::fs::MetadataExt;
        let metadata = file.metadata()?;
        if !metadata.is_file() || security::file_link_count(file)? != 1 {
            return Err(LocalStorageError::Permission);
        }
        Ok(Self {
            identity: security::file_identity(file)?,
            length: metadata.len(),
            creation: metadata.creation_time(),
            modified: metadata.last_write_time(),
            attributes: metadata.file_attributes(),
            permissions: file_security::capture(file)?,
        })
    }
}

#[cfg(windows)]
fn read_windows_snapshot(
    path: &Path,
    limit: Option<usize>,
    private: bool,
    after_open: impl FnOnce(),
) -> Result<Option<ArtifactSnapshot>, LocalStorageError> {
    use hiroute_platform::windows::security;
    let lease = match security::open_read_lease(path) {
        Ok(lease) => lease,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(LocalStorageError::Permission);
        }
        Err(error) => return Err(error.into()),
    };
    if private {
        security::verify_owner_only_handle(lease.file(), false)?;
    }
    let before = WindowsArtifactFacts::read(lease.file())?;
    let cap = limit.map_or(Ok(before.length), |limit| {
        u64::try_from(limit).map_err(|_| LocalStorageError::Permission)
    })?;
    if before.length > cap {
        return Err(LocalStorageError::Permission);
    }
    let read_limit = cap.checked_add(1).ok_or(LocalStorageError::Permission)?;
    after_open();
    let mut bytes = Zeroizing::new(Vec::new());
    lease.file().take(read_limit).read_to_end(&mut bytes)?;
    lease.verify()?;
    let reopened = lease.reopen_file()?;
    if private {
        security::verify_owner_only_handle(lease.file(), false)?;
        security::verify_owner_only_handle(&reopened, false)?;
    }
    if bytes.len() as u64 > cap
        || bytes.len() as u64 != before.length
        || WindowsArtifactFacts::read(lease.file())? != before
        || WindowsArtifactFacts::read(&reopened)? != before
    {
        return Err(LocalStorageError::InvalidData);
    }
    lease.verify()?;
    let permissions = before.permissions;
    let mode = if permissions.read_only { 0o400 } else { 0o600 };
    let fingerprint = windows_artifact_fingerprint(&bytes, &permissions)?;
    Ok(Some(ArtifactSnapshot {
        bytes,
        mode,
        fingerprint,
        windows_permissions: Some(permissions),
    }))
}

/// Restore metadata and encrypted bytes are HiRoute-private, unlike the external
/// Agent target. Read them through one verified native handle on Windows.
#[cfg(windows)]
pub(super) fn read_private_restore_file(path: &Path) -> Result<Vec<u8>, LocalStorageError> {
    let snapshot = read_windows_snapshot(path, None, true, || {})?.ok_or_else(|| {
        LocalStorageError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "private restore file unavailable",
        ))
    })?;
    Ok(snapshot.bytes.to_vec())
}

#[cfg(all(test, windows))]
pub(super) fn read_windows_artifact_after_open(
    path: &Path,
    limit: usize,
    after_open: impl FnOnce(),
) -> Result<Option<ArtifactSnapshot>, LocalStorageError> {
    read_windows_snapshot(path, Some(limit), false, after_open)
}
