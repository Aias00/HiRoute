//! Project only the production closed stage after bounded capture and secret checks.
#[path = "../../../integrations/src/agents/qoder_probe_cli_diagnostic.rs"]
mod cli_diagnostic;
#[path = "../../src/control/runtime/agent_connection_qoder_diagnostic.rs"]
mod diagnostic;

pub(super) fn project(bytes: &[u8]) {
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if let Some(reason) = line
            .strip_prefix(b"native collaboration check: native ingress probe could not establish ")
        {
            let stage = match reason {
                b"trusted CLI baseline" => "cli-baseline",
                b"trusted CLI contract" | b"CLI contract" => "cli-contract",
                b"authenticated native completion" => "native-completion",
                b"native read-only write rejection" => "readonly-control",
                b"native Skill read and trusted CLI execution" => "tool-receipts",
                b"native Skill discovery" => "skill-discovery",
                b"native launch" => "native-launch",
                b"deadline" => "deadline",
                _ => "other-native-check",
            };
            eprintln!("Windows collaboration daemon failure: stage={stage}");
        }
        if let Some(facts) = cli_diagnostic::Facts::parse(line) {
            facts.emit();
        }
        if let Some(stage) = diagnostic::line(line) {
            eprintln!("{}{}", diagnostic::PREFIX, stage);
        }
    }
}

pub(super) fn collect(reader: &mut Option<std::thread::JoinHandle<Vec<u8>>>, stderr: bool) {
    if let Some(reader) = reader.take() {
        let bytes = reader
            .join()
            .expect("bounded daemon output collector failed");
        super::assert_no_secret(&bytes);
        if stderr {
            project(&bytes);
        }
    }
}

/// Save the final writer bytes before the disposable installation is removed.
/// Periodic out-of-process collection can otherwise miss the failure itself.
pub(super) fn retain(root: &std::path::Path) {
    use hiroute_platform::windows::security;
    use std::io::Write;
    let Some(destination) = std::env::var_os("HIROUTE_VALIDATION_EVIDENCE_DIR") else {
        return;
    };
    let save = || -> std::io::Result<()> {
        let destination = std::path::PathBuf::from(destination);
        security::open_owner_only(&destination, true)?;
        let mut index = 0;
        for role in ["daemon", "desktop"] {
            let directory = root.join(role);
            if !directory.exists() {
                continue;
            }
            security::open_owner_only(&directory, true)?;
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                if path
                    .extension()
                    .is_none_or(|extension| extension != "jsonl")
                {
                    continue;
                }
                let mut input = security::open_owner_only(&path, false)?;
                if input.metadata()?.len() > 8 * 1024 * 1024 {
                    continue;
                }
                let path = destination.join(format!("diagnostics-final-{index}.jsonl"));
                let mut output = security::create_owner_only_file(&path)?;
                std::io::copy(&mut input, &mut output)?;
                output.flush()?;
                index += 1;
            }
        }
        Ok(())
    };
    if save().is_err() {
        eprintln!("Windows failure diagnostic retention could not complete");
    }
}
