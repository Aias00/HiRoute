//! Test-only observations from the actual system PowerShell parser process.
//! A timeout stays failed; output is classified after exact Job cleanup, never printed.
#[derive(Debug, Eq, PartialEq)]
struct Trace {
    entered: bool,
    parse_started: bool,
    accepted: bool,
    rejected: bool,
    complete: bool,
    unknown: bool,
    failure: Option<(i32, u8)>,
}

fn trace(bytes: &[u8], arm: &str) -> Trace {
    let mut result = Trace {
        entered: false,
        parse_started: false,
        accepted: false,
        rejected: false,
        complete: false,
        unknown: false,
        failure: None,
    };
    let Ok(text) = std::str::from_utf8(bytes) else {
        result.unknown = true;
        return result;
    };
    let prefix = format!("HiRouteParser/v1 arm={arm} stage=");
    let lines: Vec<_> = text.lines().map(str::trim).collect();
    let mut stages = Vec::new();
    for line in lines {
        match line.strip_prefix(&prefix) {
            Some("entered") => {
                result.entered = true;
                stages.push("entered");
            }
            Some("parse-started") => {
                result.parse_started = true;
                stages.push("parse-started");
            }
            Some("accepted") => {
                result.accepted = true;
                stages.push("accepted");
            }
            Some("rejected") => {
                result.rejected = true;
                stages.push("rejected");
            }
            Some("complete") => {
                result.complete = true;
                stages.push("complete");
            }
            Some(value) if value.starts_with("exception hresult=") => {
                let failure = value["exception hresult=".len()..]
                    .split_once(" category=")
                    .and_then(|(code, category)| {
                        let number = code.parse::<i32>().ok()?;
                        let kind = category.parse::<u8>().ok()?;
                        (number.to_string() == code && kind.to_string() == category && kind <= 31)
                            .then_some((number, kind))
                    });
                if result.failure.is_some() || failure.is_none() {
                    result.unknown = true;
                    result.failure = None;
                } else {
                    result.failure = failure;
                }
                stages.push("exception");
            }
            _ => result.unknown = true,
        }
    }
    let partial = ["entered", "parse-started"];
    let successful = ["entered", "parse-started", "accepted", "complete"];
    let rejected = ["entered", "parse-started", "rejected", "complete"];
    if !(stages.is_empty()
        || partial.starts_with(&stages)
        || stages == successful
        || stages == rejected
        || stages.last() == Some(&"exception") && partial.starts_with(&stages[..stages.len() - 1]))
    {
        result.unknown = true;
    }
    result
}

#[cfg(any(windows, test))]
pub(super) fn script(escaped: &str, arm: &str) -> String {
    let prefix = format!("HiRouteParser/v1 arm={arm} stage=");
    // The parser observation itself needs only language expressions and .NET;
    // emitting a checkpoint must not add an unrelated cmdlet discovery step.
    format!(
        "$ErrorActionPreference='Stop'; try {{ [Console]::WriteLine('{prefix}entered'); $tokens=$null; $errors=$null; [Console]::WriteLine('{prefix}parse-started'); $null=[System.Management.Automation.Language.Parser]::ParseInput('{escaped}',[ref]$tokens,[ref]$errors); if($errors.Count -eq 0){{[Console]::WriteLine('{prefix}accepted')}}else{{[Console]::WriteLine('{prefix}rejected')}}; [Console]::WriteLine('{prefix}complete') }} catch {{ [Console]::WriteLine('{prefix}exception hresult=' + [string][int]$_.Exception.HResult + ' category=' + [string][int]$_.CategoryInfo.Category); exit 1 }}"
    )
}

