//! Project only the production closed stage after bounded capture and secret checks.
#[path = "../../../integrations/src/agents/qoder_probe_cli_diagnostic.rs"]
mod cli_diagnostic;
#[path = "../../src/control/runtime/agent_connection_qoder_diagnostic.rs"]
mod diagnostic;

pub(super) fn project(bytes: &[u8]) {
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
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
