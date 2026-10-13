//! A self-reexecuted native owner process drives actual shipped CLI/Python entries.
//! The fixture accepts no credentials, SIDs, or destructive numeric PID locators.
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use hiroute_host_runtime::StandaloneLayout;
use hiroute_platform::windows::{
    background,
    job::{Child, Launch, Stream},
    lifecycle,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::windows_account as account;
use super::windows_token_launcher as launcher;

pub const PROBE: &str = "HIROUTE_WINDOWS_BACKGROUND_FIXTURE";
pub const ENTRY: &str = "windows_background_owner_fixture";
const OUTPUT: &str = "HIROUTE_NATIVE_BACKGROUND_RESULT=";
const BOUND: usize = 256 * 1024;

enum Process {
    Product(Child),
    Owner(launcher::Child),
}
impl Process {
    fn pipes(&mut self) -> (Option<File>, File, File) {
        match self {
            Self::Product(child) => (
                child.stdin.take(),
                child.stdout.take().unwrap(),
                child.stderr.take().unwrap(),
            ),
            Self::Owner(child) => (
                child.stdin.take(),
                child.stdout.take().unwrap(),
                child.stderr.take().unwrap(),
            ),
        }
    }
    fn exit_code(&self) -> std::io::Result<Option<i32>> {
        match self {
            Self::Product(child) => child.exit_code(),
            Self::Owner(child) => child.exit_code(),
        }
    }
    fn empty(&self) -> std::io::Result<bool> {
        match self {
            Self::Product(child) => child.empty(),
            Self::Owner(child) => child.empty(),
        }
    }
    fn terminate(&self) -> std::io::Result<()> {
        match self {
            Self::Product(child) => child.terminate(),
            Self::Owner(child) => child.terminate(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub action: String,
    pub program: Option<PathBuf>,
    pub arguments: Vec<String>,
}

pub fn required(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(name)
            .unwrap_or_else(|| panic!("required native product gate input missing: {name}")),
    );
    assert!(
        path.is_absolute() && path.is_file(),
        "native gate input must be an existing absolute file: {name}"
    );
    path
}

pub fn prepared_cli() -> PathBuf {
    let bin = required("HIROUTE_WINDOWS_ADMIN_CLI");
    assert_eq!(bin.file_name().unwrap(), "hiroute.exe");
    bin
}

/// All subprocess waits and capture readers are bounded. Secret stdin callers
/// deliberately omit arbitrary process output from failed-command diagnostics.
pub fn process(
    token: Option<&File>,
    program: &Path,
    args: &[OsString],
    input: Option<&[u8]>,
    secret: bool,
    timeout: Duration,
    fixture: bool,
) -> Value {
    assert!(program.is_absolute() && program.is_file());
    let mut environment = token
        .map(account::environment)
        .unwrap_or_else(|| std::env::vars_os().collect());
    environment.retain(|(key, _)| key != PROBE);
    if fixture {
        environment.push((PROBE.into(), "1".into()));
    }
    let mut child = match token {
        Some(token)
            if account::token_sid(token)
                != account::token_sid(&account::process_token(std::process::id())) =>
        {
            Process::Owner(
                launcher::Child::spawn(token, program, args, &environment)
                    .expect("native alternate owner creation failed"),
            )
        }
        _ => {
            let launch = Launch {
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
            };
            Process::Product(
                match token {
                    Some(token) => Child::spawn_as_token(launch, token),
                    None => Child::spawn(launch),
                }
                .expect("controlled native test process creation failed"),
            )
        }
    };
    let (stdin, stdout_file, stderr_file) = child.pipes();
    let reader = |file: File| {
        thread::spawn(move || {
            let mut output = Vec::new();
            file.take(BOUND as u64 + 1)
                .read_to_end(&mut output)
                .unwrap();
            assert!(
                output.len() <= BOUND,
                "native command output exceeds test bound"
            );
            output
        })
    };
    let stdout = reader(stdout_file);
    let stderr = reader(stderr_file);
    if let Some(input) = input {
        stdin
            .unwrap()
            .write_all(input)
            .expect("controlled stdin transfer failed");
    } else {
        drop(stdin);
    }
    let deadline = Instant::now() + timeout;
    let code = loop {
        if let Some(code) = child.exit_code().unwrap() {
            break code;
        }
        assert!(
            Instant::now() < deadline,
            "native command exceeded its bounded timeout"
        );
        thread::sleep(Duration::from_millis(20));
    };
    if !child.empty().unwrap() {
        child.terminate().unwrap();
    }
    let output = stdout.join().unwrap();
    let error = stderr.join().unwrap();
    assert!(
        child.empty().unwrap(),
        "transient native command leaked descendants"
    );
    if secret {
        assert_eq!(
            code, 0,
            "administrator credential operation failed; arbitrary output intentionally suppressed"
        );
        // The credential operation returns a closed no-secret response. Never
        // reflect unknown stdout/stderr when decoding fails.
        assert!(
            error.is_empty(),
            "administrator operation emitted unexpected diagnostic output"
        );
    }
    let parsed = if fixture {
        String::from_utf8_lossy(&output)
            .lines()
            .find_map(|line| line.strip_prefix(OUTPUT))
            .map(|body| {
                serde_json::from_str::<Value>(body).expect("owner fixture result is invalid")
            })
    } else {
        serde_json::from_slice(&output).ok()
    };
    if secret {
        let value = parsed.expect("administrator result is invalid");
        assert_eq!(value["schema"], "hiroute.windows-scm-owner-result/v1");
        assert_eq!(value["completed"], true);
        return json!({"code":code,"result":value});
    }
    json!({"code":code,"result":parsed,"stdout":String::from_utf8_lossy(&output),"stderr":String::from_utf8_lossy(&error)})
}

pub fn owner(token: &File, request: Request, timeout: Duration) -> Value {
    let binary = std::env::current_exe().unwrap();
    let arguments = [
        OsString::from("--exact"),
        ENTRY.into(),
        "--nocapture".into(),
        "--test-threads=1".into(),
    ];
    let input = serde_json::to_vec(&request).unwrap();
    let result = process(
        Some(token),
        &binary,
        &arguments,
        Some(&input),
        false,
        timeout,
        true,
    );
    assert_eq!(result["code"], 0, "actual owner fixture failed: {result}");
    result["result"].clone()
}

pub fn command(token: &File, program: &Path, args: Vec<String>) -> Value {
    owner(
        token,
        Request {
            action: "command".into(),
            program: Some(program.to_owned()),
            arguments: args,
        },
        Duration::from_secs(90),
    )
}

pub fn accepted(result: &Value) -> &Value {
    assert_eq!(result["code"], 0, "production command failed: {result}");
    assert!(
        result["result"].is_object(),
        "production command returned no JSON result: {result}"
    );
    &result["result"]
}

pub fn cli(token: &File, path: &Path, arguments: &[&str]) -> Value {
    let args = arguments
        .iter()
        .map(|v| (*v).to_owned())
        .chain([
            "--output".into(),
            "json".into(),
            "--timeout".into(),
            "45s".into(),
        ])
        .collect();
    let result = command(token, path, args);
    let envelope = accepted(&result);
    assert_eq!(
        envelope["status"], "succeeded",
        "production CLI request failed: {envelope}"
    );
    envelope["data"].clone()
}

fn gateway_ready(listen: SocketAddr) {
    let mut client = TcpStream::connect_timeout(&listen, Duration::from_secs(5)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client
        .write_all(b"GET /_hiroute/ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut bytes = Vec::new();
    client.take(65537).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 65536 && bytes.starts_with(b"HTTP/1.1 200 "));
    let start = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
    let body: Value = serde_json::from_slice(&bytes[start + 4..]).unwrap();
    assert_eq!(body["schema_version"], "hiroute.gateway.ready/v2");
    assert_eq!(body["status"], "ready");
}

/// Only used in a real token-created owner process. The ordinary test selection
/// validates the closed request shape; it cannot be cited as a product gate.
pub fn entry() {
    if std::env::var_os(PROBE).is_none() {
        assert!(
            serde_json::from_value::<Request>(
                json!({"action":"inspect","program":null,"arguments":[],"sid":"S-1-5-18"})
            )
            .is_err()
        );
        return;
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(16385)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(!bytes.is_empty() && bytes.len() <= 16384);
    let request: Request = serde_json::from_slice(&bytes).unwrap();
    let current = account::process_token(std::process::id());
    let sid = background::current_sid_string().unwrap();
    account::assert_standard(&current, &sid);
    let result = match request.action.as_str() {
        "command" => {
            let program = request.program.unwrap();
            let args = request
                .arguments
                .iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            process(
                None,
                &program,
                &args,
                None,
                false,
                Duration::from_secs(75),
                false,
            )
        }
        "layout" => {
            assert!(request.program.is_none() && request.arguments.is_empty());
            let layout = StandaloneLayout::from_environment().unwrap();
            assert!(
                !layout.marker_path.exists(),
                "product gate requires a fresh owner profile"
            );
            json!({"home":layout.home,"state":layout.state_root,"marker":layout.marker_path,"sid":sid})
        }
        "inspect" | "crash" => {
            assert!(request.program.is_none() && request.arguments.is_empty());
            let layout = StandaloneLayout::from_environment().unwrap();
            let record =
                hiroute_host_runtime::read_standalone_install_record(&layout.marker_path).unwrap();
            let daemon = hiroute_host_runtime::standalone_daemon_binary(&record.install_root);
            let held = lifecycle::retain_for_stop(
                &layout.lifecycle_endpoint(),
                &daemon,
                Duration::from_secs(5),
            )
            .unwrap();
            let token = account::process_token(held.pid);
            let session = account::assert_standard(&token, &sid);
            let program = hiroute_host_runtime::standalone_cli_binary(&record.install_root);
            let invoke = |args: &[&str]| {
                let args = args
                    .iter()
                    .map(|v| OsString::from(*v))
                    .chain([
                        "--output".into(),
                        "json".into(),
                        "--timeout".into(),
                        "15s".into(),
                    ])
                    .collect::<Vec<_>>();
                let response = process(
                    None,
                    &program,
                    &args,
                    None,
                    false,
                    Duration::from_secs(20),
                    false,
                );
                let envelope = accepted(&response);
                assert_eq!(envelope["status"], "succeeded");
                envelope["data"].clone()
            };
            let control = invoke(&["system", "status"]);
            let gateway = invoke(&["gateway", "show"]);
            assert_eq!(gateway["ready"], true);
            gateway_ready(
                gateway["connect_address"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .unwrap(),
            );
            let result = json!({"pid":held.pid,"creation_time":held.creation_time,"sid":sid,"session":session,"gateway":gateway,"control":control});
            if request.action == "crash" {
                held.terminate().unwrap();
                assert!(
                    held.wait(Duration::from_secs(10)).unwrap(),
                    "actual daemon held handle did not exit after forced crash"
                );
            }
            result
        }
        "cleanup-state" => {
            assert!(request.program.is_none() && request.arguments.is_empty());
            let layout = StandaloneLayout::from_environment().unwrap();
            assert!(
                !layout.marker_path.exists(),
                "cannot remove state of an installed product"
            );
            if layout.state_root.exists() {
                std::fs::remove_dir_all(&layout.state_root).unwrap();
            }
            json!({"removed":true})
        }
        _ => panic!("unknown owner fixture action"),
    };
    println!("{OUTPUT}{}", serde_json::to_string(&result).unwrap());
}

pub fn inspect(token: &File, crash: bool) -> Value {
    owner(
        token,
        Request {
            action: if crash { "crash" } else { "inspect" }.into(),
            program: None,
            arguments: vec![],
        },
        Duration::from_secs(60),
    )
}
