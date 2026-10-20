//! Early admission can fail before a recovery file can be safely written.
//! Diagnostic facts here can explain a cause, never authorize data operations.
use super::*;
use hiroute_diagnostics::{
    event::{DiagnosticEvent, ProcessRole, StartupFailureCode, StartupOutcome},
    files::{CURRENT_LOG_FILE, PrivateDir, role_dir_name},
    record::DiagnosticRecordV1,
};

impl StorageRecovery {
    pub fn from_admission_code(code: StartupFailureCode) -> Option<Self> {
        let cause = match code {
            StartupFailureCode::StorageAccessDenied => StorageRecoveryCause::AccessDenied,
            StartupFailureCode::StorageLocked => StorageRecoveryCause::Locked,
            StartupFailureCode::StorageKeyUnavailable => StorageRecoveryCause::MissingKey,
            StartupFailureCode::UpgradeSourceUnsupported => StorageRecoveryCause::UnsupportedFormat,
            _ => return None,
        };
        Some(Self::new(cause, StorageRecoveryAction::PreserveAndDiagnose))
    }

    /// Use only on an already failed startup surface. Prefer this attempt's
    /// diagnostic admission failure over a report left by an earlier writer.
    /// A log record never grants reinstall, resume or restore authority.
    pub fn read_for_failed_startup(storage: &Path, diagnostics: &Path) -> Option<Self> {
        if let Some(report) = admission_failure(diagnostics) {
            return Some(report);
        }
        match Self::read(storage) {
            Ok(report) => report,
            Err(error) => Some(Self::new(
                if error.kind() == io::ErrorKind::PermissionDenied {
                    StorageRecoveryCause::AccessDenied
                } else {
                    StorageRecoveryCause::UnknownState
                },
                StorageRecoveryAction::PreserveAndDiagnose,
            )),
        }
    }
}

fn admission_failure(root: &Path) -> Option<StorageRecovery> {
    let root = PrivateDir::open_existing(root).ok()?;
    root.recheck_identity().ok()?;
    let role =
        PrivateDir::open_existing(&root.path().join(role_dir_name(ProcessRole::Daemon))).ok()?;
    root.recheck_identity().ok()?;
    let mut file = role.open_read(CURRENT_LOG_FILE).ok()??;
    file.recheck().ok()?;
    let bytes = file.read_tail(64 * 1024).ok()?;
    file.recheck().ok()?;
    if !bytes.ends_with(b"\n") {
        return None;
    }
    // The newest startup/process event is the boundary. Unknown or unfinished
    // attempts cannot borrow an older admission reason from rotated history.
    for line in bytes
        .split(|byte| *byte == b'\n')
        .rev()
        .filter(|line| !line.is_empty())
    {
        let value: serde_json::Value = serde_json::from_slice(line).ok()?;
        let event = value.get("event")?;
        if event.get("process_start").is_some() {
            return None;
        }
        if event.get("startup_end").is_some() {
            let record: DiagnosticRecordV1 = serde_json::from_value(value).ok()?;
            if let DiagnosticEvent::StartupEnd(end) = record.event
                && let StartupOutcome::Failure { code } = end.outcome
            {
                return StorageRecovery::from_admission_code(code);
            }
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hiroute_diagnostics::{Component, DiagnosticRuntime, RuntimeConfig, event::StartupEnd};
    #[test]
    fn early_admission_remains_actionable_without_writing_or_trusting_storage() {
        for (code, cause) in [
            (
                StartupFailureCode::StorageLocked,
                StorageRecoveryCause::Locked,
            ),
            (
                StartupFailureCode::StorageAccessDenied,
                StorageRecoveryCause::AccessDenied,
            ),
            (
                StartupFailureCode::StorageKeyUnavailable,
                StorageRecoveryCause::MissingKey,
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let diagnostics = directory.path().join("diagnostics");
            let runtime = DiagnosticRuntime::start(RuntimeConfig {
                root: diagnostics.clone(),
                role: ProcessRole::Daemon,
                component: Component::Daemon,
                parent_session_id: None,
                level_override: None,
            });
            runtime.emit(DiagnosticEvent::StartupEnd(StartupEnd {
                outcome: StartupOutcome::Failure { code },
                elapsed_ms: 1,
            }));
            runtime.shutdown();
            let storage = directory.path().join("inaccessible-storage");
            let report = StorageRecovery::read_for_failed_startup(&storage, &diagnostics).unwrap();
            assert_eq!(report.cause, cause);
            assert_eq!(report.action, StorageRecoveryAction::PreserveAndDiagnose);
            assert_eq!(report.backup_batch, None);
            assert_eq!(report.original_state_digest, None);
            assert!(!storage.exists());
        }
    }
}
