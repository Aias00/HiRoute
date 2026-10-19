//! Share native Windows npm resolution without evaluating a command-script shim.
use super::managed_launch::ManagedLaunchFailure;
use std::path::Path;
use std::process::Command;

pub(super) fn selected(program: &Path) -> Result<Command, ManagedLaunchFailure> {
    #[cfg(not(windows))]
    {
        Ok(Command::new(program))
    }
    #[cfg(windows)]
    {
        let search = std::env::var_os("PATH")
            .unwrap_or_default()
            .into_string()
            .map_err(|_| ManagedLaunchFailure::Unavailable)?;
        let (program, prefix) = hiroute_integrations::windows_agent_command(program, &[], &search)
            .map_err(|_| ManagedLaunchFailure::Unavailable)?;
        let mut command = Command::new(program);
        command.args(prefix);
        Ok(command)
    }
}

pub(super) fn native(name: &str) -> Result<Command, ManagedLaunchFailure> {
    #[cfg(not(windows))]
    {
        Ok(Command::new(name))
    }
    #[cfg(windows)]
    {
        let search = std::env::var_os("PATH").ok_or(ManagedLaunchFailure::Unavailable)?;
        for directory in std::env::split_paths(&search) {
            if !directory.is_absolute() {
                continue;
            }
            for extension in ["exe", "cmd"] {
                let candidate = directory.join(name).with_extension(extension);
                if candidate.is_file() {
                    return selected(&candidate);
                }
            }
        }
        Err(ManagedLaunchFailure::Unavailable)
    }
}
