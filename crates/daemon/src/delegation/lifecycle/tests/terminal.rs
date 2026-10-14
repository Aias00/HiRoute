//! A duplex adapter models a durable tail that requires its input transport to stay live.
//! These are ordering regressions; the real Claude five-tool Continue remains a native gate.
use super::*;
use crate::delegation::acp::{AcpNativeIdentityContract, AcpTerminalCheckpoint, run_acp};
use crate::delegation::finalization::DelegationFinalization;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

#[derive(Default)]
struct TerminalJournal {
    events: Mutex<Vec<&'static str>>,
    finalization: Arc<DelegationFinalization>,
    completed: Notify,
    permissions: AtomicUsize,
    text_updates: AtomicUsize,
    fail_publication: AtomicBool,
    fail_revocation: AtomicBool,
}

impl AcpRunJournal for TerminalJournal {
    fn session_bound(&self, _: &AcpSessionBinding) -> Result<(), DelegationErrorV1> {
        Ok(())
    }
    fn before_prompt(&self) -> Result<(), DelegationErrorV1> {
        Ok(())
    }
    fn text_update(&self, _: &str) -> Result<(), DelegationErrorV1> {
        self.text_updates.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn allow_permission_once(&self, _: &Value) -> bool {
        self.permissions.fetch_add(1, Ordering::SeqCst);
        true
    }
}

impl WorkerRunJournal for TerminalJournal {
    fn begin_finalization(&self) -> Option<DelegationFinalizationLease> {
        Some(self.finalization.acquire())
    }
    fn before_launch(&self) -> Result<(), DelegationErrorV1> {
        Ok(())
    }
    fn process_spawned(&self, _: &WorkerProcessIdentity) -> Result<(), DelegationErrorV1> {
        Ok(())
    }
    fn stop_intent(&self) -> Result<(), DelegationErrorV1> {
        self.events.lock().unwrap().push("revoke");
        if self.fail_revocation.load(Ordering::SeqCst) {
            Err(DelegationErrorV1::PermissionDenied)
        } else {
            Ok(())
        }
    }
    fn completed(&self, _: &AcpRunOutcome) -> Result<(), DelegationErrorV1> {
        assert!(self.finalization.try_acquire().is_none());
        self.events.lock().unwrap().push("completed");
        self.completed.notify_one();
        if self.fail_publication.load(Ordering::SeqCst) {
            Err(DelegationErrorV1::Conflict)
        } else {
            Ok(())
        }
    }
    fn execution_failed(&self, _: DelegationErrorV1) -> Result<(), DelegationErrorV1> {
        self.events.lock().unwrap().push("failed");
        Ok(())
    }
    fn spawned_unrecorded(&self) -> Result<(), DelegationErrorV1> {
        panic!("fixture process must have a valid binding")
    }
}

#[derive(Clone, Copy)]
enum Terminal {
    Tail { check_publication: bool },
    Cancelled,
    Error,
    Disconnect,
    NoTail,
}

struct TailFixture {
    journal: Arc<TerminalJournal>,
    history: PathBuf,
    terminal: Terminal,
    tail_written: Arc<AtomicBool>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl TailFixture {
    fn new(journal: Arc<TerminalJournal>, root: &std::path::Path, terminal: Terminal) -> Self {
        let projects = root.join("projects");
        fixture_private_directory(&projects);
        Self {
            journal,
            history: projects.join("native-session.jsonl"),
            terminal,
            tail_written: Arc::default(),
            task: Mutex::default(),
        }
    }

    async fn joined(&self) {
        let task = self.task.lock().unwrap().take().unwrap();
        timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
}

#[async_trait::async_trait]
impl WorkerPlatformPort for TailFixture {
    fn capabilities(
        &self,
        _: &CandidateWorkerProfile,
    ) -> Result<WorkerPlatformCapabilities, DelegationErrorV1> {
        Ok(WorkerPlatformCapabilities {
            can_start: true,
            can_stop: true,
        })
    }

    async fn launch(&self, request: WorkerLaunchRequest) -> Result<ReadyWorker, DelegationErrorV1> {
        let (client, server) = tokio::io::duplex(8192);
        let journal = self.journal.clone();
        let history = self.history.clone();
        let terminal = self.terminal;
        let tail_written = self.tail_written.clone();
        let task = tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(server);
            let mut lines = BufReader::new(read).lines();
            while let Some(line) = lines.next_line().await.unwrap() {
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap();
                let result = match method {
                    "initialize" => json!({"protocolVersion":1,"agentCapabilities":{}}),
                    "session/new" => json!({"sessionId":"native-session"}),
                    "session/prompt" => match terminal {
                        Terminal::Cancelled => json!({"stopReason":"cancelled"}),
                        Terminal::Error => {
                            write
                                .write_all(
                                    format!(
                                        "{}\n",
                                        json!({"jsonrpc":"2.0","id":request["id"],
                                "error":{"code":-32603,"message":"fixture prompt error"}})
                                    )
                                    .as_bytes(),
                                )
                                .await
                                .unwrap();
                            break;
                        }
                        _ => json!({"stopReason":"end_turn"}),
                    },
                    other => panic!("unexpected fixture method {other}"),
                };
                write
                    .write_all(
                        format!(
                            "{}\n",
                            json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                if method != "session/prompt" {
                    continue;
                }
                let Terminal::Tail { check_publication } = terminal else {
                    if !matches!(terminal, Terminal::Disconnect) {
                        assert!(lines.next_line().await.unwrap().is_none());
                    }
                    break;
                };
                if check_publication {
                    // Completion wakes this adapter while its input is still open. A permission
                    // request arriving afterwards must not re-enter the journal's admission.
                    journal.completed.notified().await;
                    assert_eq!(*journal.events.lock().unwrap(), ["completed", "revoke"]);
                    assert!(journal.finalization.try_acquire().is_none());
                    write.write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":"late-permission",
                        "method":"session/request_permission", "params":{"sessionId":"native-session",
                            "toolCall":{"toolCallId":"late-tool"},"options":[{"optionId":"once","name":"Once","kind":"allow_once"}]}})).as_bytes()).await.unwrap();
                    let response = timeout(Duration::from_millis(250), lines.next_line())
                        .await
                        .unwrap()
                        .unwrap()
                        .unwrap();
                    let response: Value = serde_json::from_str(&response).unwrap();
                    assert_eq!(response["id"], "late-permission");
                    assert_eq!(response["result"]["outcome"]["outcome"], "cancelled");
                    write.write_all(format!("{}\n", json!({"jsonrpc":"2.0","method":"session/update",
                        "params":{"sessionId":"native-session","update":{"sessionUpdate":"agent_message_chunk",
                            "content":{"type":"text","text":"late ignored text"}}}})).as_bytes()).await.unwrap();
                }
                tokio::select! {
                    line = lines.next_line() => assert!(line.unwrap().is_none()),
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {
                        hiroute_platform::durable::write_private_new(&history, b"final-tail\n").unwrap();
                        tail_written.store(true, Ordering::SeqCst);
                        journal.events.lock().unwrap().push("tail");
                        assert!(lines.next_line().await.unwrap().is_none());
                    }
                }
                break;
            }
        });
        *self.task.lock().unwrap() = Some(task);
        let (stdout, stdin) = tokio::io::split(client);
        Ok(ReadyWorker {
            identity: WorkerProcessIdentity {
                launch_nonce: request.launch_nonce,
                handle_id: "held-tail-fixture".into(),
                creation_identity: "tail-fixture-creation".into(),
            },
            stdin: Box::pin(stdin),
            stdout: Box::pin(stdout),
        })
    }

    async fn observe(
        &self,
        _: &WorkerProcessIdentity,
    ) -> Result<WorkerObservation, DelegationErrorV1> {
        Ok(WorkerObservation::Running)
    }

    async fn terminate(
        &self,
        identity: &WorkerProcessIdentity,
        bound: u64,
    ) -> Result<WorkerStopEvidence, DelegationErrorV1> {
        assert_eq!(identity.handle_id, "held-tail-fixture");
        assert_eq!(bound, STOP_WAIT_MS);
        assert!(self.journal.finalization.try_acquire().is_none());
        self.journal.events.lock().unwrap().push("stop");
        Ok(WorkerStopEvidence {
            scope: WorkerStopScope::Root,
            observation: WorkerObservation::Exited { code: Some(0) },
            scope_stopped: true,
            residual_unknown: false,
        })
    }
}

