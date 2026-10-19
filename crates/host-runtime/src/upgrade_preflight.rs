//! Read-only candidate preflight. It never starts the service or grants write authority.
use crate::StorageRecovery;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradePreflight {
    pub schema: String,
    pub history_cutover_required: bool,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub recovery: Option<StorageRecovery>,
}
pub fn run_upgrade_preflight(
    program: &Path,
    storage: &Path,
    package: &Path,
) -> Result<UpgradePreflight, &'static str> {
    let mut command = Command::new(program);
    command
        .arg("--upgrade-preflight")
        .arg("--storage-root")
        .arg(storage)
        .arg("--recovery-package")
        .arg(package)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "UPGRADE_PREFLIGHT_UNAVAILABLE")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("UPGRADE_PREFLIGHT_UNAVAILABLE");
            }
        }
    };
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or("UPGRADE_PREFLIGHT_INVALID")?
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "UPGRADE_PREFLIGHT_INVALID")?;
    if bytes.len() > 16 * 1024 {
        return Err("UPGRADE_PREFLIGHT_INVALID");
    }
    let report: UpgradePreflight =
        serde_json::from_slice(&bytes).map_err(|_| "UPGRADE_PREFLIGHT_UNAVAILABLE")?;
    if report.schema != "hiroute.upgrade-preflight/v1" {
        return Err("UPGRADE_PREFLIGHT_INVALID");
    }
    if !status.success() || report.recovery.is_some() {
        return Err(match report.recovery.map(|r| r.cause) {
            Some(crate::StorageRecoveryCause::InsufficientSpace) => "UPGRADE_INSUFFICIENT_SPACE",
            Some(crate::StorageRecoveryCause::MissingKey) => "UPGRADE_KEY_UNAVAILABLE",
            Some(crate::StorageRecoveryCause::InvalidPackage) => "UPGRADE_RECOVERY_PACKAGE_INVALID",
            _ => "UPGRADE_PREFLIGHT_REFUSED",
        });
    }
    Ok(report)
}
