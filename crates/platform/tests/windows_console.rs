#![cfg(windows)]
#![windows_subsystem = "windows"]
//! Actual GUI-process attachment with explicit pipes, never the runner's console.
use std::ffi::OsString;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use hiroute_platform::windows::console::{self, ParentConsoleAttachment};
use hiroute_platform::windows::job::{Child, Launch, Stream};
use windows_sys::Win32::Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};

const MODE: &str = "HIROUTE_NATIVE_CONSOLE_FIXTURE";
const FRAME: &str = "HiRouteConsoleFixture/v1";

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
            Read::by_ref(&mut file)
                .take(16 * 1024 + 1)
                .read_to_end(&mut bytes)
                .unwrap();
            assert!(bytes.len() <= 16 * 1024);
            bytes
        })
    };
    let output = read(stdout);
    let error = read(stderr);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(code) = child.exit_code().unwrap() {
            assert_eq!(code, 0);
            if child.empty().unwrap() {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "console fixture must exit naturally with an empty held Job"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = output.join().unwrap();
    let error = error.join().unwrap();
    assert!(
        error.is_empty(),
        "console fixture cannot hide native failure output"
    );
    let output = std::str::from_utf8(&output).unwrap();
    assert_eq!(output.lines().filter(|line| *line == FRAME).count(), 1);
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
        assert!(console::is_attached().unwrap());
        run("with-parent", false);
    } else {
        assert!(matches!(mode.as_str(), "no-parent" | "with-parent"));
        assert!(
            !console::is_attached().unwrap(),
            "ordinary GUI launch cannot allocate a console"
        );
        let standards = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
        // SAFETY: observe only this fixture's current-process standard handles.
        let before = standards.map(|name| unsafe { GetStdHandle(name) });
        for handle in before {
            assert_eq!(unsafe { GetFileType(handle) }, FILE_TYPE_PIPE);
        }
        let attachment = console::attach_parent_preserving_stdio().unwrap();
        assert_eq!(
            attachment,
            if mode == "with-parent" {
                ParentConsoleAttachment::Attached
            } else {
                ParentConsoleAttachment::ParentHasNoConsole
            }
        );
        assert_eq!(console::is_attached().unwrap(), mode == "with-parent");
        assert_eq!(before, standards.map(|name| unsafe { GetStdHandle(name) }));
    }
    let mut input = Vec::new();
    std::io::stdin().take(65).read_to_end(&mut input).unwrap();
    assert_eq!(input, b"literal-pipe-proof");
    println!("\n{FRAME}");
    std::io::stdout().flush().unwrap();
}
