#![cfg(windows)]
//! Real installed binaries on a disposable Windows user profile. No mocked HiRoute runtime.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use hiroute_application_api::{
    STANDALONE_PROTECTED_INPUT_MAX_FRAME_BYTES_V1, StandaloneProtectedInputRequestV1,
    StandaloneProtectedInputResponseV1,
};
use hiroute_host_runtime::{StandaloneInstallRecordV1, StandaloneLayout};
use hiroute_platform::local_record::{self, Channel, Direction};
use hiroute_platform::windows::{lifecycle, pipe, security};
use serde_json::{Value, json};

const SECRET: &str = "windows_headless_stdin_only_secret_0123456789";
const CANDIDATE: &str = "candidate/native/agent-token-windows-headless";
const IO_BOUND: Duration = Duration::from_secs(5);

struct InstalledProduct {
    layout: StandaloneLayout,
    root: PathBuf,
    cli: PathBuf,
    daemon: PathBuf,
    stable: PathBuf,
    stable_daemon: PathBuf,
}

impl InstalledProduct {
    fn create() -> Self {
        let layout = StandaloneLayout::from_environment().unwrap();
        // Native KnownFolders cannot be redirected by HOME. Require a disposable CI
        // profile and refuse to overwrite or remove an existing user's installation.
        assert!(
            !layout.marker_path.exists() && !layout.state_root.exists(),
            "Windows product gate requires a fresh disposable Windows profile"
        );
        let stable = hiroute_host_runtime::standalone_cli_binary(&layout.home.join(".local/bin"));
        let stable_daemon =
            hiroute_host_runtime::standalone_daemon_binary(&layout.home.join(".local/bin"));
        assert!(
            !stable.exists() && !stable_daemon.exists(),
            "product gate must not replace an existing stable entry"
        );
        security::ensure_private_directory(&layout.data_root).unwrap();
        let root = layout
            .data_root
            .join(format!("Windows headless 测试 {}", std::process::id()));
        security::create_owner_only_directory(&root).unwrap();
        let prepared = std::env::var_os("HIROUTE_VALIDATION_PRODUCT_BIN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_BIN_EXE_hirouted"))
                    .parent()
                    .unwrap()
                    .to_owned()
            });
        let source_cli = hiroute_host_runtime::standalone_cli_binary(&prepared);
        let source_daemon = hiroute_host_runtime::standalone_daemon_binary(&prepared);
        assert!(
            source_cli.is_file() && source_daemon.is_file(),
            "both real CLI and daemon binaries must be built before this product gate"
        );
        let cli = hiroute_host_runtime::standalone_cli_binary(&root);
        let daemon = hiroute_host_runtime::standalone_daemon_binary(&root);
        std::fs::copy(source_cli, &cli).unwrap();
        std::fs::copy(source_daemon, &daemon).unwrap();
        security::set_owner_only(&cli, false).unwrap();
        security::set_owner_only(&daemon, false).unwrap();
        security::ensure_private_directory(stable.parent().unwrap()).unwrap();
        std::fs::hard_link(&cli, &stable).unwrap();
        std::fs::hard_link(&daemon, &stable_daemon).unwrap();
        let record = StandaloneInstallRecordV1 {
            schema_version: hiroute_host_runtime::STANDALONE_INSTALL_SCHEMA_V1.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            target: hiroute_host_runtime::standalone_host_target(),
            install_root: root.clone(),
            service_definition: root.join("fixture-task.xml"),
            installed_skills: vec![],
            cpa_binary: None,
            cpa_sha256: None,
        };
        record.validate().unwrap();
        let mut marker = security::create_owner_only_file(&layout.marker_path).unwrap();
        serde_json::to_writer(&mut marker, &record).unwrap();
        marker.flush().unwrap();
        Self {
            layout,
            root,
            cli,
            daemon,
            stable,
            stable_daemon,
        }
    }

    fn start(&self) -> RunningDaemon {
        let mut child = Command::new(&self.daemon)
            .args(["--role", "all", "--standalone"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (send_ready, readiness) = mpsc::sync_channel(1);
        let stdout_reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut output = Vec::new();
            loop {
                let mut line = Vec::new();
                if reader.read_until(b'\n', &mut line).unwrap() == 0 {
                    break;
                }
                if let Ok(value) = serde_json::from_slice::<Value>(&line)
                    && value["schema"] == "hiroute.daemon-ready/v1"
                {
                    let _ = send_ready.try_send(value);
                }
                assert!(
                    output.len() + line.len() <= 256 * 1024,
                    "daemon output exceeded gate bound"
                );
                output.extend(line);
            }
            output
        });
        let stderr_reader = thread::spawn(move || {
            let mut output = Vec::new();
            stderr
                .take(256 * 1024 + 1)
                .read_to_end(&mut output)
                .unwrap();
            assert!(
                output.len() <= 256 * 1024,
                "daemon stderr exceeded gate bound"
            );
            output
        });
        let mut running = RunningDaemon {
            child,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
            listen: "127.0.0.1:1".parse().unwrap(),
        };
        let ready = readiness
            .recv_timeout(Duration::from_secs(45))
            .expect("real standalone daemon must emit its production readiness frame");
        assert_eq!(
            ready["process_id"].as_u64(),
            Some(u64::from(running.child.id()))
        );
        running.listen = ready["gateway_listen"].as_str().unwrap().parse().unwrap();
        assert_eq!(
            ready["control_endpoint"],
            json!(self.layout.runtime_root.join("hiroute/control.sock"))
        );
        running
    }

    fn verify_prepared_installation_refuses_business_startup(&self) {
        let service_root = self.layout.data_root.join("service");
        assert!(
            !service_root.exists(),
            "product fixture requires a fresh installation maintenance directory"
        );
        security::create_owner_only_directory(&service_root).unwrap();
        let path = service_root.join("install-journal.json");
        let mut journal = security::create_owner_only_file(&path).unwrap();
        serde_json::to_writer(
            &mut journal,
            &json!({
                "schema": "hiroute.windows-install-journal/v1",
                "operation": "install",
                "phase": "prepared",
                "old_marker": null,
                "new_marker": null,
                "old_definition": null,
                "new_definition": null,
                "files": []
            }),
        )
        .unwrap();
        journal.sync_all().unwrap();
        drop(journal);
        let child = Command::new(&self.daemon)
            .args(["--role", "all", "--standalone"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut rejected = RunningDaemon {
            child,
            stdout_reader: None,
            stderr_reader: None,
            listen: "127.0.0.1:1".parse().unwrap(),
        };
        let deadline = Instant::now() + IO_BOUND;
        loop {
            if let Some(status) = rejected.child.try_wait().unwrap() {
                assert!(!status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "prepared maintenance must reject before starting the business runtime"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        rejected
            .child
            .stdout
            .take()
            .unwrap()
            .take(4096)
            .read_to_end(&mut stdout)
            .unwrap();
        rejected
            .child
            .stderr
            .take()
            .unwrap()
            .take(4096)
            .read_to_end(&mut stderr)
            .unwrap();
        assert!(
            stdout.is_empty(),
            "rejected admission cannot publish daemon readiness"
        );
        assert!(
            String::from_utf8_lossy(&stderr)
                .contains("standalone installation maintenance is incomplete")
        );
        assert!(!self.layout.state_root.exists());
        assert!(
            lifecycle::query(&self.layout.lifecycle_endpoint(), &self.daemon, IO_BOUND).is_err()
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(service_root).unwrap();
    }

    fn cli(&self, args: &[&str]) -> Value {
        let output = Command::new(&self.stable)
            .args(args)
            .args(["--output", "json", "--timeout", "10s"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "actual public CLI failed: {:?}",
            output.status
        );
        assert_no_secret(&output.stdout);
        assert_no_secret(&output.stderr);
        serde_json::from_slice(&output.stdout).expect("public CLI must return JSON")
    }

    fn register_inherited_secret(&self) -> Value {
        use hiroute_platform::windows::job::{self, Launch, Stream};
        use std::ffi::OsString;
        use std::os::windows::io::AsRawHandle;
        let (read, mut write) = job::credential_pipe_pair().unwrap();
        let carrier = job::duplicate_inheritable_pipe(&read).unwrap();
        let args = [
            OsString::from("protected-input"),
            "register".into(),
            "--candidate".into(),
            CANDIDATE.into(),
            "--secret-handle".into(),
            (carrier.as_raw_handle() as usize as u64).to_string().into(),
            "--output".into(),
            "json".into(),
            "--timeout".into(),
            "10s".into(),
        ];
        let environment: Vec<_> = std::env::vars_os().collect();
        assert!(
            args.iter()
                .all(|value| !value.to_string_lossy().contains(SECRET))
        );
        assert!(
            environment
                .iter()
                .all(|(_, value)| !value.to_string_lossy().contains(SECRET))
        );
        let mut child = job::Child::spawn_with_inherited_pipes(
            Launch {
                program: &self.stable,
                args: &args,
                env: &environment,
                cwd: self.stable.parent(),
                stdin: Stream::Null,
                stdout: Stream::Piped,
                stderr: Stream::Piped,
            },
            &[&carrier],
        )
        .unwrap();
        write.write_all(SECRET.as_bytes()).unwrap();
        drop(write);
        drop(carrier);
        drop(read);
        let deadline = Instant::now() + Duration::from_secs(15);
        let exit = loop {
            if let Some(exit) = child.exit_code().unwrap() {
                break exit;
            }
            assert!(
                Instant::now() < deadline,
                "actual protected-input CLI did not consume inherited EOF"
            );
            thread::sleep(Duration::from_millis(20));
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        child
            .stdout
            .take()
            .unwrap()
            .take(65537)
            .read_to_end(&mut stdout)
            .unwrap();
        child
            .stderr
            .take()
            .unwrap()
            .take(65537)
            .read_to_end(&mut stderr)
            .unwrap();
        assert!(stdout.len() <= 65536 && stderr.len() <= 65536);
        assert_no_secret(&stdout);
        assert_no_secret(&stderr);
        assert_eq!(
            exit, 0,
            "actual protected-input CLI inherited handle failed"
        );
        assert!(
            child.empty().unwrap(),
            "actual protected-input CLI leaked descendants"
        );
        serde_json::from_slice(&stdout).unwrap()
    }

    fn input(
        &self,
        request: StandaloneProtectedInputRequestV1,
        ack: bool,
    ) -> std::io::Result<StandaloneProtectedInputResponseV1> {
        let mut stream =
            pipe::Stream::connect_timeout(&self.layout.protected_input_socket(), IO_BOUND)?;
        stream.set_deadline(Some(Instant::now() + IO_BOUND))?;
        let body = serde_json::to_vec(&request).unwrap();
        local_record::write(
            &mut stream,
            Channel::ProtectedInput,
            Direction::Request,
            &body,
            STANDALONE_PROTECTED_INPUT_MAX_FRAME_BYTES_V1 as usize,
        )?;
        let response = local_record::read(
            &mut stream,
            Channel::ProtectedInput,
            Direction::Response,
            STANDALONE_PROTECTED_INPUT_MAX_FRAME_BYTES_V1 as usize,
        )?;
        if ack {
            local_record::acknowledge(&mut stream)?;
        }
        serde_json::from_slice(&response).map_err(std::io::Error::other)
    }
}

impl Drop for InstalledProduct {
    fn drop(&mut self) {
        let _ = lifecycle::stop(
            &self.layout.lifecycle_endpoint(),
            &self.daemon,
            Duration::from_secs(5),
        );
        let _ = std::fs::remove_file(&self.stable);
        let _ = std::fs::remove_file(&self.stable_daemon);
        let _ = std::fs::remove_file(&self.layout.marker_path);
        let _ = std::fs::remove_dir_all(&self.layout.state_root);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct RunningDaemon {
    child: Child,
    stdout_reader: Option<thread::JoinHandle<Vec<u8>>>,
    stderr_reader: Option<thread::JoinHandle<Vec<u8>>>,
    listen: SocketAddr,
}

impl RunningDaemon {
    fn wait(&mut self, success: bool) {
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.success(), success, "unexpected daemon termination");
                break;
            }
            assert!(
                Instant::now() < deadline,
                "daemon did not terminate within its join bound"
            );
            thread::sleep(Duration::from_millis(20));
        }
        for reader in [&mut self.stdout_reader, &mut self.stderr_reader] {
            if let Some(reader) = reader.take() {
                assert_no_secret(&reader.join().unwrap());
            }
        }
    }
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_no_secret(bytes: &[u8]) {
    assert!(
        !bytes
            .windows(SECRET.len())
            .any(|window| window == SECRET.as_bytes()),
        "protected input leaked into process output"
    );
}

fn gateway_ready(listen: SocketAddr) {
    let mut client = TcpStream::connect_timeout(&listen, IO_BOUND).unwrap();
    client.set_read_timeout(Some(IO_BOUND)).unwrap();
    client
        .write_all(b"GET /_hiroute/ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    let body_start = response
        .windows(4)
        .position(|value| value == b"\r\n\r\n")
        .unwrap();
    let body: Value = serde_json::from_slice(&response[body_start + 4..]).unwrap();
    assert_eq!(body["schema_version"], "hiroute.gateway.ready/v2");
    assert_eq!(body["status"], "ready");
}

#[test]
fn installed_native_headless_roundtrip_and_graceful_then_forced_restart() {
    let product = InstalledProduct::create();
    product.verify_prepared_installation_refuses_business_startup();
    let mut first = product.start();
    gateway_ready(first.listen);
    assert_eq!(product.cli(&["system", "status"])["status"], "succeeded");
    let owner = lifecycle::query(
        &product.layout.lifecycle_endpoint(),
        &product.daemon,
        IO_BOUND,
    )
    .unwrap();
    assert_eq!(owner.pid, first.child.id());
    assert!(!owner.exited().unwrap());

    let response = product
        .input(
            StandaloneProtectedInputRequestV1::register(CANDIDATE, SECRET.into()),
            false,
        )
        .unwrap();
    assert!(response.registered);
    // The successful response was consumed, but intentionally has no ACK. A duplicate
    // registration must still reject, proving timeout did not silently revoke its input.
    thread::sleep(Duration::from_millis(2200));
    assert!(
        product
            .input(
                StandaloneProtectedInputRequestV1::register(CANDIDATE, SECRET.into()),
                true
            )
            .is_err()
    );
    assert_eq!(
        product.cli(&["protected-input", "release", "--candidate", CANDIDATE])["data"]["registered"],
        true
    );
    assert!(
        product
            .input(
                StandaloneProtectedInputRequestV1::register(CANDIDATE, SECRET.into()),
                true
            )
            .unwrap()
            .registered
    );
    assert!(
        product
            .input(StandaloneProtectedInputRequestV1::release(CANDIDATE), true)
            .unwrap()
            .registered
    );
    let inherited = product.register_inherited_secret();
    assert_eq!(inherited["status"], "succeeded");
    assert_eq!(inherited["data"]["registered"], true);
    assert_eq!(
        product.cli(&["protected-input", "release", "--candidate", CANDIDATE])["data"]["registered"],
        true
    );

    let grant_endpoint = product
        .layout
        .runtime_root
        .join("hiroute/agent-grant-v1.sock");
    let mut bad_grant = pipe::Stream::connect_timeout(&grant_endpoint, IO_BOUND).unwrap();
    bad_grant
        .set_deadline(Some(Instant::now() + IO_BOUND))
        .unwrap();
    local_record::write(
        &mut bad_grant,
        Channel::AgentGrant,
        Direction::Request,
        b"invalid-grant",
        65536,
    )
    .unwrap();
    assert!(
        local_record::read(
            &mut bad_grant,
            Channel::AgentGrant,
            Direction::Response,
            65536
        )
        .is_err()
    );
    assert_eq!(product.cli(&["system", "status"])["status"], "succeeded");

    let port = first.listen;
    lifecycle::stop(
        &product.layout.lifecycle_endpoint(),
        &product.daemon,
        Duration::from_secs(30),
    )
    .unwrap();
    first.wait(true);
    assert!(owner.exited().unwrap());
    drop(TcpListener::bind(port).expect("graceful RoleAll join must release the listener"));

    let mut second = product.start();
    gateway_ready(second.listen);
    assert_eq!(product.cli(&["system", "status"])["status"], "succeeded");
    second.child.kill().unwrap();
    second.wait(false);
    let mut recovered = product.start();
    gateway_ready(recovered.listen);
    assert_eq!(product.cli(&["system", "status"])["status"], "succeeded");
    lifecycle::stop(
        &product.layout.lifecycle_endpoint(),
        &product.daemon,
        Duration::from_secs(30),
    )
    .unwrap();
    recovered.wait(true);
    assert!(product.cli.is_file());
}
