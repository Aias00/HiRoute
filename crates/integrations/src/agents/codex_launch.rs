//! Capture the selected CLI and home before a settings Operation is accepted. This never
//! imports native configuration into a launch descriptor or executes a model request.
use super::{AgentFilesystemScanError, FilesystemAgentScannerV1};
use hiroute_domain::{CODEX_LAUNCH_SNAPSHOT_SCHEMA_V1, CanonicalDigest, CodexLaunchSnapshotV1};
use std::path::Path;

impl FilesystemAgentScannerV1 {
    pub fn codex_launch_snapshot(&self) -> Result<CodexLaunchSnapshotV1, AgentFilesystemScanError> {
        let target = self
            .codex_engine_target(hiroute_domain::AgentModelSurfaceV2::CodexCli)
            .ok_or(AgentFilesystemScanError::SourceUnavailable)?;
        let executable = super::executable::resolve(&target)
            .map_err(|_| AgentFilesystemScanError::SourceUnavailable)?
            .ok_or(AgentFilesystemScanError::SourceUnavailable)?;
        let root = super::canonical_codex_config_path(&self.codex_user_config_target());
        let home = root
            .parent()
            .ok_or(AgentFilesystemScanError::InvalidConfig)?;
        let value = CodexLaunchSnapshotV1 {
            schema: CODEX_LAUNCH_SNAPSHOT_SCHEMA_V1.into(),
            executable: executable
                .to_str()
                .ok_or(AgentFilesystemScanError::InvalidConfig)?
                .into(),
            codex_home: home
                .to_str()
                .ok_or(AgentFilesystemScanError::InvalidConfig)?
                .into(),
            profile_name: super::CODEX_MANAGED_PROFILE_NAME.into(),
            installation_identity: executable_identity(&executable)?,
        };
        value
            .validate()
            .map_err(|_| AgentFilesystemScanError::InvalidConfig)?;
        // Refuse a path substitution during the sample, rather than sealing unrelated bytes.
        if super::executable::resolve(&target).ok().flatten().as_ref() != Some(&executable)
            || executable_identity(&executable)? != value.installation_identity
        {
            return Err(AgentFilesystemScanError::SourceUnavailable);
        }
        Ok(value)
    }

    pub fn codex_launch_snapshot_is_current(
        &self,
        snapshot: &CodexLaunchSnapshotV1,
    ) -> Result<bool, AgentFilesystemScanError> {
        Ok(self.codex_launch_snapshot()? == *snapshot)
    }
}

/// Recheck the frozen executable immediately before the CLI starts it. This reads no
/// native configuration and does not resolve a replacement installation through PATH.
pub fn codex_launch_executable_is_current(snapshot: &CodexLaunchSnapshotV1) -> bool {
    snapshot.validate().is_ok()
        && std::fs::canonicalize(&snapshot.executable)
            .ok()
            .is_some_and(|path| path.to_str() == Some(snapshot.executable.as_str()))
        && executable_identity(Path::new(&snapshot.executable))
            .ok()
            .as_ref()
            == Some(&snapshot.installation_identity)
}

fn executable_identity(path: &Path) -> Result<CanonicalDigest, AgentFilesystemScanError> {
    let unavailable = |_| AgentFilesystemScanError::SourceUnavailable;
    let metadata = std::fs::metadata(path).map_err(unavailable)?;
    if !metadata.is_file() {
        return Err(AgentFilesystemScanError::SourceUnavailable);
    }
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        CanonicalDigest::of(&(
            path,
            metadata.dev(),
            metadata.ino(),
            metadata.uid(),
            metadata.gid(),
            metadata.mode(),
            metadata.len(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        ))
    };
    #[cfg(windows)]
    let identity = {
        use std::os::windows::fs::MetadataExt;
        let file =
            hiroute_platform::windows::security::open_identity(path, false).map_err(unavailable)?;
        let native_identity =
            hiroute_platform::windows::security::file_identity(&file).map_err(unavailable)?;
        CanonicalDigest::of(&(
            path,
            native_identity,
            metadata.file_attributes(),
            metadata.file_size(),
            metadata.creation_time(),
            metadata.last_write_time(),
        ))
    };
    #[cfg(any(unix, windows))]
    {
        identity.map_err(|_| AgentFilesystemScanError::InvalidConfig)
    }
    #[cfg(not(any(unix, windows)))]
    {
        Err(AgentFilesystemScanError::SourceUnavailable)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn frozen_executable_is_rechecked_without_following_a_replacement_link() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("codex");
        std::fs::write(&file, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        let file = std::fs::canonicalize(file).unwrap();
        let snapshot = CodexLaunchSnapshotV1 {
            schema: CODEX_LAUNCH_SNAPSHOT_SCHEMA_V1.into(),
            executable: file.to_str().unwrap().into(),
            codex_home: file.parent().unwrap().to_str().unwrap().into(),
            profile_name: "hiroute".into(),
            installation_identity: executable_identity(&file).unwrap(),
        };
        assert!(codex_launch_executable_is_current(&snapshot));
        let replacement = file.with_file_name("replacement");
        std::fs::write(&replacement, b"#!/bin/sh\nexit 1\n").unwrap();
        std::fs::rename(replacement, &file).unwrap();
        assert!(!codex_launch_executable_is_current(&snapshot));
        std::fs::remove_file(&file).unwrap();
        assert!(!codex_launch_executable_is_current(&snapshot));
    }
}
