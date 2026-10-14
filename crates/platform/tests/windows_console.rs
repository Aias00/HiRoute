#![cfg(windows)]
#![windows_subsystem = "windows"]
//! Actual GUI-process attachment with explicit pipes, never the runner's console.
use std::ffi::OsString;
use std::io;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use hiroute_platform::windows::console::{self, ParentConsoleAttachment};
use hiroute_platform::windows::job::{Child, Launch, Stream};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};

const MODE: &str = "HIROUTE_NATIVE_CONSOLE_FIXTURE";
const FRAME: &str = "HiRouteConsoleFixture/v1";
const FACTS: &str = "HiRouteConsoleFacts/v1 ";

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    HostBeforeAttached,
    HostAfterChild,
    WithParentBeforeAttach,
    NoParentBeforeAttach,
    AfterAttach,
    HandlesChecked,
    InputBeforeRead,
    InputAfterRead,
    NewlineFrameWritten,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConsoleFacts {
    stage: Stage,
    attached: Option<bool>,
    file_types: [u32; 3],
    os_error: Option<u32>,
    attachment: Option<u8>,
    handles_preserved: Option<bool>,
    input_matches: Option<bool>,
}

fn file_types() -> [u32; 3] {
    [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].map(|name| {
        // SAFETY: observe only this process's borrowed standard handles.
        unsafe { GetFileType(GetStdHandle(name)) }
    })
}

fn emit_facts(
    stage: Stage,
    attached: Option<bool>,
    error: Option<&io::Error>,
    attachment: Option<u8>,
    handles_preserved: Option<bool>,
    input_matches: Option<bool>,
) {
    let facts = ConsoleFacts {
        stage,
        attached,
        file_types: file_types(),
        os_error: error
            .and_then(io::Error::raw_os_error)
            .map(|code| code as u32),
        attachment,
        handles_preserved,
        input_matches,
    };
    // A leading newline separates the closed frame from libtest's pretty caption.
    println!("\n{FACTS}{}", serde_json::to_string(&facts).unwrap());
    std::io::stdout().flush().unwrap();
}

fn observe(stage: Stage) -> bool {
    let attached = console::is_attached();
    emit_facts(
        stage,
        attached.as_ref().ok().copied(),
        attached.as_ref().err(),
        None,
        None,
        None,
    );
    attached.expect("console attachment query must succeed")
}

fn frame_count(bytes: &[u8]) -> Option<usize> {
    std::str::from_utf8(bytes)
        .ok()
        .map(|text| text.lines().filter(|line| *line == FRAME).count())
}

fn project_facts(output: &[u8]) -> Vec<ConsoleFacts> {
    // Raw libtest output remains in memory. Only the closed fixture schema can
    // leave this boundary; panic text, paths and the environment are never echoed.
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| line.strip_prefix(FACTS))
        .filter_map(|value| serde_json::from_str::<ConsoleFacts>(value).ok())
        .filter(|facts| {
            facts.file_types.iter().all(|kind| matches!(kind, 0..=3))
                && facts.attachment.is_none_or(|value| value <= 2)
        })
        .collect()
}

fn spawn(mode: &str, isolated_console: bool) -> Child {
    let executable = std::env::current_exe().unwrap();
    let args = [
        "--ignored",
        "--exact",
        "console_fixture",
        "--nocapture",
        "--test-threads=1",
    ]
    .map(OsString::from);
    let mut environment: Vec<_> = std::env::vars_os()
        .filter(|(name, _)| name != MODE)
        .collect();
    environment.push((MODE.into(), mode.into()));
    let launch = Launch {
        program: &executable,
        args: &args,
        env: &environment,
        cwd: executable.parent(),
        stdin: Stream::Piped,
        stdout: Stream::Piped,
        stderr: Stream::Piped,
    };
    if isolated_console {
        Child::spawn_console_host(launch).unwrap()
    } else {
        Child::spawn(launch).unwrap()
    }
}

