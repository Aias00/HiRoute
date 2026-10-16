//! Native test process/private carrier owner. Public business requests remain in its caller.
use super::oracle;
use hiroute_host_runtime::StandaloneLayout;
use hiroute_platform::windows::{
    job::{self, Child, Launch, Stream},
    security,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;
const BOUND: u64 = 1024 * 1024;
static PRIVATE_TOKENS: Mutex<Vec<Zeroizing<Vec<u8>>>> = Mutex::new(Vec::new());

pub(in super::super) fn assert_no_secret(bytes: &[u8]) {
    oracle::assert_no_secret(bytes);
    for token in PRIVATE_TOKENS.lock().unwrap().iter() {
        assert!(
            !bytes
                .windows(token.len())
                .any(|part| part == token.as_slice()),
            "a private Agent grant reached a public body or capture"
        );
    }
}
pub(super) fn stage_codex(layout: &StandaloneLayout, source: &Path, expected: &str) {
    assert!(source.is_absolute());
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    super::super::windows_execution_host::image(source);
    let mut file = security::open_identity(source, false).unwrap();
    let identity = security::file_identity(&file).unwrap();
    let before = file.metadata().unwrap();
    assert!(before.is_file() && before.len() > 0 && before.len() <= 512 * 1024 * 1024);
    let mut hash = Sha256::new();
    let mut buffer = Zeroizing::new(vec![0u8; 64 * 1024]);
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    assert_eq!(
        format!("{:x}", hash.finalize()),
        expected,
        "actual pinned Codex source hash differs before staging"
    );
    // Resolve in the production candidate order, including the first matching
    // PATH directory. A later npm shim is not selected when the verified native
    // image precedes it. Never accept a selected wrapper or version-only match.
    'lookup: for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        for leaf in ["codex.exe", "codex.cmd", "codex"] {
            let candidate = directory.join(leaf);
            if !candidate.is_file() {
                continue;
            }
            assert_eq!(
                leaf, "codex.exe",
                "upgrade discovery cannot select an unverified wrapper"
            );
            let mut image = security::open_identity(&candidate, false).unwrap();
            let mut hash = Sha256::new();
            loop {
                let count = image.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            assert_eq!(
                format!("{:x}", hash.finalize()),
                expected,
                "upgrade discovery PATH exposes a different native Codex producer"
            );
            break 'lookup;
        }
    }
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).unwrap();
    let destination = layout.home.join(".local/bin/codex.exe");
    if destination.exists() {
        let mut existing = security::open_identity(&destination, false).unwrap();
        security::verify_owner_only_handle(&existing, false).unwrap();
        let mut hash = Sha256::new();
        loop {
            let count = existing.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        assert_eq!(
            format!("{:x}", hash.finalize()),
            expected,
            "upgrade replay cannot replace a different external engine"
        );
        super::super::windows_execution_host::image(&destination);
        return;
    }
    let mut stage = security::create_owner_only_file(&destination).unwrap();
    let mut copied = 0;
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        stage.write_all(&buffer[..count]).unwrap();
        copied += count as u64;
    }
    assert_eq!(copied, before.len());
    let after = file.metadata().unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(security::file_identity(&file).unwrap(), identity);
    stage.sync_all().unwrap();
    security::verify_owner_only_handle(&stage, false).unwrap();
    stage.seek(SeekFrom::Start(0)).unwrap();
    let mut hash = Sha256::new();
    loop {
        let count = stage.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    assert_eq!(format!("{:x}", hash.finalize()), expected);
    drop(stage);
    super::super::windows_execution_host::image(&destination);
}
pub(super) fn read(file: File) -> Zeroizing<Vec<u8>> {
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(BOUND + 1).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() as u64 <= BOUND);
    bytes
}
fn finish(
    mut child: Child,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> (i32, Zeroizing<Vec<u8>>, Zeroizing<Vec<u8>>) {
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let output = thread::spawn(move || read(stdout));
    let error = thread::spawn(move || read(stderr));
    let writer = input.map(|bytes| {
        let mut stdin = child.stdin.take().unwrap();
        thread::spawn(move || {
            stdin.write_all(&bytes).unwrap();
        })
    });
    drop(child.stdin.take());
    let deadline = Instant::now() + timeout;
    let exit = loop {
        if let Some(exit) = child.exit_code().unwrap() {
            break exit;
        }
        if Instant::now() >= deadline {
            child.terminate().unwrap();
            panic!("upgrade native CLI exceeded its bounded deadline");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let drained = Instant::now() + Duration::from_secs(5);
    while !child.empty().unwrap() {
        if Instant::now() >= drained {
            child.terminate().unwrap();
            panic!("upgrade native CLI left a live descendant");
        }
        thread::sleep(Duration::from_millis(10));
    }
    if let Some(writer) = writer {
        writer.join().unwrap();
    }
    (exit, output.join().unwrap(), error.join().unwrap())
}
pub(super) fn execute(
    program: &Path,
    args: &[OsString],
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> (i32, Zeroizing<Vec<u8>>, Zeroizing<Vec<u8>>) {
    let environment: Vec<_> = std::env::vars_os().collect();
    assert!(
        args.iter()
            .all(|value| !value.to_string_lossy().contains(oracle::SECRET))
    );
    assert!(
        environment
            .iter()
            .all(|(_, value)| !value.to_string_lossy().contains(oracle::SECRET))
    );
    let child = Child::spawn(Launch {
        program,
        args,
        env: &environment,
        cwd: program.parent(),
        stdin: if input.is_some() {
            Stream::Piped
        } else {
            Stream::Null
        },
        stdout: Stream::Piped,
        stderr: Stream::Piped,
    })
    .unwrap();
    finish(child, input, timeout)
}
pub(super) fn cli(program: &Path, args: &[&str], payload: Option<&Value>) -> Value {
    let mut arguments: Vec<OsString> = args.iter().map(OsString::from).collect();
    let input = payload.map(|payload| {
        let bytes = serde_json::to_vec(payload).unwrap();
        assert!(bytes.len() <= 65536);
        assert_no_secret(&bytes);
        arguments.push("--request-stdin".into());
        bytes
    });
    arguments.extend(["--output", "json", "--timeout", "45s"].map(OsString::from));
    let (exit, output, error) = execute(program, &arguments, input, Duration::from_secs(60));
    assert_no_secret(&output);
    assert_no_secret(&error);
    if exit != 0 {
        let stage = match args {
            ["agents", "restore", "preview"] => "restore-preview",
            ["agents", "restore", "apply"] => "restore-apply",
            ["agents", "connect", "status", ..] => "connect-status",
            ["agents", "connect", ..] => "connect-change",
            ["operations", "get", ..] => "operation-read",
            ["compute", ..] => "compute",
            ["routing", ..] => "routing",
            ["sessions", ..] => "receipts",
            _ => "other",
        };
        eprintln!("upgrade public CLI failure: stage={stage}, exit={exit}");
    }
    assert_eq!(exit, 0, "upgrade public CLI failed its real entry");
    assert!(error.is_empty(), "upgrade public CLI emitted diagnostics");
    let value: Value =
        serde_json::from_slice(&output).expect("upgrade public CLI envelope invalid");
    assert!(
        matches!(value["status"].as_str(), Some("succeeded" | "accepted")),
        "upgrade public CLI did not accept its request"
    );
    value
}
pub(super) fn register(program: &Path, candidate: &str) {
    let (read, mut write) = job::credential_pipe_pair().unwrap();
    let carrier = job::duplicate_inheritable_pipe(&read).unwrap();
    let args = [
        "protected-input".into(),
        "register".into(),
        "--candidate".into(),
        candidate.into(),
        "--secret-handle".into(),
        (carrier.as_raw_handle() as usize as u64).to_string().into(),
        "--output".into(),
        "json".into(),
        "--timeout".into(),
        "15s".into(),
    ];
    let environment: Vec<_> = std::env::vars_os().collect();
    let child = Child::spawn_with_inherited_pipes(
        Launch {
            program,
            args: &args,
            env: &environment,
            cwd: program.parent(),
            stdin: Stream::Null,
            stdout: Stream::Piped,
            stderr: Stream::Piped,
        },
        &[&carrier],
    )
    .unwrap();
    write.write_all(oracle::SECRET.as_bytes()).unwrap();
    drop(write);
    drop(carrier);
    drop(read);
    let (exit, output, error) = finish(child, None, Duration::from_secs(20));
    assert_no_secret(&output);
    assert_no_secret(&error);
    assert_eq!(exit, 0, "actual protected-input inherited carrier failed");
    assert!(error.is_empty());
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["status"], "succeeded");
    assert_eq!(value["data"]["registered"], true);
}

pub(super) fn grant(program: &Path, context: &str) -> Zeroizing<String> {
    let args = [
        OsString::from(hiroute_application_api::HIDDEN_AGENT_GRANT_HELPER_VERB_V1),
        OsString::from(format!("agent-connection/{context}")),
    ];
    let (exit, output, error) = execute(program, &args, None, Duration::from_secs(20));
    assert_eq!(
        exit, 0,
        "actual native Agent grant helper did not grant the published fixed route"
    );
    assert_no_secret(&error);
    assert!(error.is_empty());
    let token = Zeroizing::new(std::str::from_utf8(&output).unwrap().trim().to_owned());
    assert!(!token.is_empty() && token.len() <= 65536 && !token.contains(['\r', '\n']));
    PRIVATE_TOKENS
        .lock()
        .unwrap()
        .push(Zeroizing::new(token.as_bytes().to_vec()));
    token
}
