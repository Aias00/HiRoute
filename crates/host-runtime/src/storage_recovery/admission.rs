//! Early admission can fail before a recovery file can be safely written.
//! Diagnostic facts here can explain a cause, never authorize data operations.
use super::*;
use hiroute_diagnostics::{
    event::{DiagnosticEvent, ProcessRole, StartupFailureCode, StartupOutcome},
    files::{CURRENT_LOG_FILE, PrivateDir, role_dir_name},
    record::DiagnosticRecordV1,
};

impl StorageRecovery {
    pub fn read_failed_startup_code(diagnostics: &Path) -> Option<&'static str> {
        failed_startup_code(diagnostics).map(startup_failure_host_code)
    }
    pub fn from_admission_code(code: StartupFailureCode) -> Option<Self> {
        let cause = match code {
            StartupFailureCode::StorageUnavailable => StorageRecoveryCause::UnknownState,
            StartupFailureCode::StorageAccessDenied => StorageRecoveryCause::AccessDenied,
            StartupFailureCode::StorageLocked => StorageRecoveryCause::Locked,
            StartupFailureCode::StorageKeyUnavailable => StorageRecoveryCause::MissingKey,
            StartupFailureCode::UpgradeSourceUnsupported => StorageRecoveryCause::UnsupportedFormat,
            _ => return None,
        };
        let action = if matches!(
            cause,
            StorageRecoveryCause::Locked | StorageRecoveryCause::AccessDenied
        ) {
            StorageRecoveryAction::RetryCurrent
        } else {
            StorageRecoveryAction::PreserveAndDiagnose
        };
        Some(Self::new(cause, action))
    }

    pub fn read_for_admission_failure(
        code: StartupFailureCode,
        storage: Option<&Path>,
    ) -> Option<Self> {
        Some(Self::from_admission_code(code)?.with_verified_restore(storage))
    }

    fn with_verified_restore(self, storage: Option<&Path>) -> Self {
        // The current diagnostic supplies only the cause. Whole-group restore
        // authority still comes from revalidating the storage owner's exact
        // original batch. Never borrow program-only reinstall or resume advice.
        if matches!(
            self.cause,
            StorageRecoveryCause::MissingKey
                | StorageRecoveryCause::UnsupportedFormat
                | StorageRecoveryCause::UnknownState
        ) && let Some(storage) = storage
            && let Ok(Some(record)) = Self::read(storage)
            && record.cause == self.cause
            && record.action == StorageRecoveryAction::RestoreCompleteGroup
        {
            return record;
        }
        self
    }

    /// Use only on an already failed startup surface. Prefer this attempt's
    /// diagnostic admission failure over a report left by an earlier writer.
    /// A log record never grants reinstall, resume or restore authority.
    pub fn read_for_failed_startup(storage: &Path, diagnostics: &Path) -> Option<Self> {
        if let Some(code) = failed_startup_code(diagnostics) {
            if let Some(report) = Self::from_admission_code(code) {
                return Some(report.with_verified_restore(Some(storage)));
            }
            if code != StartupFailureCode::UpgradeStorageFailed {
                return None;
            }
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

fn failed_startup_code(root: &Path) -> Option<StartupFailureCode> {
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
                return Some(code);
            }
            return None;
        }
    }
    None
}