fn claude_inputs() -> (
    WorkerLaunchRequest,
    AcpRunInput,
    crate::test_support::PrivateTempDir,
) {
    let (request, mut input, fixture) = inputs_for_harness(None, WorkerHarnessV1::ClaudeCode);
    assert_eq!(
        request.profile.identity_contract,
        AcpNativeIdentityContract::ClaudeSessionV1
    );
    input.native_session_mode = None;
    input.deadline = Instant::now() + Duration::from_secs(3);
    (request, input, fixture)
}

#[tokio::test]
async fn claude_terminal_checkpoint_keeps_transport_live_until_final_tail() {
    let (request, input, _directory) = claude_inputs();
    let journal = Arc::new(TerminalJournal::default());
    let fixture = TailFixture::new(
        journal.clone(),
        &request.profile.session_root,
        Terminal::Tail {
            check_publication: true,
        },
    );
    let result = execute(&fixture, request, input, journal.clone())
        .await
        .unwrap();
    fixture.joined().await;
    assert!(result.execution.as_ref().unwrap().text.is_empty());
    assert!(result.native_history_settled);
    assert!(result.stop.as_ref().unwrap().scope_stopped);
    assert!(fixture.tail_written.load(Ordering::SeqCst));
    assert_eq!(fs::read(&fixture.history).unwrap(), b"final-tail\n");
    assert_eq!(
        *journal.events.lock().unwrap(),
        ["completed", "revoke", "tail", "stop"]
    );
    assert_eq!(journal.permissions.load(Ordering::SeqCst), 0);
    assert_eq!(journal.text_updates.load(Ordering::SeqCst), 0);
    assert!(journal.finalization.try_acquire().is_none());
    drop(result);
    assert!(journal.finalization.try_acquire().is_some());
}

