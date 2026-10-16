//! Windows service logs read the diagnostic writer's files through held directories.
//! Settings, correlation keys and unrecognized entries are never log content.

use hiroute_diagnostics::event::ProcessRole;
use hiroute_diagnostics::files::{
    CURRENT_LOG_FILE, FileSafetyError, PrivateDir, previous_log_file, role_dir_name,
};
use hiroute_host_runtime::StandaloneLayout;
use serde_json::{Value, json};

use super::super::{MAX_LOG_BYTES, ServiceFailure};

pub(super) fn read(layout: &StandaloneLayout, manager: &str) -> Result<Value, ServiceFailure> {
    let root = layout.diagnostics_root();
    let directory = PrivateDir::open_existing(&root).map_err(super::failure)?;
    let files = read_files(&directory).map_err(super::failure)?;
    Ok(json!({
        "schema":"hiroute.standalone-service-logs/v1", "manager":manager,
        "diagnostics_root":root, "files":files,
    }))
}

fn read_files(root: &PrivateDir) -> Result<Vec<Value>, FileSafetyError> {
    let role_name = role_dir_name(ProcessRole::Daemon);
    let Some(role) = root.open_existing_child(role_name)? else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    // The production writer rotates current into previous-1. Lexicographic directory
    // order does not identify the newest logs and may include secret settings/key files.
    for name in [CURRENT_LOG_FILE.to_owned(), previous_log_file(1)] {
        if let Some(mut file) = role.open_read(&name)? {
            file.recheck()?;
            let bytes = file.read_tail(MAX_LOG_BYTES as u64)?;
            file.recheck()?;
            output.push(json!({
                "file":format!("{role_name}/{name}"),
                "output":String::from_utf8_lossy(&bytes),
            }));
        }
    }
    Ok(output)
}

#[cfg(test)]
#[path = "logs/tests.rs"]
mod tests;
