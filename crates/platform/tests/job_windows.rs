#![cfg(windows)]

#[path = "support/windows_helper_diagnostics.rs"]
mod windows_helper_diagnostics;

use hiroute_platform::windows::{
    job::{
        Child, Launch, Stream, credential_pipe_pair, duplicate_inheritable_pipe,
        read_inherited_credential,
    },
    launcher,
};
use std::os::windows::io::AsRawHandle;
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

const MANAGED_HELPER_LITERAL: &str = "literal %PATH% !bang! & | ' Unicode 空 格";

fn managed_helper_probe_args() -> [String; 6] {
    [
        "--exact",
        "job_probe",
        "--ignored",
        "--nocapture",
        "--skip",
        MANAGED_HELPER_LITERAL,
    ]
    .map(String::from)
}

// This driver enters an atomic held Job before creating cmd. Keep the actual
// node:child_process.exec wrapping verbatim; ordinary argv quoting is different.
fn managed_helper_driver(root: &Path) {
    use std::os::windows::process::CommandExt;
    let binary = root.join("helper % ! & '.exe");
    let command = launcher::managed_helper_command(&binary, &managed_helper_probe_args()).unwrap();
    assert!(!command.contains(MANAGED_HELPER_LITERAL));
    let cmd = launcher::system_executable(Path::new("cmd.exe")).unwrap();
    let status = std::process::Command::new(cmd)
        .raw_arg(format!("/d /s /c \"{command}\""))
        .env_clear()
        .envs(environment("argv", root))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .unwrap();
    std::fs::write(
        root.join("cmd.exited.json"),
        serde_json::to_vec(&serde_json::json!({"exit_code": status.code()})).unwrap(),
    )
    .unwrap();
    assert!(status.success(), "managed helper cmd exit");
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
    if mode == "managed-helper-driver" {
        managed_helper_driver(&root);
        return;
    }
    if mode == "defaults" {
        hiroute_platform::windows::security::initialize_current_process_private_defaults().unwrap();
        std::fs::write(root.join("defaults.receipt"), "user-owned").unwrap();
        return;
    }
    if mode == "credential" {
        let handle: u64 = std::env::args().last().unwrap().parse().unwrap();
        let credential = read_inherited_credential(handle, 256, Duration::from_secs(2)).unwrap();
        assert_eq!(&credential[..], b"public-test-carrier\n");
        // The fixture has reached the real child reader, not merely process creation.
        std::fs::write(root.join("credential.receipt"), "verified").unwrap();
        return;
    }
    if mode == "argv" {
        std::fs::write(root.join("child.started"), "native-fixture-entered").unwrap();
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
    hiroute_platform::windows::security::create_owner_only_directory(&directory).unwrap();
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
    let parent_defaults = token_defaults();
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
    assert_eq!(
        token_defaults(),
        parent_defaults,
        "child defaults must not mutate parent token"
    );
    await_condition(|| child.exit_code().unwrap().is_some());
    assert_eq!(child.exit_code().unwrap(), Some(0));
    stop(&child);
    let parent = hiroute_platform::windows::security::open_owner_only(&directory, true).unwrap();
    for name in ["child.started", "argv.json"] {
        let file = hiroute_platform::windows::security::open_inherited_private_file_at(
            &parent,
            name.as_ref(),
        )
        .unwrap();
        assert!(
            hiroute_platform::windows::security::verify_owner_only_handle(&file, false).is_err(),
            "std-created file retains inherited DACL; the strict contract must remain distinct"
        );
    }
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("argv.json")).unwrap()).unwrap();
    assert_eq!(record["args"].as_array().unwrap().last().unwrap(), literal);
    // Exercise the daemon's initialization API from an ordinary CreateProcessW
    // child, independently of the Job launcher's token-default initialization.
    let mut ordinary = std::process::Command::new(&binary)
        .args(["--exact", "job_probe", "--ignored", "--nocapture"])
        .env_clear()
        .envs(environment("defaults", &directory))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = ordinary.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            ordinary.kill().unwrap();
            ordinary.wait().unwrap();
            panic!("private defaults initialization deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(status.success());
    assert_eq!(
        token_defaults(),
        parent_defaults,
        "self initialization must not mutate parent shell token"
    );
    hiroute_platform::windows::security::open_inherited_private_file_at(
        &parent,
        "defaults.receipt".as_ref(),
    )
    .unwrap();
}

