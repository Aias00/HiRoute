//! Windows Live checks retain an atomic Job and capture bounded private files.
use super::*;

fn private_capture() -> std::io::Result<tempfile::NamedTempFile> {
    let capture = tempfile::NamedTempFile::new()?;
    hiroute_platform::windows::security::set_owner_only(capture.path(), false)?;
    Ok(capture)
}

fn read_capture(capture: &tempfile::NamedTempFile) -> std::io::Result<Vec<u8>> {
    use std::io::{Seek, SeekFrom};
    let mut file = capture.as_file().try_clone()?;
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(OUTPUT_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > OUTPUT_LIMIT {
        return Err(std::io::Error::other("native output bound"));
    }
    Ok(bytes)
}

pub(super) fn run_captured(
    command: &Command,
    inherit: bool,
    deadline: Instant,
    working: &mut LiveWorkspace,
) -> Result<(Vec<u8>, bool), AttemptFailure> {
    let unavailable = || AttemptFailure {
        executed: false,
        reason: CLIENT_FAILED,
    };
    if Instant::now() >= deadline {
        return Err(AttemptFailure {
            executed: false,
            reason: CHECK_TIMEOUT,
        });
    }
    let stdout = private_capture().map_err(|_| unavailable())?;
    let stderr = private_capture().map_err(|_| unavailable())?;
    let writer = stdout.reopen().map_err(|_| unavailable())?;
    let errors = stderr.reopen().map_err(|_| unavailable())?;
    let mut child = hiroute_integrations::agents::spawn_windows_native_probe(
        command,
        inherit,
        &writer,
        Some(&errors),
    )
    .map_err(|_| unavailable())?;
    let failure = |reason| AttemptFailure {
        executed: true,
        reason,
    };
    let result = (|| {
        loop {
            if [stdout.as_file(), stderr.as_file()]
                .into_iter()
                .any(|file| {
                    file.metadata()
                        .map(|metadata| metadata.len() > OUTPUT_LIMIT as u64)
                        .unwrap_or(true)
                })
            {
                return Err(failure(CLIENT_OUTPUT_INVALID));
            }
            if let Some(success) = child.observe().map_err(|_| failure(CLIENT_FAILED))? {
                break Ok(success);
            }
            if Instant::now() >= deadline {
                break Err(failure(CHECK_TIMEOUT));
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    // Even a successful leader can leave a tool running. Stop precedes final reads
    // and destruction of all captured files and the native working directory.
    if child.stop().is_err() {
        working.keep();
        let _ = stdout.keep();
        let _ = stderr.keep();
        return Err(failure(CLIENT_FAILED));
    }
    let success = result?;
    let bytes = read_capture(&stdout).map_err(|_| failure(CLIENT_OUTPUT_INVALID))?;
    let errors = read_capture(&stderr).map_err(|_| failure(CLIENT_OUTPUT_INVALID))?;
    drop(zeroize::Zeroizing::new(errors));
    Ok((bytes, success))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIXTURE: &str =
        "control::runtime::agent_live_check::windows::tests::windows_live_capture_fixture";

    fn fixture(mode: &str, working: &LiveWorkspace) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", FIXTURE, "--nocapture"])
            .env("HIROUTE_LIVE_CAPTURE_FIXTURE", mode)
            .current_dir(working.path());
        command
    }

    #[test]
    #[ignore = "selected only as the actual native process child"]
    fn windows_live_capture_fixture() {
        match std::env::var("HIROUTE_LIVE_CAPTURE_FIXTURE")
            .unwrap()
            .as_str()
        {
            "success" => println!("{LIVE_PROMPT_RESPONSE}"),
            "stderr-overflow" => {
                std::io::stderr()
                    .write_all(&vec![b'x'; OUTPUT_LIMIT + 1])
                    .unwrap();
            }
            "timeout" => {
                let child = Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", FIXTURE, "--nocapture"])
                    .env("HIROUTE_LIVE_CAPTURE_FIXTURE", "tool")
                    .spawn()
                    .unwrap();
                std::fs::write("tool.pid", child.id().to_string()).unwrap();
                thread::sleep(Duration::from_secs(60));
            }
            "tool" => thread::sleep(Duration::from_secs(60)),
            _ => panic!("unknown capture fixture"),
        }
    }

    #[test]
    fn windows_live_capture_observes_exit_and_bounds_final_stderr() {
        let mut working = LiveWorkspace::new().unwrap();
        let command = fixture("success", &working);
        let result = run_captured(
            &command,
            true,
            Instant::now() + Duration::from_secs(10),
            &mut working,
        );
        let (bytes, success) = result.unwrap_or_else(|failure| {
            panic!(
                "capture failed: executed={} reason={}",
                failure.executed, failure.reason
            )
        });
        assert!(success);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains(LIVE_PROMPT_RESPONSE)
        );
        let command = fixture("stderr-overflow", &working);
        let failure = run_captured(
            &command,
            true,
            Instant::now() + Duration::from_secs(10),
            &mut working,
        )
        .err()
        .expect("a fast stderr writer cannot bypass the final bound");
        assert!(failure.executed);
        assert_eq!(failure.reason, CLIENT_OUTPUT_INVALID);
    }

    #[test]
    fn windows_live_capture_timeout_stops_the_owned_tool_tree() {
        let mut working = LiveWorkspace::new().unwrap();
        let command = fixture("timeout", &working);
        let failure = run_captured(
            &command,
            true,
            Instant::now() + Duration::from_secs(3),
            &mut working,
        )
        .err()
        .expect("a hanging native client cannot pass a bounded check");
        assert!(failure.executed);
        assert_eq!(failure.reason, CHECK_TIMEOUT);
        let pid = std::fs::read_to_string(working.path().join("tool.pid"))
            .expect("the fixture must launch its real ordinary descendant")
            .parse::<u32>()
            .unwrap();
        assert!(!hiroute_platform::windows::job::process_is_running(pid).unwrap());
    }
}
