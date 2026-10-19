//! Bounded startup recovery facts. Only the storage owner assigns an action.
use hiroute_platform::{durable, private_fs};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read},
    path::Path,
};

pub const STORAGE_RECOVERY_SCHEMA: &str = "hiroute.storage-startup-recovery/v1";
pub const STORAGE_RECOVERY_FILE: &str = "startup-recovery.json";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StorageRecoveryCause {
    MissingPackage,
    InvalidPackage,
    InsufficientSpace,
    AccessDenied,
    Locked,
    MissingKey,
    UnsupportedFormat,
    MigrationFailed,
    UnknownState,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StorageRecoveryAction {
    ReinstallOriginal,
    RetryCurrent,
    ResumeCurrent,
    RestoreCompleteGroup,
    PreserveAndDiagnose,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StorageRecovery {
    pub schema: String,
    pub cause: StorageRecoveryCause,
    pub action: StorageRecoveryAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_state_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_batch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_name: Option<String>,
}
impl StorageRecovery {
    pub fn new(cause: StorageRecoveryCause, action: StorageRecoveryAction) -> Self {
        Self {
            schema: STORAGE_RECOVERY_SCHEMA.into(),
            cause,
            action,
            original_state_digest: None,
            backup_batch: None,
            backup_name: None,
        }
    }
    pub fn read(storage: &Path) -> io::Result<Option<Self>> {
        let mut file = match private_fs::open_private_file(&storage.join(STORAGE_RECOVERY_FILE)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        private_fs::verify_single_link(&file)?;
        let mut bytes = Vec::new();
        file.by_ref().take(4097).read_to_end(&mut bytes)?;
        let record: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if bytes.len() > 4096 || record.schema != STORAGE_RECOVERY_SCHEMA {
            return Err(io::Error::other("STORAGE_RECOVERY_INVALID"));
        }
        // A cutover marker always revokes a previous program-only instruction.
        if record.action == StorageRecoveryAction::ReinstallOriginal
            && (storage_upgrade_started(storage)?
                || record.original_state_digest.as_deref()
                    != Some(original_state_digest(storage)?.as_str()))
        {
            return Err(io::Error::other("STORAGE_RECOVERY_STALE"));
        }
        Ok(Some(record))
    }
    pub fn bind_original_state(&mut self, storage: &Path) -> io::Result<()> {
        self.original_state_digest = Some(original_state_digest(storage)?);
        Ok(())
    }
    pub fn store(&self, storage: &Path) -> io::Result<()> {
        durable::write_private_atomic(
            &storage.join(STORAGE_RECOVERY_FILE),
            &serde_json::to_vec(self)?,
        )
    }
    pub fn clear(storage: &Path) -> io::Result<()> {
        let path = storage.join(STORAGE_RECOVERY_FILE);
        match private_fs::open_private_file(&path) {
            Ok(file) => {
                private_fs::verify_single_link(&file)?;
                drop(file);
                std::fs::remove_file(path)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
    pub fn operator_guidance(&self) -> &'static str {
        match (self.cause, self.action) {
            (_, StorageRecoveryAction::ResumeCurrent) => {
                "Keep all generations and reopen the current version to resume the identified original upgrade batch. Do not run an older program. If it cannot continue, preserve newer data separately and follow this batch's complete-group recovery guide."
            }
            (_, StorageRecoveryAction::RestoreCompleteGroup) => {
                "Stop HiRoute and preserve newer data with its matching program separately. Follow the verified original batch's RECOVERY.md or recovery guide to restore the entire old group and matching complete program. Never restore individual databases or mix generations."
            }
            (_, StorageRecoveryAction::ReinstallOriginal) => {
                "The original data group was retained. Stop HiRoute and reinstall the exact complete program used before this upgrade without deleting data. Desktop: open that version and use Settings > Updates. Headless: retain the old package, set HIROUTE_RECOVERY_PACKAGE for the actual daemon process, then retry the supported upgrade."
            }
            (StorageRecoveryCause::InsufficientSpace, _) => {
                "Keep all data and recovery material. Free unrelated space on the storage volume, then retry the current version."
            }
            (StorageRecoveryCause::MissingPackage | StorageRecoveryCause::InvalidPackage, _) => {
                "Keep all storage generations. Supply the verified complete matching old package through the supported installer or HIROUTE_RECOVERY_PACKAGE for the actual daemon process. Do not launch an old program against this data group. Follow the complete-group recovery guide if current-version recovery cannot continue."
            }
            (StorageRecoveryCause::AccessDenied | StorageRecoveryCause::Locked, _) => {
                "Close other owned HiRoute instances and resolve the reported file access or ownership problem. Preserve all data and retry the current version. Do not change permissions recursively or delete recovery material."
            }
            _ => {
                "Keep all storage generations, keys, packages and journals. Do not downgrade or delete data. Inspect startup diagnostics and the matching complete-group recovery guide; preserve newer data separately before restoration. Missing keys or source material may require support and cannot be repaired by repeated retries."
            }
        }
    }
}

// Core authority is small recovery material. Observation uses bounded native
// metadata only: never traverse or hash old history to display startup guidance.
fn original_state_digest(storage: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for name in [
        "live/control.db",
        "live/runtime.db",
        "live/secrets.db",
        "master-key",
        "observation-hmac-key",
    ] {
        digest.update(name.as_bytes());
        let mut file = private_fs::open_private_file(&storage.join(name))?;
        private_fs::verify_single_link(&file)?;
        let mut buffer = [0; 64 * 1024];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            digest.update(&buffer[..n]);
        }
    }
    for name in [
        "live/control.db-wal",
        "live/runtime.db-wal",
        "live/secrets.db-wal",
        "observation/activity.db",
        "observation/activity.db-wal",
    ] {
        digest.update(name.as_bytes());
        match private_fs::open_private_file(&storage.join(name)) {
            Ok(file) => {
                private_fs::verify_single_link(&file)?;
                let metadata = file.metadata()?;
                digest.update(metadata.len().to_le_bytes());
                digest.update(
                    metadata
                        .modified()?
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(io::Error::other)?
                        .as_nanos()
                        .to_le_bytes(),
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    digest.update(metadata.dev().to_le_bytes());
                    digest.update(metadata.ino().to_le_bytes());
                }
                #[cfg(windows)]
                {
                    let id = hiroute_platform::windows::security::file_identity(&file)?;
                    digest.update(format!("{id:?}").as_bytes());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && name.ends_with("-wal") => {
                digest.update([0])
            }
            Err(e) => return Err(e),
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Installation rollback is insufficient evidence of storage downgrade safety.
/// Refuse conservatively if any storage-upgrade authority is present or unreadable.
pub fn storage_upgrade_started(storage: &Path) -> io::Result<bool> {
    for name in [
        "observation-source-backup.json",
        "observation.upgrade.json",
        "observation.pre-v5-source",
        "observation.v5-target",
        "migration-set",
    ] {
        if storage.join(name).try_exists()? {
            return Ok(true);
        }
    }
    let Some(parent) = storage.parent() else {
        return Err(io::Error::other("STORAGE_ROOT_INVALID"));
    };
    let Some(name) = storage.file_name() else {
        return Err(io::Error::other("STORAGE_ROOT_INVALID"));
    };
    let mut backup = name.to_os_string();
    backup.push(".upgrade-backups");
    let backup = parent.join(backup);
    if backup.try_exists()? {
        let metadata = std::fs::symlink_metadata(&backup)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(std::io::Error::other("STORAGE_BACKUP_ROOT_INVALID"));
        }
        // Even unrecognized material prevents an original-compatible claim.
        if std::fs::read_dir(backup)?.next().is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_recovery_cannot_authorize_downgrade() {
        for field in ["cause", "action"] {
            let mut value = serde_json::to_value(StorageRecovery::new(
                StorageRecoveryCause::MissingPackage,
                StorageRecoveryAction::PreserveAndDiagnose,
            ))
            .unwrap();
            value[field] = serde_json::json!("future_value");
            assert!(serde_json::from_value::<StorageRecovery>(value).is_err());
        }
    }
    #[test]
    fn cutover_revokes_program_only_advice() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().join("storage");
        private_fs::prepare_storage_directory(&storage).unwrap();
        let mut report = StorageRecovery::new(
            StorageRecoveryCause::MissingPackage,
            StorageRecoveryAction::ReinstallOriginal,
        );
        private_fs::prepare_storage_directory(&storage.join("live")).unwrap();
        private_fs::prepare_storage_directory(&storage.join("observation")).unwrap();
        for name in [
            "live/control.db",
            "live/runtime.db",
            "live/secrets.db",
            "master-key",
            "observation-hmac-key",
            "observation/activity.db",
        ] {
            durable::write_private_new(&storage.join(name), b"unchanged original authority")
                .unwrap();
        }
        report.bind_original_state(&storage).unwrap();
        report.store(&storage).unwrap();
        assert_eq!(StorageRecovery::read(&storage).unwrap(), Some(report));
        durable::write_private_new(&storage.join("observation-source-backup.json"), b"{}").unwrap();
        assert!(StorageRecovery::read(&storage).is_err());
    }
}
