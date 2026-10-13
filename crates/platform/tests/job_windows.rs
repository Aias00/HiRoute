#![cfg(windows)]

use hiroute_platform::windows::{
    job::{Child, Launch, Stream},
    launcher,
};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn await_condition(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Windows process fixture deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn environment(mode: &str, root: &Path) -> Vec<(OsString, OsString)> {
    let mut env = vec![
        ("HIROUTE_JOB_MODE".into(), mode.into()),
        ("HIROUTE_JOB_ROOT".into(), root.as_os_str().to_owned()),
    ];
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            env.push((name.into(), value));
        }
    }
    env
}
fn spawn(mode: &str, root: &Path) -> Child {
    let binary = std::env::current_exe().unwrap();
    let args = ["--exact", "job_probe", "--ignored", "--nocapture"].map(OsString::from);
    let env = environment(mode, root);
    Child::spawn(Launch {
        program: &binary,
        args: &args,
        env: &env,
        cwd: Some(root),
        stdin: Stream::Null,
        stdout: Stream::Null,
        stderr: Stream::Null,
    })
    .unwrap()
}
fn stop(child: &Child) {
    child.terminate().unwrap();
    await_condition(|| child.empty().unwrap() && child.exit_code().unwrap().is_some());
}

// Native no-model fixture. It records kernel Job membership before it starts a tool.
#[test]
#[ignore = "native child fixture selected only by the production Job launcher"]
fn job_probe() {
    let mode = std::env::var("HIROUTE_JOB_MODE").unwrap();
    let root = PathBuf::from(std::env::var_os("HIROUTE_JOB_ROOT").unwrap());
    if mode == "argv" {
        let record = serde_json::json!({"binary": std::env::current_exe().unwrap(), "args": std::env::args_os().skip(1).map(|arg| arg.into_string().unwrap()).collect::<Vec<_>>()});
        std::fs::write(root.join("argv.json"), serde_json::to_vec(&record).unwrap()).unwrap();
        return;
    }
    let mut assigned = 0;
    assert_ne!(
        unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                std::ptr::null_mut(),
                &mut assigned,
            )
        },
        0
    );
    assert_ne!(
        assigned, 0,
        "process must enter its Job before fixture execution"
    );
    std::fs::write(root.join("assigned"), "yes").unwrap();
    if mode == "tree" {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "job_probe", "--ignored", "--nocapture"])
            .env_clear()
            .envs(environment("heartbeat", &root))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::fs::write(root.join("tool.pid"), child.id().to_string()).unwrap();
        await_condition(|| root.join("heartbeat").exists());
        // Leader exits while its ordinary tool remains in the inherited Job.
        let _ = &mut child;
        return;
    }
    let mut heartbeat = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("heartbeat"))
        .unwrap();
    loop {
        writeln!(heartbeat, "tick").unwrap();
        heartbeat.flush().unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn job_stops_surviving_tool_after_leader_exit_and_preserves_neighbor() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let neighbor = root.path().join("neighbor");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&neighbor).unwrap();
    let tree = spawn("tree", &first);
    let adjacent = spawn("heartbeat", &neighbor);
    await_condition(|| {
        tree.exit_code().unwrap().is_some()
            && first.join("heartbeat").exists()
            && neighbor.join("heartbeat").exists()
    });
    assert!(!tree.empty().unwrap());
    assert!(first.join("assigned").exists());
    stop(&tree);
    let before = std::fs::metadata(first.join("heartbeat")).unwrap().len();
    let neighbor_before = std::fs::metadata(neighbor.join("heartbeat")).unwrap().len();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        before,
        std::fs::metadata(first.join("heartbeat")).unwrap().len()
    );
    assert!(std::fs::metadata(neighbor.join("heartbeat")).unwrap().len() > neighbor_before);
    assert!(adjacent.exit_code().unwrap().is_none());
    stop(&adjacent);
}

#[test]
fn native_job_argv_preserves_spaces_quotes_unicode_and_shell_characters() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("路径 空格 % ! & ' (literal)");
    std::fs::create_dir(&directory).unwrap();
    let binary = directory.join("fixture % ! & '.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &binary).unwrap();
    let literal = "空 格 %PATH% !bang! & | ; ' \"quote\" tail\\";
    let args = [
        "--exact",
        "job_probe",
        "--ignored",
        "--nocapture",
        "--skip",
        literal,
    ]
    .map(OsString::from);
    let env = environment("argv", &directory);
    let child = Child::spawn(Launch {
        program: &binary,
        args: &args,
        env: &env,
        cwd: Some(&directory),
        stdin: Stream::Null,
        stdout: Stream::Null,
        stderr: Stream::Null,
    })
    .unwrap();
    await_condition(|| child.exit_code().unwrap().is_some());
    assert_eq!(child.exit_code().unwrap(), Some(0));
    stop(&child);
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("argv.json")).unwrap()).unwrap();
    assert_eq!(record["args"].as_array().unwrap().last().unwrap(), literal);
}

#[test]
fn managed_helper_executes_from_cmd_with_literal_special_character_path_and_argv() {
    use std::os::windows::process::CommandExt;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("helper 路径 % ! & ' 空格");
    std::fs::create_dir(&directory).unwrap();
    let binary = directory.join("helper % ! & '.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &binary).unwrap();
    let literal = "literal %PATH% !bang! & | ' Unicode 空 格";
    let args = [
        "--exact",
        "job_probe",
        "--ignored",
        "--nocapture",
        "--skip",
        literal,
    ]
    .map(String::from);
    let command = launcher::managed_helper_command(&binary, &args).unwrap();
    assert!(!command.contains(literal));
    let cmd = launcher::system_executable(Path::new("cmd.exe")).unwrap();
    // Match node:child_process.exec's Windows /d /s /c wrapping and verbatim command.
    let mut child = std::process::Command::new(cmd)
        .raw_arg(format!("/d /s /c \"{command}\""))
        .env_clear()
        .envs(environment("argv", &directory))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    await_condition(|| child.try_wait().unwrap().is_some());
    assert!(child.wait().unwrap().success());
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("argv.json")).unwrap()).unwrap();
    assert_eq!(record["args"].as_array().unwrap().last().unwrap(), literal);
}

#[test]
fn duplicate_windows_environment_keys_are_rejected_before_launch() {
    let binary = std::env::current_exe().unwrap();
    let env = [("Path".into(), "one".into()), ("PATH".into(), "two".into())];
    assert!(
        Child::spawn(Launch {
            program: &binary,
            args: &[],
            env: &env,
            cwd: None,
            stdin: Stream::Null,
            stdout: Stream::Null,
            stderr: Stream::Null
        })
        .is_err()
    );
}