fn token_defaults() -> (Vec<u8>, Vec<u8>) {
    use windows_sys::Win32::Security::{
        GetLengthSid, GetTokenInformation, TOKEN_DEFAULT_DACL, TOKEN_OWNER, TOKEN_QUERY,
        TokenDefaultDacl, TokenOwner,
    };
    let mut token = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            windows_sys::Win32::System::Threading::OpenProcessToken(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                TOKEN_QUERY,
                &mut token,
            )
        },
        0
    );
    let read = |class| {
        let mut size = 0;
        unsafe {
            GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut size);
        }
        assert!((1..65536).contains(&size));
        let mut words = vec![0_usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        assert_ne!(
            unsafe {
                GetTokenInformation(token, class, words.as_mut_ptr().cast(), size, &mut size)
            },
            0
        );
        words
    };
    let owner = read(TokenOwner);
    let dacl = read(TokenDefaultDacl);
    let result = unsafe {
        let owner = (*owner.as_ptr().cast::<TOKEN_OWNER>()).Owner;
        let acl = (*dacl.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl;
        assert!(!owner.is_null() && !acl.is_null());
        (
            std::slice::from_raw_parts(owner.cast::<u8>(), GetLengthSid(owner) as usize).to_vec(),
            std::slice::from_raw_parts(acl.cast::<u8>(), (*acl).AclSize as usize).to_vec(),
        )
    };
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(token);
    }
    result
}

