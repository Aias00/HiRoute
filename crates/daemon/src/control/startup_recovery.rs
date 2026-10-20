//! Terminal guidance belongs to the current failed attempt, never an older writer's report.
use hiroute_diagnostics::event::StartupFailureCode;
use hiroute_host_runtime::{StorageRecovery, StorageRecoveryAction, StorageRecoveryCause};
use std::path::Path;

pub(super) fn role_failure(error: hiroute_daemon::RoleAllError) -> super::StartupError {
    let code = super::classify_role_all_error(&error);
    let message = error.to_string();
    let recovery = match error {
        hiroute_daemon::RoleAllError::StorageRecovery(report) => Some(report),
        _ => None,
    };
    super::StartupError {
        code,
        message,
        recovery,
    }
}

pub(super) fn failure_report(
    code: StartupFailureCode,
    storage: Option<&Path>,
    current: Option<&StorageRecovery>,
) -> Option<StorageRecovery> {
    if let Some(current) = current {
        return Some(current.clone());
    }
    if let Some(report) = StorageRecovery::from_admission_code(code) {
        return Some(report);
    }
    match code {
        StartupFailureCode::UpgradeStorageFailed => Some(
            storage
                .and_then(|root| StorageRecovery::read(root).ok().flatten())
                .unwrap_or_else(unknown),
        ),
        StartupFailureCode::StorageUnavailable => Some(unknown()),
        _ => None,
    }
}

fn unknown() -> StorageRecovery {
    StorageRecovery::new(
        StorageRecoveryCause::UnknownState,
        StorageRecoveryAction::PreserveAndDiagnose,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_admission_failure_never_borrows_an_old_package_instruction() {
        let directory = tempfile::tempdir().unwrap();
        StorageRecovery::new(
            StorageRecoveryCause::MissingPackage,
            StorageRecoveryAction::PreserveAndDiagnose,
        )
        .store(directory.path())
        .unwrap();
        let report = failure_report(
            StartupFailureCode::StorageLocked,
            Some(directory.path()),
            None,
        )
        .unwrap();
        assert_eq!(report.cause, StorageRecoveryCause::Locked);
        assert_eq!(report.action, StorageRecoveryAction::PreserveAndDiagnose);
        assert!(
            report
                .operator_guidance()
                .contains("Close other owned HiRoute")
        );
        assert!(
            !report
                .operator_guidance()
                .contains("HIROUTE_RECOVERY_PACKAGE")
        );
        assert_eq!(
            StorageRecovery::read(directory.path())
                .unwrap()
                .unwrap()
                .cause,
            StorageRecoveryCause::MissingPackage,
            "a failed non-writer must not overwrite the old writer's report",
        );
        assert!(
            failure_report(
                StartupFailureCode::InvalidConfiguration,
                Some(directory.path()),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn missing_or_unreadable_current_report_only_grants_preservation() {
        let directory = tempfile::tempdir().unwrap();
        for code in [
            StartupFailureCode::StorageUnavailable,
            StartupFailureCode::UpgradeStorageFailed,
        ] {
            let report = failure_report(code, Some(directory.path()), None).unwrap();
            assert_eq!(report.action, StorageRecoveryAction::PreserveAndDiagnose);
            assert_eq!(report.cause, StorageRecoveryCause::UnknownState);
        }
    }

    #[test]
    fn current_typed_disposition_survives_both_daemon_launch_modes() {
        let directory = tempfile::tempdir().unwrap();
        StorageRecovery::new(
            StorageRecoveryCause::MissingPackage,
            StorageRecoveryAction::PreserveAndDiagnose,
        )
        .store(directory.path())
        .unwrap();
        let current = unknown();
        let error = role_failure(hiroute_daemon::RoleAllError::StorageRecovery(Box::new(
            current.clone(),
        )));
        let report = failure_report(
            error.code,
            Some(directory.path()),
            error.recovery.as_deref(),
        )
        .unwrap();
        assert_eq!(report, current);
        assert!(
            !report
                .operator_guidance()
                .contains("HIROUTE_RECOVERY_PACKAGE")
        );
    }
}