fn run(mode: &str, isolated_console: bool) {
    assert!(matches!(
        mode,
        "no-parent" | "with-parent" | "isolated-host"
    ));
    // The old fixture omitted the newline and libtest's pretty caption consumed
    // its exact frame. Preserve both sides of this causal control in the real gate.
    assert_eq!(
        frame_count(format!("test console_fixture ... {FRAME}\n").as_bytes()),
        Some(0)
    );
    assert_eq!(
        frame_count(format!("test console_fixture ... \n{FRAME}\n").as_bytes()),
        Some(1)
    );
    let mut child = spawn(mode, isolated_console);
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"literal-pipe-proof")
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |mut file: std::fs::File| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let read = Read::by_ref(&mut file)
                .take(16 * 1024 + 1)
                .read_to_end(&mut bytes);
            (
                bytes,
                read.is_ok(),
                read.err().and_then(|error| error.raw_os_error()),
            )
        })
    };
    let output = read(stdout);
    let error = read(stderr);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut code = None;
    let mut native_error = None;
    let mut natural = false;
    loop {
        match child.exit_code() {
            Ok(value) => code = value,
            Err(error) => {
                native_error = error.raw_os_error();
                break;
            }
        }
        match child.empty() {
            Ok(true) if code.is_some() => {
                natural = true;
                break;
            }
            Ok(_) => {}
            Err(error) => {
                native_error = error.raw_os_error();
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // A failure must not abort before its held Job and pipes have been observed.
    // Forced termination is cleanup only and can never satisfy the natural gate.
    if !natural && let Err(error) = child.terminate() {
        native_error = error.raw_os_error();
    }
    let cleanup_deadline = Instant::now() + Duration::from_secs(2);
    let stopped = loop {
        if child.empty().unwrap_or(false) && child.exit_code().ok().flatten().is_some() {
            break true;
        }
        if Instant::now() >= cleanup_deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if !stopped {
        let summary = serde_json::json!({"stage": "job-cleanup", "natural": false, "job_empty": false, "os_error": native_error});
        println!("\nHiRouteConsoleDiagnostic/v1 {summary}");
        panic!("console fixture exact Job cleanup was not proven");
    }
    let (output, output_read, output_error) = output.join().expect("stdout reader must not panic");
    let (error, error_read, error_code) = error.join().expect("stderr reader must not panic");
    let frames = frame_count(&output);
    let bounded = output.len() <= 16 * 1024 && error.len() <= 16 * 1024;
    let success = natural
        && code == Some(0)
        && output_read
        && error_read
        && bounded
        && error.is_empty()
        && frames == Some(1);
    if !success {
        for facts in project_facts(&output) {
            println!("\n{FACTS}{}", serde_json::to_string(&facts).unwrap());
        }
        let summary = serde_json::json!({
            "stage": "job-stopped", "mode": mode, "natural": natural,
            "job_empty": stopped, "exit_code": code, "os_error": native_error,
            "stdout_bytes": output.len(), "stderr_bytes": error.len(),
            "stdout_read": output_read, "stderr_read": error_read,
            "stdout_error": output_error, "stderr_error": error_code,
            "bounded": bounded, "frame_count": frames,
        });
        println!("\nHiRouteConsoleDiagnostic/v1 {summary}");
    }
    assert!(
        natural,
        "console fixture must exit naturally with an empty held Job"
    );
    assert_eq!(code, Some(0), "console fixture root must exit successfully");
    assert!(
        output_read && error_read && bounded,
        "console pipes must reach bounded EOF"
    );
    assert!(
        error.is_empty(),
        "console fixture cannot hide native failure output"
    );
    assert_eq!(frames, Some(1), "exact fixture frame is required");
}

#[test]
fn gui_request_without_a_parent_console_preserves_pipes_and_creates_no_console() {
    assert!(
        !console::is_attached().unwrap(),
        "the actual GUI test executable starts detached"
    );
    run("no-parent", false);
}

#[test]
fn gui_attach_preserves_inherited_pipes_with_a_real_parent_console() {
    assert!(!console::is_attached().unwrap());
    run("isolated-host", true);
}

#[test]
#[ignore = "child fixture selected only by the two real native console gates"]
fn console_fixture() {
    let mode = std::env::var(MODE).expect("explicit native console fixture mode required");
    if mode == "isolated-host" {
        assert!(observe(Stage::HostBeforeAttached));
        run("with-parent", false);
        observe(Stage::HostAfterChild);
    } else {
        assert!(matches!(mode.as_str(), "no-parent" | "with-parent"));
        assert!(
            !observe(if mode == "with-parent" {
                Stage::WithParentBeforeAttach
            } else {
                Stage::NoParentBeforeAttach
            }),
            "ordinary GUI launch cannot allocate a console"
        );
        let standards = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
        // SAFETY: observe only this fixture's current-process standard handles.
        let before = standards.map(|name| unsafe { GetStdHandle(name) });
        for handle in before {
            assert_eq!(unsafe { GetFileType(handle) }, FILE_TYPE_PIPE);
        }
        let attachment = console::attach_parent_preserving_stdio();
        emit_facts(
            Stage::AfterAttach,
            console::is_attached().ok(),
            attachment.as_ref().err(),
            attachment.as_ref().ok().map(|value| match value {
                ParentConsoleAttachment::Attached => 0,
                ParentConsoleAttachment::AlreadyAttached => 1,
                ParentConsoleAttachment::ParentHasNoConsole => 2,
            }),
            None,
            None,
        );
        let attachment = attachment.unwrap();
        assert_eq!(
            attachment,
            if mode == "with-parent" {
                ParentConsoleAttachment::Attached
            } else {
                ParentConsoleAttachment::ParentHasNoConsole
            }
        );
        assert_eq!(console::is_attached().unwrap(), mode == "with-parent");
        let after = standards.map(|name| unsafe { GetStdHandle(name) });
        emit_facts(
            Stage::HandlesChecked,
            console::is_attached().ok(),
            None,
            None,
            Some(before == after),
            None,
        );
        assert_eq!(before, after);
    }
    observe(Stage::InputBeforeRead);
    let mut input = Vec::new();
    let read = std::io::stdin().take(65).read_to_end(&mut input);
    emit_facts(
        Stage::InputAfterRead,
        console::is_attached().ok(),
        read.as_ref().err(),
        None,
        None,
        Some(input == b"literal-pipe-proof"),
    );
    read.unwrap();
    assert_eq!(input, b"literal-pipe-proof");
    println!("\n{FRAME}");
    std::io::stdout().flush().unwrap();
    observe(Stage::NewlineFrameWritten);
}