#[test]
fn managed_helper_executes_from_cmd_with_literal_special_character_path_and_argv() {
    hiroute_platform::windows::job::initialize_resident().unwrap();
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("helper 路径 % ! & ' 空格");
    std::fs::create_dir(&directory).unwrap();
    let binary = directory.join("helper % ! & '.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &binary).unwrap();
    let args = managed_helper_probe_args();
    let stdout_path = directory.join("helper.stdout");
    let stderr_path = directory.join("helper.stderr");
    let stdout = std::fs::File::create(&stdout_path).unwrap();
    let stderr = std::fs::File::create(&stderr_path).unwrap();
    let driver = std::env::current_exe().unwrap();
    let driver_args = ["--exact", "job_probe", "--ignored", "--nocapture"].map(OsString::from);
    let env = environment("managed-helper-driver", &directory);
    let started = Instant::now();
    let child = Child::spawn(Launch {
        program: &driver,
        args: &driver_args,
        env: &env,
        cwd: Some(&directory),
        stdin: Stream::Null,
        stdout: Stream::File(&stdout),
        stderr: Stream::File(&stderr),
    })
    .unwrap();
    // These are platform-independent TEST budgets, not a changed production
    // helper timeout or an OS startup SLA. Win11 evidence placed system
    // PowerShell startup at the former whole-chain five-second bound.
    // Do not warm it first. Once the actual argv child writes its independent
    // entry marker, retain the original five seconds for execution AND exit.
    const COLD_ENTRY_BUDGET: Duration = Duration::from_secs(30);
    const NATIVE_COMPLETION_BUDGET: Duration = Duration::from_secs(5);
    let absolute_deadline = started + COLD_ENTRY_BUDGET + NATIVE_COMPLETION_BUDGET;
    let mut entered_at = None;
    let failure = loop {
        let now = Instant::now();
        if entered_at.is_none() && directory.join("child.started").exists() {
            entered_at = Some(now);
        }
        let exit_code = child.exit_code().unwrap();
        if let Some(code) = exit_code {
            if code != 0 {
                break Some("driver_exit");
            }
            if child.empty().unwrap() {
                break entered_at.is_none().then_some("exit_without_native_entry");
            }
        }
        let phase_deadline = entered_at.map_or(started + COLD_ENTRY_BUDGET, |entered| {
            entered + NATIVE_COMPLETION_BUDGET
        });
        if now >= phase_deadline || now >= absolute_deadline {
            break Some(if entered_at.is_some() {
                "native_completion_deadline"
            } else {
                "cold_entry_deadline"
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let read_log = |path: &Path| {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .take(8192)
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    if let Some(phase) = failure {
        let entry_at_first_failure_sample = directory.join("child.started").exists();
        let receipt_at_first_failure_sample = directory.join("argv.json").exists();
        let elapsed_ms = started.elapsed().as_millis();
        let descendants = windows_helper_diagnostics::descendants(child.id());
        child.terminate().unwrap();
        let cleanup_deadline = Instant::now() + Duration::from_secs(5);
        while !child.empty().unwrap() || child.exit_code().unwrap().is_none() {
            assert!(Instant::now() < cleanup_deadline, "held helper Job cleanup");
            std::thread::sleep(Duration::from_millis(10));
        }
        // Snapshotting is not atomic with child completion. Record the last
        // markers too, rather than falsely claiming it never reached the child.
        let entry_after_cleanup = directory.join("child.started").exists();
        let receipt_after_cleanup = directory.join("argv.json").exists();
        let stdout = read_log(&stdout_path);
        let stderr = read_log(&stderr_path);
        let stdout_contains_probe_success = stdout
            .windows(b"test job_probe ... ok".len())
            .any(|bytes| bytes == b"test job_probe ... ok");
        let powershell =
            windows_helper_diagnostics::post_failure_powershell(&environment("argv", &directory));
        panic!(
            "managed helper phase={phase}; elapsed_ms={elapsed_ms}; entry_at_first_failure_sample={entry_at_first_failure_sample}; receipt_at_first_failure_sample={receipt_at_first_failure_sample}; entry_after_cleanup={entry_after_cleanup}; receipt_after_cleanup={receipt_after_cleanup}; stdout_contains_probe_success={stdout_contains_probe_success}; driver_exit={:?}; job_empty={}; descendants={descendants}; post_failure_system_powershell={powershell}; stdout_bytes={}; stderr_bytes={}",
            child.exit_code().unwrap(),
            child.empty().unwrap(),
            stdout.len(),
            stderr.len()
        );
    }
    let completed_at = Instant::now();
    let entered_at = entered_at.unwrap();
    eprintln!(
        "managed helper phases: cold_entry_ms={}; native_completion_ms={}; total_ms={}; driver_exit={:?}; job_empty={}",
        entered_at.duration_since(started).as_millis(),
        completed_at.duration_since(entered_at).as_millis(),
        completed_at.duration_since(started).as_millis(),
        child.exit_code().unwrap(),
        child.empty().unwrap()
    );
    assert!(entered_at.duration_since(started) < COLD_ENTRY_BUDGET);
    assert!(completed_at.duration_since(entered_at) < NATIVE_COMPLETION_BUDGET);
    assert!(completed_at < absolute_deadline);
    assert_eq!(child.exit_code().unwrap(), Some(0));
    assert!(
        child.empty().unwrap(),
        "helper Job must contain no survivors"
    );
    let cmd_receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("cmd.exited.json")).unwrap()).unwrap();
    assert_eq!(cmd_receipt, serde_json::json!({"exit_code": 0}));
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("argv.json")).unwrap()).unwrap();
    assert_eq!(record["binary"], serde_json::to_value(&binary).unwrap());
    assert_eq!(record["args"], serde_json::to_value(args).unwrap());
    assert_eq!(
        record["args"].as_array().unwrap().last().unwrap(),
        MANAGED_HELPER_LITERAL
    );
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

fn inherited(file: &std::fs::File) -> u64 {
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    assert_ne!(
        unsafe {
            SetHandleInformation(
                file.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        },
        0
    );
    file.as_raw_handle() as usize as u64
}

#[test]
fn inherited_credential_pipe_roundtrips_and_preserves_original_handle() {
    use windows_sys::Win32::Foundation::GetHandleInformation;
    let (read, mut write) = credential_pipe_pair().unwrap();
    let handle = inherited(&read);
    // No secret or model invocation: exercise actual native pipe transport and EOF.
    write.write_all(b"fixture-public-credential\n").unwrap();
    drop(write);
    let bytes = read_inherited_credential(handle, 256, Duration::from_secs(2)).unwrap();
    assert_eq!(&bytes[..], b"fixture-public-credential\n");
    let mut flags = 0;
    assert_ne!(
        unsafe { GetHandleInformation(read.as_raw_handle(), &mut flags) },
        0
    );
}

#[test]
fn inherited_credential_rejects_invalid_stdio_file_and_noninherited_handles() {
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_HANDLE, INVALID_HANDLE_VALUE},
        System::Console::{GetStdHandle, STD_INPUT_HANDLE},
    };
    for handle in
        [
            0,
            INVALID_HANDLE_VALUE as usize as u64,
            unsafe { GetStdHandle(STD_INPUT_HANDLE) } as usize as u64,
        ]
    {
        assert!(read_inherited_credential(handle, 256, Duration::from_secs(1)).is_err());
    }
    let file = tempfile::tempfile().unwrap();
    assert!(read_inherited_credential(inherited(&file), 256, Duration::from_secs(1)).is_err());
    let (read, _write) = credential_pipe_pair().unwrap();
    assert!(
        read_inherited_credential(
            read.as_raw_handle() as usize as u64,
            256,
            Duration::from_secs(1)
        )
        .is_err()
    );
    // A fabricated high pointer must fail validation before any borrowed ownership is taken.
    let error = read_inherited_credential(0x1_0000_0001, 256, Duration::from_secs(1)).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_HANDLE as i32));
}

