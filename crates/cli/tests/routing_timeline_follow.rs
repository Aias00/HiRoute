#![cfg(unix)]
//! The installed CLI retries actual reader admission failures over Local Control.
use hiroute_domain::*;
use hiroute_observation::{DigestAuthority, LocalObservationStore};
use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

struct TracedStore {
    store: Arc<LocalObservationStore>,
    busy: mpsc::Sender<()>,
}

impl ObservationQueryPort for TracedStore {
    fn observed_routing_timeline(
        &self,
        r: &ObservationReaderContext,
        q: &RoutingTimelineQueryV1,
        now: i64,
    ) -> Result<RoutingTimelinePageV1, ObservationQueryError> {
        let result = ObservationQueryPort::observed_routing_timeline(&*self.store, r, q, now);
        if matches!(&result, Err(ObservationQueryError::ReaderBusy)) {
            self.busy.send(()).unwrap();
        }
        result
    }
    fn list_sessions(
        &self,
        w: &WorkspaceId,
        q: &SessionListQueryV1,
    ) -> Result<SessionListV1, ObservationQueryError> {
        ObservationQueryPort::list_sessions(&*self.store, w, q)
    }
    fn get_session(
        &self,
        w: &WorkspaceId,
        s: &SessionId,
        c: ContentMode,
    ) -> Result<SessionDetailV1, ObservationQueryError> {
        ObservationQueryPort::get_session(&*self.store, w, s, c)
    }
    fn get_receipt(
        &self,
        w: &WorkspaceId,
        r: &ReceiptId,
    ) -> Result<RoutingReceiptV1, ObservationQueryError> {
        ObservationQueryPort::get_receipt(&*self.store, w, r)
    }
    fn get_value(
        &self,
        w: &WorkspaceId,
        q: &ValueQueryV1,
    ) -> Result<ValueViewV1, ObservationQueryError> {
        ObservationQueryPort::get_value(&*self.store, w, q)
    }
    fn get_status(&self, w: &WorkspaceId) -> Result<ObservationStatusV1, ObservationQueryError> {
        ObservationQueryPort::get_status(&*self.store, w)
    }
}

struct OwnedChild(Option<Child>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn review_cli_follow_retries_real_busy_reads() {
    // Runtime startup may inspect native paths; keep this test and its CLI in
    // one isolated child HOME without changing the test runner's environment.
    if std::env::var_os("HIROUTE_TIMELINE_TEST_CHILD").is_none() {
        let home = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "review_cli_follow_retries_real_busy_reads",
                "--nocapture",
            ])
            .env("HOME", home.path())
            .env("HIROUTE_TIMELINE_TEST_CHILD", "1")
            .env_remove("PI_CODING_AGENT_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("DSH_CONFIG_DIR")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let state = root.path().join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let runtime = hiroute_daemon::control::ProductionControlRuntime::open(&state).unwrap();
    let store = Arc::new(
        LocalObservationStore::open(
            root.path().join("observation"),
            DigestAuthority::new([27; 32]),
        )
        .unwrap(),
    );
    let (busy, reads) = mpsc::channel();
    let mut ports = runtime.application_ports();
    ports.observation = Arc::new(TracedStore {
        store: store.clone(),
        busy,
    });
    let endpoint = root.path().join("runtime");
    let mut server = hiroute_daemon::control::start_control(
        hiroute_application::ApplicationService::new(ports),
        &endpoint,
    )
    .unwrap();
    let mut child = store.with_all_query_permits_for_test(|| {
        let child = OwnedChild(Some(
            Command::new(env!("CARGO_BIN_EXE_hiroute"))
                .args([
                    "sessions",
                    "timeline",
                    "missing-session",
                    "--follow",
                    "--output",
                    "json",
                ])
                .env("HIROUTE_RUNTIME_DIR", &endpoint)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ));
        // Two real failures prove the CLI survived the first busy response and
        // retried through the server, rather than exiting or using a fake hint.
        for _ in 0..2 {
            reads
                .recv_timeout(Duration::from_secs(5))
                .expect("follow did not retry the real busy read");
        }
        child
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "follow did not stop for the terminal not-found result after reader recovery"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "RESOURCE_NOT_FOUND", "{value}");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("temporarily unavailable"));
    server.shutdown();
    server.join(Duration::from_secs(3)).unwrap();
}
