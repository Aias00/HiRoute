//! A shell/installer failure cannot borrow an earlier storage writer's advice.
use hiroute_host_runtime::StorageRecovery;
use std::path::Path;

pub(super) fn resolve(
    code: Option<&'static str>,
    storage: &Path,
    diagnostics: &Path,
) -> (Option<&'static str>, Option<StorageRecovery>) {
    // A failed manager command does not prove that a new daemon even started.
    // Only the completed readiness wait may project the latest daemon attempt.
    if code != Some("SERVICE_NOT_READY") {
        return (code, None);
    }
    let Some(current) = StorageRecovery::read_failed_startup_code(diagnostics) else {
        return (code, None);
    };
    let storage_failure = matches!(
        current,
        "DAEMON_STORAGE_UNREADABLE"
            | "DAEMON_STORAGE_ACCESS_DENIED"
            | "DAEMON_STORAGE_LOCKED"
            | "DAEMON_STORAGE_KEY_UNAVAILABLE"
            | "DAEMON_UPGRADE_SOURCE_UNSUPPORTED"
            | "DAEMON_UPGRADE_STORAGE_FAILED"
    );
    let recovery = if storage_failure {
        StorageRecovery::read_for_failed_startup(storage, diagnostics)
    } else {
        None
    };
    (Some(current), recovery)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn non_storage_failures_and_unproven_attempts_do_not_load_an_old_report() {
        let absent = Path::new("a-path-that-does-not-establish-storage-authority");
        for code in [
            "SHELL_INSTALL_RECOVERY_REQUIRED",
            "CONSOLE_LISTENER_UNAVAILABLE",
            "CONSOLE_LAUNCH_UNAVAILABLE",
            "INSTALLATION_UNAVAILABLE",
            "SERVICE_START_FAILED",
            "SERVICE_COMMAND_FAILED",
            "SERVICE_COMMAND_TIMED_OUT",
            "SERVICE_NOT_READY",
        ] {
            let (actual, recovery) = resolve(Some(code), absent, absent);
            assert_eq!(actual, Some(code));
            assert!(recovery.is_none());
        }
    }
}
