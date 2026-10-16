//! Public configuration recovery without manufacturing a Task registration.
//! The CLI reports manager-unavailable; a later explicit real foreground launch
//! applies recovered A. This is deliberately distinct from manager restart.
use super::windows_headless_business::missing_gateway_manager;
use super::{InstalledProduct, RunningDaemon, assert_no_secret, gateway_ready, lifecycle};
use hiroute_host_runtime::{GatewayListenerConfigV1, GatewayListenerOperationStateV1};
use serde_json::Value;
use std::io::Read;
use std::net::{SocketAddr, TcpListener};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub struct Recovery {
    occupied: TcpListener,
    blocked: SocketAddr,
    applied: SocketAddr,
}
impl Recovery {
    pub fn begin(product: &InstalledProduct, running: &RunningDaemon) -> Self {
        let original = config(product);
        let applied = original.applied_address().unwrap().unwrap();
        assert_eq!(applied, running.listen);
        gateway_ready(applied);
        let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
        let blocked = occupied.local_addr().unwrap();
        assert_ne!(blocked, applied);
        let port = blocked.port().to_string();
        failed_command(
            product,
            &["gateway", "set", "--address", "127.0.0.1", "--port", &port],
        );
        let config = config(product);
        assert_eq!(config.desired_address().unwrap(), Some(blocked));
        assert_eq!(config.applied_address().unwrap(), Some(applied));
        failed_operation(&config);
        // No manager exists: failed manager lookup must neither kill the actual
        // foreground daemon nor make a second listener appear at desired B.
        gateway_ready(applied);
        let actual = lifecycle::query(
            &product.layout.lifecycle_endpoint(),
            &product.daemon,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(actual.pid, running.child.id());
        assert!(!actual.exited().unwrap());
        Self {
            occupied,
            blocked,
            applied,
        }
    }
    pub fn recover_without_manager(&self, product: &InstalledProduct) {
        self.refuse_blocked_foreground(product);
        assert_eq!(self.occupied.local_addr().unwrap(), self.blocked);
        failed_command(product, &["gateway", "recover"]);
        let config = config(product);
        assert_eq!(config.desired_address().unwrap(), Some(self.applied));
        assert_eq!(config.applied_address().unwrap(), Some(self.applied));
        failed_operation(&config);
        assert_eq!(product.cli(&["gateway", "show"])["data"]["ready"], false);
    }
    pub fn recovered(&self, product: &InstalledProduct, running: &RunningDaemon) {
        assert_eq!(self.occupied.local_addr().unwrap(), self.blocked);
        assert_eq!(running.listen, self.applied);
        gateway_ready(self.applied);
        let config = config(product);
        assert_eq!(config.desired_address().unwrap(), Some(self.applied));
        assert_eq!(config.applied_address().unwrap(), Some(self.applied));
        assert_eq!(
            config.operation.unwrap().state,
            GatewayListenerOperationStateV1::Succeeded
        );
        println!(
            "Windows foreground public Gateway recovery: typed unavailable manager, occupied B refused, retained A reapplied by explicit service run; manager recovery remains a separate gate"
        );
    }
    fn refuse_blocked_foreground(&self, product: &InstalledProduct) {
        let child = Command::new(&product.daemon)
            .args(["--role", "all", "--standalone"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut child = RunningDaemon {
            child,
            stdout_reader: None,
            stderr_reader: None,
            listen: self.blocked,
        };
        let output = capture(child.child.stdout.take().unwrap());
        let error = capture(child.child.stderr.take().unwrap());
        let deadline = Instant::now() + Duration::from_secs(35);
        loop {
            if let Some(status) = child.child.try_wait().unwrap() {
                assert!(
                    !status.success(),
                    "occupied fixed desired B must prevent real standalone startup"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "occupied listener startup did not fail within its bound"
            );
            thread::sleep(Duration::from_millis(20));
        }
        let output = output.join().unwrap();
        let error = error.join().unwrap();
        assert!(
            !output
                .split(|byte| *byte == b'\n')
                .any(|line| serde_json::from_slice::<Value>(line)
                    .is_ok_and(|value| value["schema"] == "hiroute.daemon-ready/v1")),
            "failed listener cannot publish readiness"
        );
        assert!(
            !error.is_empty(),
            "real startup refusal must emit its fixed diagnostic"
        );
        assert!(
            lifecycle::query(
                &product.layout.lifecycle_endpoint(),
                &product.daemon,
                Duration::from_secs(5)
            )
            .is_err()
        );
    }
}
fn capture(file: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        file.take(256 * 1024 + 1).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 256 * 1024);
        assert_no_secret(&bytes);
        bytes
    })
}
fn config(product: &InstalledProduct) -> GatewayListenerConfigV1 {
    let shown = product.cli(&["gateway", "show"]);
    let config: GatewayListenerConfigV1 =
        serde_json::from_value(shown["data"]["config"].clone()).unwrap();
    config.validate().unwrap();
    config
}
fn failed_command(product: &InstalledProduct, args: &[&str]) {
    let (code, result) = missing_gateway_manager(product, args);
    assert_ne!(
        code, 0,
        "missing manager must remain a typed public failure"
    );
    assert_eq!(result["error"]["code"], "DAEMON_UNAVAILABLE");
}

#[test]
#[ignore = "focused execution of the full journey's foreground recovery"]
fn actual_windows_foreground_gateway_recovery() {
    let product = InstalledProduct::create();
    let mut first = product.start();
    let recovery = Recovery::begin(&product, &first);
    lifecycle::stop(
        &product.layout.lifecycle_endpoint(),
        &product.daemon,
        Duration::from_secs(30),
    )
    .unwrap();
    first.wait(true);
    recovery.recover_without_manager(&product);
    let mut running = product.start_with_entry(true);
    recovery.recovered(&product, &running);
    lifecycle::stop(
        &product.layout.lifecycle_endpoint(),
        &product.daemon,
        Duration::from_secs(30),
    )
    .unwrap();
    running.wait(true);
}
fn failed_operation(config: &GatewayListenerConfigV1) {
    let operation = config.operation.as_ref().unwrap();
    assert_eq!(operation.state, GatewayListenerOperationStateV1::Failed);
    assert_eq!(
        operation.error_code.as_deref(),
        Some("SERVICE_RESTART_FAILED")
    );
}
