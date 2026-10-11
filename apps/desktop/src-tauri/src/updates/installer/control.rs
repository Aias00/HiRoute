//! Parent-owned precommit helper. Only an explicit decision authorizes installation.
use super::write_private;
use hiroute_diagnostics::files::PrivateDir;
use std::{
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

pub(super) const STARTED: &[u8] = b"hiroute.upgrade-installer-started/v1\n";
const COMMIT: &[u8] = b"hiroute.upgrade-installer-decision/v1\ncommit\n";
const CANCEL: &[u8] = b"hiroute.upgrade-installer-decision/v1\ncancel\n";

pub(in crate::updates) struct Installer {
    child: Child,
    directory: PrivateDir,
    committed: bool,
}
impl Installer {
    pub(in crate::updates) fn commit(mut self) -> Result<(), String> {
        if let Some(status) = self
            .child
            .try_wait()
            .map_err(|_| "UPGRADE_INSTALLER_WAIT_FAILED")?
        {
            return Err(failure(&self.directory, status));
        }
        self.directory
            .recheck_identity()
            .map_err(|_| "UPGRADE_INSTALLER_INVALID")?;
        write_private(&self.directory.path().join("decision"), COMMIT)?;
        self.committed = true;
        Ok(())
    }
}
impl Drop for Installer {
    fn drop(&mut self) {
        if !self.committed {
            let _ = write_private(&self.directory.path().join("decision"), CANCEL);
            // This is our precommit helper, never a daemon/Worker. It has no authorization
            // to replace an App; reap it before returning control to the old service.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
pub(in crate::updates) fn start(path: &Path) -> Result<Installer, String> {
    let mut command =
        Command::new(std::env::current_exe().map_err(|_| "UPGRADE_INSTALLER_UNAVAILABLE")?);
    command.arg("--finish-upgrade").arg(path);
    start_command(path, command, Duration::from_secs(10))
}
fn start_command(
    path: &Path,
    mut command: Command,
    timeout: Duration,
) -> Result<Installer, String> {
    let directory = PrivateDir::open_existing(path.parent().ok_or("UPGRADE_INSTALLER_INVALID")?)
        .map_err(|_| "UPGRADE_INSTALLER_INVALID")?;
    use std::os::unix::fs::OpenOptionsExt;
    let log = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.path().join("installer.log"))
        .map_err(|_| "UPGRADE_INSTALLER_WRITE_FAILED")?;
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|_| "UPGRADE_INSTALLER_UNAVAILABLE")?;
    let mut installer = Installer {
        child,
        directory,
        committed: false,
    };
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = installer
            .child
            .try_wait()
            .map_err(|_| "UPGRADE_INSTALLER_WAIT_FAILED")?
        {
            return Err(failure(&installer.directory, status));
        }
        if let Some(mut ack) = installer
            .directory
            .open_read("started")
            .map_err(|_| "UPGRADE_INSTALLER_INVALID")?
        {
            if ack
                .read_prefix(64)
                .map_err(|_| "UPGRADE_INSTALLER_INVALID")?
                == STARTED
            {
                return Ok(installer);
            }
            return Err("UPGRADE_INSTALLER_INVALID".into());
        }
        if Instant::now() >= deadline {
            return Err("UPGRADE_INSTALLER_START_TIMEOUT".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn stable_code(code: &str) -> bool {
    code.len() <= 96
        && (code.starts_with("UPGRADE_")
            || code.starts_with("PACKAGE_")
            || code.starts_with("PRIVATE_"))
        && code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}
fn failure(directory: &PrivateDir, status: ExitStatus) -> String {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Failure {
        code: String,
    }
    let read = |name: &str| -> Option<Vec<u8>> {
        let mut file = directory.open_read(name).ok()??;
        if file.len() > 512 {
            return None;
        }
        let bytes = file.read_prefix(512).ok()?;
        file.recheck().ok()?;
        Some(bytes)
    };
    if let Some(value) =
        read("failed.json").and_then(|bytes| serde_json::from_slice::<Failure>(&bytes).ok())
        && stable_code(&value.code)
    {
        return value.code;
    }
    // load failures cannot authorize a result write beside an untrusted handoff. stderr
    // was opened by this parent; accept only a closed code, never arbitrary output.
    if let Some(code) = read("installer.log").and_then(|bytes| String::from_utf8(bytes).ok())
        && stable_code(code.trim())
    {
        return code.trim().to_owned();
    }
    use std::os::unix::process::ExitStatusExt;
    if status.signal().is_some() {
        "UPGRADE_INSTALLER_TERMINATED".into()
    } else {
        "UPGRADE_INSTALLER_EXIT_FAILED".into()
    }
}
pub(super) fn await_commit(
    directory: &PrivateDir,
    parent_pid: u32,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(mut decision) = directory
            .open_read("decision")
            .map_err(|_| "UPGRADE_INSTALLER_INVALID")?
        {
            let bytes = decision
                .read_prefix(128)
                .map_err(|_| "UPGRADE_INSTALLER_INVALID")?;
            decision
                .recheck()
                .map_err(|_| "UPGRADE_INSTALLER_INVALID")?;
            return match bytes.as_slice() {
                COMMIT => Ok(()),
                CANCEL => Err("UPGRADE_INSTALLER_CANCELLED".into()),
                _ => Err("UPGRADE_INSTALLER_DECISION_INVALID".into()),
            };
        }
        match nix::sys::signal::kill(nix::unistd::Pid::from_raw(parent_pid as i32), None) {
            Err(nix::errno::Errno::ESRCH) => return Err("UPGRADE_INSTALLER_COMMIT_REQUIRED".into()),
            Err(_) => return Err("UPGRADE_INSTALLER_PARENT_UNKNOWN".into()),
            Ok(()) => {}
        }
        if Instant::now() >= deadline {
            return Err("UPGRADE_INSTALLER_COMMIT_TIMEOUT".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests;
