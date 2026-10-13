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
    use std::os::windows::process::CommandExt;
    hiroute_platform::windows::job::initialize_resident().unwrap();
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
    let stdout_path = directory.join("helper.stdout");
    let stderr_path = directory.join("helper.stderr");
    let stdout = std::fs::File::create(&stdout_path).unwrap();
    let stderr = std::fs::File::create(&stderr_path).unwrap();
    // Match node:child_process.exec's Windows /d /s /c wrapping and verbatim command.
    let mut child = std::process::Command::new(cmd)
        .raw_arg(format!("/d /s /c \"{command}\""))
        .env_clear()
        .envs(environment("argv", &directory))
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if started.elapsed() >= Duration::from_secs(5) {
            break None;
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
        String::from_utf8_lossy(&bytes).into_owned()
    };
    if status.is_none_or(|status| !status.success()) {
        let native_entered = directory.join("child.started").exists();
        let receipt_written = directory.join("argv.json").exists();
        let elapsed_ms = started.elapsed().as_millis();
        let descendants = windows_helper_diagnostics::descendants(child.id());
        let _ = child.kill();
        let _ = child.wait();
        let powershell =
            windows_helper_diagnostics::post_failure_powershell(&environment("argv", &directory));
        panic!(
            "managed helper phase: cmd_status={status:?}; elapsed_ms={elapsed_ms}; native_fixture_entered={native_entered}; receipt_written={receipt_written}; descendants={descendants}; post_failure_system_powershell={powershell}; stdout={:?}; stderr={:?}",
            read_log(&stdout_path),
            read_log(&stderr_path)
        );
    }
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
