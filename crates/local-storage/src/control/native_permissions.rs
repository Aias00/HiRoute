//! Windows artifact permission facts participate in ownership and encrypted
//! restoration. Unix keeps its released marker and fingerprint byte contracts.
use super::*;
use hiroute_platform::file_security::WindowsPermissions;

pub(super) const fn current_artifact_schema() -> &'static str {
    if cfg!(windows) {
        "hiroute.managed-artifact-marker/windows-v5"
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
    let mut file = match hiroute_platform::windows::security::open_identity(path, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if hiroute_platform::windows::security::file_link_count(&file)? != 1 {
        return Err(LocalStorageError::Permission);
    }
    let before = file.metadata()?;
    let permissions = hiroute_platform::file_security::capture(&file)?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if before.len() != after.len()
        || before.modified()? != after.modified()?
        || hiroute_platform::file_security::capture(&file)? != permissions
    {
        return Err(LocalStorageError::InvalidData);
    }
    let mode = if permissions.read_only { 0o400 } else { 0o600 };
    let fingerprint = windows_artifact_fingerprint(&bytes, &permissions)?;
    Ok(Some(ArtifactSnapshot {
        bytes,
        mode,
        fingerprint,
        windows_permissions: Some(permissions),
    }))
}