#[tokio::test]
async fn eof_before_claude_checkpoint_prevents_final_tail() {
    let (request, input, _directory) = claude_inputs();
    let journal = Arc::new(TerminalJournal::default());
    let checkpoint = ClaudeHistoryCheckpoint::capture(&request.profile, &input).unwrap();
    let fixture = TailFixture::new(
        journal.clone(),
        &request.profile.session_root,
        Terminal::Tail {
            check_publication: false,
        },
    );
    let worker = fixture.launch(request).await.unwrap();
    // The previous ordering awaited this foreground result (and dropped the transport)
    // before invoking the exact same history checkpoint below.
    let outcome = run_acp(worker.stdout, worker.stdin, input, journal)
        .await
        .unwrap();
    fixture.joined().await;
    assert!(
        !checkpoint
            .wait_until_settled(outcome.session.native_session_id.as_deref())
            .await
    );
    assert!(!fixture.tail_written.load(Ordering::SeqCst));
    assert!(!fixture.history.exists());
}

#[tokio::test]
async fn claude_cancel_and_protocol_error_do_not_wait_for_history() {
    for terminal in [Terminal::Cancelled, Terminal::Error] {
        let (request, input, _directory) = claude_inputs();
        let journal = Arc::new(TerminalJournal::default());
        let fixture = TailFixture::new(journal.clone(), &request.profile.session_root, terminal);
        let result = timeout(
            Duration::from_millis(500),
            execute(&fixture, request, input, journal.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        fixture.joined().await;
        match terminal {
            Terminal::Cancelled => {
                assert_eq!(result.execution.unwrap().stop_reason, "cancelled");
                assert_eq!(
                    *journal.events.lock().unwrap(),
                    ["completed", "revoke", "stop"]
                );
            }
            Terminal::Error => {
                assert!(result.execution.is_err());
                assert_eq!(
                    *journal.events.lock().unwrap(),
                    ["failed", "revoke", "stop"]
                );
            }
            _ => unreachable!(),
        }
        assert!(!result.native_history_settled);
        assert!(!fixture.tail_written.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn claude_disconnect_after_terminal_keeps_completion_without_history_ready() {
    let (request, input, _directory) = claude_inputs();
    let journal = Arc::new(TerminalJournal::default());
    let fixture = TailFixture::new(
        journal.clone(),
        &request.profile.session_root,
        Terminal::Disconnect,
    );
    let result = execute(&fixture, request, input, journal.clone())
        .await
        .unwrap();
    fixture.joined().await;
    assert_eq!(result.execution.unwrap().stop_reason, "end_turn");
    assert!(!result.native_history_settled);
    assert_eq!(
        *journal.events.lock().unwrap(),
        ["completed", "revoke", "stop"]
    );
}

#[tokio::test]
async fn terminal_publication_failure_does_not_wait_or_double_write() {
    let (request, input, _directory) = claude_inputs();
    let journal = Arc::new(TerminalJournal::default());
    journal.fail_publication.store(true, Ordering::SeqCst);
    journal.fail_revocation.store(true, Ordering::SeqCst);
    let fixture = TailFixture::new(
        journal.clone(),
        &request.profile.session_root,
        Terminal::NoTail,
    );
    let result = timeout(
        Duration::from_millis(500),
        execute(&fixture, request, input, journal.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    fixture.joined().await;
    assert!(matches!(result.execution, Err(DelegationErrorV1::Conflict)));
    assert_eq!(
        result.stop_intent_error,
        Some(DelegationErrorV1::PermissionDenied)
    );
    assert!(result.stop.unwrap().scope_stopped);
    assert!(!result.native_history_settled);
    assert_eq!(
        *journal.events.lock().unwrap(),
        ["completed", "revoke", "stop"]
    );
}

#[tokio::test]
async fn interrupted_terminal_checkpoint_retains_lease_completion_and_false_readiness() {
    let (request, input, _directory) = claude_inputs();
    let journal = Arc::new(TerminalJournal::default());
    let checkpoint = LiveTerminalCheckpoint::new(
        journal.clone(),
        ClaudeHistoryCheckpoint::capture(&request.profile, &input).unwrap(),
    );
    let outcome = AcpRunOutcome {
        session: AcpSessionBinding {
            acp_session_id: "native-session".into(),
            native_session_id: Some("native-session".into()),
        },
        stop_reason: "end_turn".into(),
        text: "completed body".into(),
        content_incomplete: false,
    };
    {
        let callback = checkpoint.before_transport_close(&outcome);
        tokio::pin!(callback);
        tokio::select! {
            biased;
            _ = &mut callback => panic!("missing history cannot settle"),
            _ = journal.completed.notified() => {}
        }
    }
    // Dropping a live checkpoint future models the ACP hard bound interrupting it.
    // Terminal publication remains authoritative, with no invented Continue readiness.
    assert_eq!(*journal.events.lock().unwrap(), ["completed", "revoke"]);
    assert_eq!(checkpoint.revoke(), None);
    let receipt = checkpoint.take_receipt().unwrap();
    assert_eq!(receipt.execution.unwrap().text, "completed body");
    assert!(!receipt.history_settled);
    assert!(journal.finalization.try_acquire().is_none());
    drop(receipt.finalization);
    assert!(journal.finalization.try_acquire().is_some());
    assert_eq!(*journal.events.lock().unwrap(), ["completed", "revoke"]);
}