pub fn startup_failure_host_code(code: StartupFailureCode) -> &'static str {
    match code {
        StartupFailureCode::StorageUnavailable => "DAEMON_STORAGE_UNREADABLE",
        StartupFailureCode::StorageAccessDenied => "DAEMON_STORAGE_ACCESS_DENIED",
        StartupFailureCode::StorageLocked => "DAEMON_STORAGE_LOCKED",
        StartupFailureCode::StorageKeyUnavailable => "DAEMON_STORAGE_KEY_UNAVAILABLE",
        StartupFailureCode::UpgradeSourceUnsupported => "DAEMON_UPGRADE_SOURCE_UNSUPPORTED",
        StartupFailureCode::UpgradeStorageFailed => "DAEMON_UPGRADE_STORAGE_FAILED",
        StartupFailureCode::ReleaseFactsInvalid => "DAEMON_RELEASE_INVALID",
        StartupFailureCode::DependencyUnavailable => "DAEMON_DEPENDENCY_UNAVAILABLE",
        StartupFailureCode::GatewayUnavailable => "DAEMON_GATEWAY_UNAVAILABLE",
        StartupFailureCode::ControlUnavailable => "DAEMON_CONTROL_UNAVAILABLE",
        StartupFailureCode::Cancelled => "DAEMON_START_CANCELLED",
        StartupFailureCode::InvalidConfiguration
        | StartupFailureCode::WorkerRejected
        | StartupFailureCode::ReadyChannelFailed
        | StartupFailureCode::Internal => "DAEMON_START_FAILED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hiroute_diagnostics::{Component, DiagnosticRuntime, RuntimeConfig, event::StartupEnd};
    #[test]
    fn current_non_storage_and_unknown_failures_cannot_borrow_an_old_storage_report() {
        for code in [
            StartupFailureCode::DependencyUnavailable,
            StartupFailureCode::GatewayUnavailable,
            StartupFailureCode::InvalidConfiguration,
            StartupFailureCode::StorageUnavailable,
            StartupFailureCode::StorageLocked,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let storage = directory.path();
            let previous = StorageRecovery::new(
                StorageRecoveryCause::MissingPackage,
                StorageRecoveryAction::PreserveAndDiagnose,
            );
            previous.store(storage).unwrap();
            assert_eq!(
                StorageRecovery::read(storage).unwrap(),
                Some(previous.clone())
            );
            let diagnostics = storage.join("diagnostics");
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
            let report = StorageRecovery::read_for_failed_startup(storage, &diagnostics);
            match code {
                StartupFailureCode::StorageUnavailable => assert_eq!(
                    report,
                    Some(StorageRecovery::new(
                        StorageRecoveryCause::UnknownState,
                        StorageRecoveryAction::PreserveAndDiagnose,
                    ))
                ),
                StartupFailureCode::StorageLocked => assert_eq!(
                    report,
                    Some(StorageRecovery::new(
                        StorageRecoveryCause::Locked,
                        StorageRecoveryAction::RetryCurrent,
                    ))
                ),
                _ => assert!(report.is_none()),
            }
            assert_eq!(StorageRecovery::read(storage).unwrap(), Some(previous));
        }
    }

    #[test]
    fn latest_failed_startup_code_preserves_non_storage_causes_and_stops_at_a_healthy_boot() {
        for (code, expected) in [
            (
                StartupFailureCode::DependencyUnavailable,
                "DAEMON_DEPENDENCY_UNAVAILABLE",
            ),
            (
                StartupFailureCode::GatewayUnavailable,
                "DAEMON_GATEWAY_UNAVAILABLE",
            ),
            (StartupFailureCode::StorageLocked, "DAEMON_STORAGE_LOCKED"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("diagnostics");
            let config = || RuntimeConfig {
                root: root.clone(),
                role: ProcessRole::Daemon,
                component: Component::Daemon,
                parent_session_id: None,
                level_override: None,
            };
            let runtime = DiagnosticRuntime::start(config());
            runtime.emit(DiagnosticEvent::StartupEnd(StartupEnd {
                outcome: StartupOutcome::Failure { code },
                elapsed_ms: 1,
            }));
            runtime.shutdown();
            assert_eq!(
                StorageRecovery::read_failed_startup_code(&root),
                Some(expected)
            );
            let runtime = DiagnosticRuntime::start(config());
            runtime.emit(DiagnosticEvent::StartupEnd(StartupEnd {
                outcome: StartupOutcome::Success,
                elapsed_ms: 1,
            }));
            runtime.shutdown();
            assert_eq!(StorageRecovery::read_failed_startup_code(&root), None);
        }
    }
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
            assert_eq!(
                report.action,
                if matches!(
                    cause,
                    StorageRecoveryCause::Locked | StorageRecoveryCause::AccessDenied
                ) {
                    StorageRecoveryAction::RetryCurrent
                } else {
                    StorageRecoveryAction::PreserveAndDiagnose
                }
            );
            assert_eq!(report.backup_batch, None);
            assert_eq!(report.original_state_digest, None);
            assert!(!storage.exists());
        }
    }
}
