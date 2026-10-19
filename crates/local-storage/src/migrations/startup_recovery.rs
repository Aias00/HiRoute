//! Recovery disposition is decided while the startup writer lock is held.
use super::*;
use hiroute_host_runtime::{StorageRecoveryAction as Action, StorageRecoveryCause as Cause};

/// A deliberately narrow no-cutover case. Combined core upgrades, prior batches,
/// missing authorities and unknown Observation formats never authorize old-program use.
pub(crate) fn original_group_admitted(root: &Path) -> Result<bool, LocalStorageError> {
    if hiroute_host_runtime::storage_upgrade_started(root)? {
        return Ok(false);
    }
    for path in live_database_paths(&root.join("live")) {
        if database_version(&path)? != Some(LATEST_SCHEMA_VERSION) {
            return Ok(false);
        }
        validate_owner_file(&path)?;
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let integrity: String = db.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Ok(false);
        }
    }
    if crate::LocalSecretStore::migration_binding(
        &root.join("live/secrets.db"),
        &root.join("master-key"),
    )?
    .is_none()
    {
        return Ok(false);
    }
    let key = root.join("observation-hmac-key");
    validate_owner_file(&key)?;
    if fs::metadata(key)?.len() != 32 {
        return Ok(false);
    }
    let path = root.join("observation/activity.db");
    validate_owner_file(&path)?;
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let version: String = db.query_row(
        "SELECT value FROM observation_meta WHERE key='schema_version'",
        [],
        |row| row.get(0),
    )?;
    Ok(matches!(version.as_str(), "1" | "2" | "3" | "4"))
}

pub(crate) fn publish_failure(
    root: &Path,
    error: LocalStorageError,
    original: bool,
) -> LocalStorageError {
    let mut recovery = error.recovery();
    // The missing-package branch precedes source publication and core writes.
    // No other error, even low space after a prepared batch, borrows this proof.
    if original && recovery.cause == Cause::MissingPackage {
        recovery.action = Action::ReinstallOriginal;
        if recovery.bind_original_state(root).is_err() {
            recovery.action = Action::PreserveAndDiagnose;
            recovery.original_state_digest = None;
        }
    }
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
    use hiroute_host_runtime::StorageRecovery;
    #[test]
    fn missing_package_proves_the_complete_original_group_and_rejects_prior_upgrade_state() {
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
            assert_eq!(
                recovery.action,
                if prior_upgrade {
                    Action::PreserveAndDiagnose
                } else {
                    Action::ReinstallOriginal
                }
            );
            for (path, before) in paths.iter().zip(original) {
                assert_eq!(fs::read(root.join(path)).unwrap(), before, "changed {path}");
            }
        }
    }
}