#[test]
fn inherited_credential_pipe_timeout_and_byte_limit_are_bounded() {
    let (read, mut write) = credential_pipe_pair().unwrap();
    let handle = inherited(&read);
    let start = Instant::now();
    let error = read_inherited_credential(handle, 256, Duration::from_millis(50)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(1));
    write.write_all(&[b'x'; 257]).unwrap();
    let error = read_inherited_credential(handle, 256, Duration::from_secs(1)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(read_inherited_credential(handle, 1024 * 1024 + 1, Duration::from_secs(1)).is_err());
    assert!(read_inherited_credential(handle, 256, Duration::from_secs(31)).is_err());
}

#[test]
fn inherited_credential_crosses_an_explicit_job_child_handle_list() {
    let root = tempfile::tempdir().unwrap();
    let (read, mut write) = credential_pipe_pair().unwrap();
    let carrier = duplicate_inheritable_pipe(&read).unwrap();
    let binary = std::env::current_exe().unwrap();
    let handle = (carrier.as_raw_handle() as usize as u64).to_string();
    let args = [
        "--exact",
        "job_probe",
        "--ignored",
        "--nocapture",
        "--skip",
        &handle,
    ]
    .map(OsString::from);
    let env = environment("credential", root.path());
    let child = Child::spawn_with_inherited_pipes(
        Launch {
            program: &binary,
            args: &args,
            env: &env,
            cwd: Some(root.path()),
            stdin: Stream::Null,
            stdout: Stream::Null,
            stderr: Stream::Null,
        },
        &[&carrier],
    )
    .unwrap();
    write.write_all(b"public-test-carrier\n").unwrap();
    drop(write);
    await_condition(|| child.exit_code().unwrap().is_some());
    assert_eq!(child.exit_code().unwrap(), Some(0));
    assert_eq!(
        std::fs::read_to_string(root.path().join("credential.receipt")).unwrap(),
        "verified"
    );
    stop(&child);
}
