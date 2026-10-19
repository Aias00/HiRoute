//! Recovery disposition is decided while the startup writer lock is held.
use super::*;
use hiroute_host_runtime::StorageRecoveryAction as Action;

pub(crate) fn publish_failure(root: &Path, error: LocalStorageError) -> LocalStorageError {
    let mut recovery = error.recovery();
    // Missing package and matching SQL markers cannot establish that a complete
    // original group is readable by the exact old program. In particular,
    // current Gateway payloads can be incompatible without a SQL version change.
    // No unproven program-only downgrade is authorized here.
    if recovery.action == Action::PreserveAndDiagnose {
        if let Ok((name, id, phase)) = verified_original_source(root) {
            recovery.backup_name = Some(name);
            recovery.backup_batch = Some(id);
            recovery.action =
                if matches!(phase, BackupSetPhase::Restored | BackupSetPhase::Restoring) {
                    Action::RestoreCompleteGroup
                } else {
                    Action::ResumeCurrent
                };
        }
    }
    let _ = recovery.store(root);
    match error {
        LocalStorageError::Recovery { source, .. } => {
            LocalStorageError::Recovery { recovery, source }
        }
        error => error,
    }
}

fn verified_original_source(
    root: &Path,
) -> Result<(String, String, BackupSetPhase), LocalStorageError> {
    let link = root.join("observation-source-backup.json");
    validate_owner_file(&link)?;
    if fs::metadata(&link)?.len() > 16 * 1024 {
        return Err(LocalStorageError::InvalidData);
    }
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(link)?).map_err(|_| LocalStorageError::InvalidData)?;
    let name = record["backup_name"]
        .as_str()
        .ok_or(LocalStorageError::InvalidData)?;
    let id = record["set_id"]
        .as_str()
        .ok_or(LocalStorageError::InvalidData)?;
    let portable = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && !s.contains("..")
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    if record["schema"] != 1 || !portable(name) || !portable(id) {
        return Err(LocalStorageError::InvalidData);
    }
    let parent = crate::upgrade_backup_root(root)?
        .parent()
        .ok_or(LocalStorageError::InvalidData)?
        .to_path_buf();
    for name in [name.to_owned(), format!("migration-set.{id}")] {
        let path = parent.join(&name);
        if !path.exists() {
            continue;
        }
        let set = BackupSet::open(&crate::daemon_storage_authority(), &path)?;
        if set.set_id() != id {
            continue;
        }
        crate::backup::source_snapshot::verify_sealed(&path)?;
        return Ok((name, id.to_owned(), set.phase()));
    }
    Err(LocalStorageError::InvalidData)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hiroute_host_runtime::{StorageRecovery, StorageRecoveryCause as Cause};
    #[test]
    fn missing_package_never_infers_original_program_compatibility_from_sql_markers() {
        for prior_upgrade in [false, true] {
            let directory = crate::test_tempdir().unwrap();
            let root = directory.path().join("storage");
            drop(crate::LocalStorageSet::open_for_daemon_startup(&root).unwrap());
            hiroute_platform::durable::write_private_new(
                &root.join("observation-hmac-key"),
                &[7; 32],
            )
            .unwrap();
            crate::prepare_owner_directory(&root.join("observation")).unwrap();
            let path = root.join("observation/activity.db");
            let db = Connection::open(&path).unwrap();
            db.execute_batch("CREATE TABLE observation_meta(key TEXT PRIMARY KEY,value TEXT); INSERT INTO observation_meta VALUES('schema_version','4'); CREATE TABLE original_content(id INTEGER PRIMARY KEY,body BLOB); INSERT INTO original_content VALUES(1,X'00ff');").unwrap();
            drop(db);
            super::super::set_owner_file_permissions(&path).unwrap();
            let paths = [
                "live/control.db",
                "live/runtime.db",
                "live/secrets.db",
                "master-key",
                "observation-hmac-key",
                "observation/activity.db",
            ];
            let original: Vec<_> = paths
                .iter()
                .map(|p| fs::read(root.join(p)).unwrap())
                .collect();
            if prior_upgrade {
                fs::write(
                    root.join("observation.upgrade.json"),
                    b"unknown prior phase",
                )
                .unwrap();
            }
            assert!(
                crate::LocalStorageSet::open_for_daemon_startup_with_options(
                    &root,
                    &crate::StorageStartupOptions {
                        preserve_observation_source: true,
                        ..Default::default()
                    }
                )
                .is_err()
            );
            let recovery = StorageRecovery::read(&root).unwrap().unwrap();
            assert_eq!(recovery.cause, Cause::MissingPackage);
            assert_eq!(recovery.action, Action::PreserveAndDiagnose);
            for (path, before) in paths.iter().zip(original) {
                assert_eq!(fs::read(root.join(path)).unwrap(), before, "changed {path}");
            }
        }
    }
}