#[cfg(windows)]
pub(super) fn check(
    command: &std::process::Command,
    timeout: std::time::Duration,
    arm: &str,
    expected: &str,
) -> std::io::Result<()> {
    use hiroute_platform::windows::job::Stream;
    use std::io::{self, Read, Seek, SeekFrom};
    use std::time::{Duration, Instant};

    let capture = super::super::super::probe_files::temporary("hiroute-parser-capture-")?;
    let stdout = super::super::super::probe_files::file(&capture.path().join("stdout"))?;
    let stderr = super::super::super::probe_files::file(&capture.path().join("stderr"))?;
    let child = super::super::super::windows_runtime::spawn(
        command,
        false,
        Stream::Null,
        Stream::File(&stdout),
        Stream::File(&stderr),
    )?;
    let start = Instant::now();
    let mut natural_exit = None;
    let outcome = (|| {
        loop {
            if stdout.metadata()?.len() > 1024 || stderr.metadata()?.len() > 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "parser capture bound",
                ));
            }
            if let Some(code) = child.exit_code()? {
                natural_exit = Some(code);
                return if code == 0 {
                    Ok(())
                } else {
                    Err(io::Error::other("parser process exit"))
                };
            }
            if start.elapsed() >= timeout {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "native parser timeout",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    // Metadata does not move the capture's shared file pointer. Do not seek/read
    // while a native writer is alive, including after its root process exits.
    let root_status = child.exit_code();
    let empty_status = child.empty();
    let stdout_status = stdout.metadata().map(|value| value.len());
    let stderr_status = stderr.metadata().map(|value| value.len());
    let root_running = root_status.as_ref().ok().map(Option::is_none);
    let empty_before_cleanup = empty_status.as_ref().ok().copied();
    let stdout_len = stdout_status.as_ref().ok().copied();
    let stderr_len = stderr_status.as_ref().ok().copied();
    let snapshot_error = root_status
        .as_ref()
        .err()
        .or(empty_status.as_ref().err())
        .or(stdout_status.as_ref().err())
        .or(stderr_status.as_ref().err())
        .map(|error| (error.kind(), error.raw_os_error()));
    let cleanup = (|| {
        child.terminate()?;
        let stop = Instant::now();
        while !child.empty()? {
            if stop.elapsed() >= Duration::from_secs(1) {
                return Err(io::Error::other("parser Job cleanup unconfirmed"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    })();
    if cleanup.is_err() {
        eprintln!("native readonly parser facts: arm={arm} cleanup_confirmed=false");
        let _ = capture.keep();
        return Err(io::Error::other("parser Job cleanup unconfirmed"));
    }
    let read = |file: &std::fs::File| -> io::Result<Vec<u8>> {
        let mut file = file.try_clone()?;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(1025).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "parser capture bound",
            ));
        }
        Ok(bytes)
    };
    let stdout = read(&stdout)?;
    let stderr = read(&stderr)?;
    let observed = trace(&stdout, arm);
    eprintln!(
        "native readonly parser facts: arm={arm} timed_out={} natural_exit={natural_exit:?} root_running={root_running:?} job_empty_before_cleanup={empty_before_cleanup:?} stdout_bytes={stdout_len:?} stderr_bytes={stderr_len:?} snapshot_error={snapshot_error:?} cleanup_confirmed=true trace={observed:?}",
        outcome
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::TimedOut),
    );
    if snapshot_error.is_some() {
        return Err(io::Error::other("parser snapshot unavailable"));
    }
    outcome?;
    if !stderr.is_empty()
        || observed.unknown
        || observed.failure.is_some()
        || !observed.complete
        || !(expected == "accepted" && observed.accepted
            || expected == "rejected" && observed.rejected)
    {
        return Err(io::Error::other("parser fixed receipt differs"));
    }
    Ok(())
}

#[test]
fn readonly_parser_trace_requires_ordered_actual_entry_parse_and_completion() {
    let prefix = "HiRouteParser/v1 arm=complete stage=";
    let valid = format!(
        "{prefix}entered\r\n{prefix}parse-started\r\n{prefix}accepted\r\n{prefix}complete\r\n"
    );
    let receipt = trace(valid.as_bytes(), "complete");
    assert!(receipt.entered && receipt.parse_started && receipt.accepted && receipt.complete);
    assert!(!receipt.unknown);
    for body in [
        format!("{prefix}accepted\n{prefix}complete"),
        format!("{prefix}parse-started\n{prefix}entered"),
        format!("{prefix}entered\n{prefix}entered"),
        format!("echo '{prefix}entered'; '{prefix}accepted'"),
        valid.replace("arm=complete", "arm=other"),
    ] {
        assert!(trace(body.as_bytes(), "complete").unknown);
    }
    let before_parse = trace(format!("{prefix}entered\n").as_bytes(), "complete");
    assert!(before_parse.entered && !before_parse.parse_started && !before_parse.complete);
    assert!(!before_parse.unknown);
    let before_entry = trace(&[], "complete");
    assert!(!before_entry.entered && !before_entry.complete && !before_entry.unknown);
}

#[test]
fn readonly_parser_trace_limits_native_failure_facts_without_retaining_dynamic_output() {
    let prefix = "HiRouteParser/v1 arm=incomplete stage=";
    let receipt = trace(format!("{prefix}entered\n{prefix}parse-started\n{prefix}exception hresult=-2147024891 category=18").as_bytes(), "incomplete");
    assert_eq!(receipt.failure, Some((-2147024891, 18)));
    assert!(!receipt.complete && !receipt.unknown);
    for body in [
        "exception hresult=+5 category=18",
        "exception hresult=05 category=18",
        "exception hresult=2147483648 category=18",
        "exception hresult=-1 category=32",
        "exception hresult=-1 category=18 private-account-path",
    ] {
        let receipt = trace(format!("{prefix}{body}").as_bytes(), "incomplete");
        assert!(receipt.unknown);
        assert_eq!(receipt.failure, None);
        assert!(!format!("{receipt:?}").contains("private-account-path"));
    }
    assert!(trace(b"\xff", "incomplete").unknown);
}

#[test]
fn readonly_parser_console_frames_preserve_parse_input_and_reject_echo_as_completion() {
    let literal = "'fixture-literal'".replace('\'', "''");
    let command = script(&literal, "complete");
    assert!(!command.contains("Write-Output"));
    assert!(!command.contains('\n'));
    assert!(command.contains(&format!(
        "::ParseInput('{literal}',[ref]$tokens,[ref]$errors)"
    )));
    for phase in [
        "entered",
        "parse-started",
        "accepted",
        "rejected",
        "complete",
    ] {
        assert!(command.contains(&format!(
            "[Console]::WriteLine('HiRouteParser/v1 arm=complete stage={phase}')"
        )));
    }
    assert!(command.contains(
        "[Console]::WriteLine('HiRouteParser/v1 arm=complete stage=exception hresult=' +"
    ));
    assert!(command.ends_with("; exit 1 }"));
    let echoed = trace(command.as_bytes(), "complete");
    assert!(echoed.unknown && !echoed.entered && !echoed.accepted && !echoed.complete);
    let prefix = "HiRouteParser/v1 arm=complete stage=";
    for result in ["accepted", "rejected"] {
        let bytes = format!(
            "{prefix}entered\r\n{prefix}parse-started\r\n{prefix}{result}\r\n{prefix}complete\r\n"
        );
        let observed = trace(bytes.as_bytes(), "complete");
        assert!(observed.entered && observed.parse_started && observed.complete);
        assert_eq!(observed.accepted, result == "accepted");
        assert_eq!(observed.rejected, result == "rejected");
        assert!(!observed.unknown && observed.failure.is_none());
    }
}
