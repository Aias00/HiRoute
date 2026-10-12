use std::cell::RefCell;
use std::collections::VecDeque;

use hiroute_application_api::{
    AgentSettingsApplyV2, ErrorV1, LocalControlWireRequestV2, MachineEnvelopeV2, MachineStatus,
};
use hiroute_client_core::{ClientFailure, FailureCode, SubmissionState};
use serde_json::{Value, json};

use super::*;

type Reply = Result<MachineEnvelopeV2<Value>, ClientFailure>;
struct Script {
    replies: RefCell<VecDeque<(&'static str, Reply)>>,
    calls: RefCell<Vec<LocalControlWireRequestV2>>,
}
impl Script {
    fn new(replies: Vec<(&'static str, Reply)>) -> Self {
        Self {
            replies: RefCell::new(replies.into()),
            calls: RefCell::new(Vec::new()),
        }
    }
    fn done(&self) {
        assert!(self.replies.borrow().is_empty());
    }
}
impl flow::Control for Script {
    fn call(&self, request: LocalControlWireRequestV2) -> Reply {
        let (operation, reply) = self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("unexpected extra call");
        assert_eq!(request.operation_id, operation);
        assert!(request.protected_grant.is_none());
        self.calls.borrow_mut().push(request);
        reply
    }
}
fn succeeded(data: Value) -> Reply {
    Ok(MachineEnvelopeV2::succeeded(data, Some("request".into())))
}
fn scan(agent_id: &str, codex: bool) -> Reply {
    succeeded(
        json!({"agents":[{"agent_id":agent_id,"supported":true,"context_id":"agent-context/profile", "codex_access": if codex { json!({"root_context_id":"agent-context/root", "profile_context_id":"agent-context/profile", "selected_mode":"profile", "slot_occupied":true}) } else { Value::Null }}]}),
    )
}
fn spec(context: &str, mode: &str) -> Value {
    json!({"schema_version":{"major":2,"minor":0},"context_id":context,"model":{"intent":"keep"},"access_token":{"intent":"keep"},"collaboration":{"intent":"configure","settings":{"trigger_mode":mode}}})
}
fn preview(spec: Value, capabilities: &[&str], extra_block: bool) -> Reply {
    let digest = hiroute_application_api::CanonicalDigest::of_bytes(b"preview");
    let blockers = if capabilities.is_empty() && !extra_block {
        json!([])
    } else {
        json!([{"facet":"collaboration", "reason":if extra_block { "skill_file_conflict" } else { "capability_unavailable" },"capabilities":capabilities.iter().map(|capability| json!({"capability":capability})).collect::<Vec<_>>()}])
    };
    succeeded(
        json!({"schema":"hiroute.agent-settings-preview/v2","spec":spec,"applicable":blockers.as_array().unwrap().is_empty(),"blockers":blockers,"accept_digest":digest,"dependency_digest":digest,"expected_revisions":{"target":0,"dependencies":{}}}),
    )
}
fn accepted() -> Reply {
    let mut response = MachineEnvelopeV2::succeeded(
        json!({"operation_id":"operation/1"}),
        Some("request".into()),
    );
    response.status = MachineStatus::Accepted;
    Ok(response)
}
fn invocation(arguments: &[&str]) -> Invocation {
    parse(
        &arguments
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
fn test_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // A shared state ancestor is legal. Do not let the runner's private umask
        // hide an incorrectly selected receipt namespace in this fixture.
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    directory
}
fn receipt_root(directory: &tempfile::TempDir) -> std::path::PathBuf {
    receipt::root(
        &directory.path().join("state"),
        &directory.path().join("run/control.sock"),
    )
    .unwrap()
}
fn run(script: &Script, arguments: &[&str]) -> MachineEnvelopeV2<Value> {
    let directory = test_directory();
    let response = flow::run(
        script,
        &receipt_root(&directory),
        invocation(arguments),
        "request".into(),
    );
    script.done();
    response
}

#[test]
fn configure_occupied_codex_profile_checks_then_saves_root_with_model_keep() {
    let original = spec("agent-context/root", "explicit");
    let mut sealed = original.clone();
    sealed["protected_native_model_ids"] = json!(["native-model"]);
    let script = Script::new(vec![
        ("ScanAgents", scan("agent_codex_default", true)),
        (
            "PreviewAgentConnectionChange",
            preview(original, &["skill_loading", "trusted_cli_execution"], false),
        ),
        (
            "CheckAgentConnection",
            succeeded(json!({"scope":"collaboration","model_call":false})),
        ),
        ("PreviewAgentConnectionChange", preview(sealed, &[], false)),
        ("ApplyAgentConnectionChange", accepted()),
    ]);
    let response = run(
        &script,
        &[
            "configure",
            "agent_codex_default",
            "--trigger",
            "explicit",
            "--idempotency-key",
            "save1",
        ],
    );
    assert_eq!(response.status, MachineStatus::Accepted);
    let calls = script.calls.borrow();
    assert_eq!(calls[2].payload["scope"], "collaboration");
    assert_eq!(calls[2].payload["allow_model_call"], false);
    assert!(calls[2].payload.get("target").is_none());
    let apply: AgentSettingsApplyV2 = serde_json::from_value(calls[4].payload.clone()).unwrap();
    assert_eq!(apply.spec.context_id, "agent-context/root");
    assert!(matches!(
        apply.spec.model,
        hiroute_application_api::AgentFacetIntent::Keep
    ));
    assert_eq!(apply.spec.protected_native_model_ids, ["native-model"]);
    assert_eq!(
        response.data.unwrap()["cli_submission"]["idempotency_key"],
        "save1"
    );
}

#[test]
fn preview_is_read_only_and_other_agents_use_their_own_context() {
    for (agent_id, codex) in [
        ("agent_codex_default", true),
        ("agent_claude_default", false),
        ("agent_qoder_default", false),
        ("agent_pi_default", false),
        ("agent_dsh_default", false),
    ] {
        let script = Script::new(vec![
            ("ScanAgents", scan(agent_id, codex)),
            (
                "PreviewAgentConnectionChange",
                preview(
                    spec(
                        if codex {
                            "agent-context/root"
                        } else {
                            "agent-context/profile"
                        },
                        "explicit",
                    ),
                    &["skill_loading"],
                    false,
                ),
            ),
        ]);
        let directory = test_directory();
        let receipts = receipt_root(&directory);
        let response = flow::run(
            &script,
            &receipts,
            invocation(&["configure", agent_id, "--trigger", "explicit", "--preview"]),
            "request".into(),
        );
        assert_eq!(response.status, MachineStatus::Succeeded);
        assert_eq!(response.data.unwrap()["applicable"], false);
        assert!(!receipts.exists());
        script.done();
    }
}

#[test]
fn a_successful_check_cannot_override_a_blocker_or_an_unexpected_preview_intent() {
    for changed_context in [false, true] {
        let initial = spec("agent-context/root", "explicit");
        let mut next = initial.clone();
        if changed_context {
            next["context_id"] = json!("agent-context/profile");
        }
        let script = Script::new(vec![
            ("ScanAgents", scan("agent_codex_default", true)),
            (
                "PreviewAgentConnectionChange",
                preview(initial, &["skill_loading"], false),
            ),
            ("CheckAgentConnection", succeeded(json!({}))),
            (
                "PreviewAgentConnectionChange",
                preview(next, &[], !changed_context),
            ),
        ]);
        let response = run(
            &script,
            &["configure", "agent_codex_default", "--trigger", "explicit"],
        );
        assert_eq!(
            response.status,
            if changed_context {
                MachineStatus::Unavailable
            } else {
                MachineStatus::Conflict
            }
        );
    }
}

#[test]
fn ingress_and_collaboration_checks_are_bounded_and_a_failed_check_stops_save() {
    let initial = spec("agent-context/root", "explicit");
    let script = Script::new(vec![
        ("ScanAgents", scan("agent_codex_default", true)),
        (
            "PreviewAgentConnectionChange",
            preview(
                initial.clone(),
                &["ingress_authentication", "skill_loading"],
                false,
            ),
        ),
        ("CheckAgentConnection", succeeded(json!({}))),
        (
            "PreviewAgentConnectionChange",
            preview(initial.clone(), &["skill_loading"], false),
        ),
        ("CheckAgentConnection", succeeded(json!({}))),
        (
            "PreviewAgentConnectionChange",
            preview(initial, &["ingress_authentication"], false),
        ),
    ]);
    let response = run(
        &script,
        &["configure", "agent_codex_default", "--trigger", "explicit"],
    );
    assert_eq!(response.status, MachineStatus::Conflict);
    let calls = script.calls.borrow();
    assert_eq!(calls[2].payload["scope"], "native_authentication");
    assert_eq!(calls[4].payload["scope"], "collaboration");
    drop(calls);
    let script = Script::new(vec![
        ("ScanAgents", scan("agent_codex_default", true)),
        (
            "PreviewAgentConnectionChange",
            preview(
                spec("agent-context/root", "explicit"),
                &["skill_loading"],
                false,
            ),
        ),
        (
            "CheckAgentConnection",
            Ok(MachineEnvelopeV2::failed(
                ErrorV1::new(ErrorCode::CapabilityDenied),
                Some("request".into()),
            )),
        ),
    ]);
    assert_eq!(
        run(
            &script,
            &["configure", "agent_codex_default", "--trigger", "explicit"]
        )
        .status,
        MachineStatus::Denied
    );
}

#[test]
fn uncertain_delivery_replays_the_frozen_request_and_changed_intent_conflicts_before_checks() {
    for submission in [
        SubmissionState::NotSent,
        SubmissionState::MayHaveReachedServer,
    ] {
        let directory = test_directory();
        let runtime = directory.path().join("run");
        std::fs::create_dir(&runtime).unwrap();
        let state = directory.path().join("state");
        let endpoint = runtime.join("hiroute/control.sock");
        let receipts = receipt::root(&state, &endpoint).unwrap();
        assert_ne!(
            receipts,
            receipt::root(&state, &directory.path().join("other/control.sock")).unwrap()
        );
        let script = Script::new(vec![
            ("ScanAgents", scan("agent_codex_default", true)),
            (
                "PreviewAgentConnectionChange",
                preview(spec("agent-context/root", "explicit"), &[], false),
            ),
            (
                "ApplyAgentConnectionChange",
                Err(ClientFailure {
                    code: FailureCode::Deadline,
                    submission,
                }),
            ),
            ("ApplyAgentConnectionChange", accepted()),
        ]);
        let args = [
            "configure",
            "agent_codex_default",
            "--trigger",
            "explicit",
            "--idempotency-key",
            "retry1",
        ];
        let first = flow::run(&script, &receipts, invocation(&args), "request".into());
        assert_eq!(first.status, MachineStatus::Unavailable);
        assert_eq!(
            first.next_actions[0].reason_code,
            if submission == SubmissionState::NotSent {
                "SUBMISSION_NOT_SENT"
            } else {
                "SUBMISSION_UNKNOWN"
            }
        );
        let conflicting = flow::run(
            &script,
            &receipts,
            invocation(&[
                "configure",
                "agent_codex_default",
                "--trigger",
                "delegate-by-default",
                "--idempotency-key",
                "retry1",
            ]),
            "request".into(),
        );
        assert_eq!(
            conflicting.error.unwrap().code,
            ErrorCode::IdempotencyKeyReused
        );
        // A logout/reboot clears sockets, not the immutable submission. Reopening the same
        // endpoint must return the original operation without rescanning or new digests.
        std::fs::remove_dir_all(&runtime).unwrap();
        std::fs::create_dir(&runtime).unwrap();
        let reopened = receipt::root(&state, &endpoint).unwrap();
        assert_eq!(receipts, reopened);
        let retry = flow::run(&script, &reopened, invocation(&args), "request".into());
        assert_eq!(retry.status, MachineStatus::Accepted);
        let calls = script.calls.borrow();
        assert_eq!(calls[2].payload, calls[3].payload);
        script.done();
    }
}

#[test]
fn disable_reads_the_root_collaboration_restore_point_and_keeps_the_model() {
    let status = json!({"schema":"hiroute.agent-collaboration-only-settings-status/v2", "context_id":"agent-context/root", "collaboration":{"schema":"hiroute.agent-collaboration-settings-status/v2", "state":"configured", "operation_id":null,"operation_state":null,"restore_point_ref":"restore/skill"}});
    let restore = json!({"schema_version":{"major":2,"minor":0},"context_id":"agent-context/root","model":{"intent":"keep"},"access_token":{"intent":"keep"},"collaboration":{"intent":"restore","restore_point_ref":"restore/skill"}});
    let script = Script::new(vec![
        ("ScanAgents", scan("agent_codex_default", true)),
        ("GetAgentConnectionStatus", succeeded(status)),
        (
            "PreviewAgentConnectionRestore",
            preview(restore, &[], false),
        ),
        ("ApplyAgentConnectionRestore", accepted()),
    ]);
    assert_eq!(
        run(&script, &["disable", "agent_codex_default", "--yes"]).status,
        MachineStatus::Accepted
    );
    assert_eq!(
        script.calls.borrow()[3].payload["spec"]["model"]["intent"],
        "keep"
    );
    assert_eq!(
        script.calls.borrow()[3].payload["spec"]["collaboration"]["restore_point_ref"],
        "restore/skill"
    );
}

#[test]
fn invalid_or_unconfirmed_arguments_are_rejected_without_transport() {
    for arguments in [
        vec!["disable", "agent_codex_default"],
        vec!["configure", "agent_codex_default"],
        vec!["configure", "agent_codex_default", "--trigger", "live"],
        vec![
            "configure",
            "agent_codex_default",
            "--trigger",
            "explicit",
            "--preview",
            "--idempotency-key",
            "k",
        ],
        vec!["status", "agent_codex_default", "--yes"],
        vec![
            "configure",
            "agent_codex_default",
            "--trigger",
            "explicit",
            "--trigger",
            "explicit",
        ],
    ] {
        assert!(
            parse(
                &arguments
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
            )
            .is_err(),
            "{arguments:?}"
        );
    }
    let (arguments, globals) = crate::args::split_globals(
        [
            "agents",
            "collaboration",
            "configure",
            "agent_codex_default",
            "--trigger",
            "explicit",
            "--idempotency-key",
            "--help",
        ]
        .map(str::to_owned)
        .to_vec(),
    )
    .unwrap();
    assert!(!globals.help);
    assert!(parse(&arguments[2..]).is_err());
    for key in ["retry/1", "retry 1", "重试", "", "--help"] {
        // Exercise the real argv entry, so invalid keys cannot reach environment/transport,
        // native prerequisites or receipt creation (even when no daemon is available).
        let execution = crate::execute([
            "agents",
            "collaboration",
            "configure",
            "agent_codex_default",
            "--trigger",
            "explicit",
            "--idempotency-key",
            key,
        ]);
        let envelope: MachineEnvelopeV2<Value> = serde_json::from_str(&execution.stdout).unwrap();
        assert_eq!(envelope.error.unwrap().code, ErrorCode::InvalidArguments);
    }
    for (length, valid) in [(200, true), (201, false)] {
        let key = "a".repeat(length);
        let arguments = [
            "configure",
            "agent_codex_default",
            "--trigger",
            "explicit",
            "--idempotency-key",
            &key,
        ];
        assert_eq!(parse(&arguments.map(str::to_owned)).is_ok(), valid);
    }
}

#[test]
fn concurrent_submissions_preserve_the_first_frozen_request_and_isolate_endpoints() {
    let directory = test_directory();
    let state = directory.path().join("state");
    let receipts = receipt::root(&state, &directory.path().join("one/control.sock")).unwrap();
    let selection =
        invocation(&["configure", "agent_codex_default", "--trigger", "explicit"]).selection;
    let make_apply = |digest: &[u8]| AgentSettingsApplyV2 {
        spec: serde_json::from_value(spec("agent-context/root", "explicit")).unwrap(),
        accept_digest: hiroute_application_api::CanonicalDigest::of_bytes(digest),
        dependency_digest: hiroute_application_api::CanonicalDigest::of_bytes(digest),
        expected_revisions: hiroute_application_api::RevisionSetV1 {
            target: 0,
            dependencies: Default::default(),
        },
        idempotency_key: "concurrent".into(),
        login_item: None,
    };
    let barrier = std::sync::Barrier::new(2);
    let winners = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            receipt::freeze(&receipts, &selection, make_apply(b"first")).unwrap()
        });
        let second = scope.spawn(|| {
            barrier.wait();
            receipt::freeze(&receipts, &selection, make_apply(b"second")).unwrap()
        });
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(winners[0], winners[1]);
    assert_eq!(
        receipt::load(&receipts, "concurrent", &selection).unwrap(),
        Some(winners[0].clone())
    );
    let other = receipt::root(&state, &directory.path().join("two/control.sock")).unwrap();
    assert!(
        receipt::load(&other, "concurrent", &selection)
            .unwrap()
            .is_none()
    );
    // Moving a record across endpoint namespaces does not grant replay authority.
    std::fs::create_dir_all(&other).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let record = std::fs::read_dir(&receipts)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::copy(&record, other.join(record.file_name().unwrap())).unwrap();
    assert_eq!(
        receipt::load(&other, "concurrent", &selection).unwrap_err(),
        ErrorCode::DaemonUnavailable
    );
}

#[test]
fn corrupted_or_nonprivate_receipts_never_reconstruct_an_apply() {
    let directory = test_directory();
    let receipts = receipt_root(&directory);
    let script = Script::new(vec![
        ("ScanAgents", scan("agent_codex_default", true)),
        (
            "PreviewAgentConnectionChange",
            preview(spec("agent-context/root", "explicit"), &[], false),
        ),
        ("ApplyAgentConnectionChange", accepted()),
    ]);
    let args = [
        "configure",
        "agent_codex_default",
        "--trigger",
        "explicit",
        "--idempotency-key",
        "corrupt",
    ];
    let initial = flow::run(&script, &receipts, invocation(&args), "request".into());
    assert_eq!(initial.status, MachineStatus::Accepted);
    let path = std::fs::read_dir(&receipts)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(directory.path()), 0o755);
        for private_directory in [receipts.parent().unwrap(), receipts.as_path()] {
            assert_eq!(mode(private_directory), 0o700);
            std::fs::set_permissions(private_directory, std::fs::Permissions::from_mode(0o755))
                .unwrap();
            // A nonprivate namespace or endpoint must reject replay before any Control
            // call and preserve the existing permissions instead of repairing them.
            let replay = flow::run(&script, &receipts, invocation(&args), "request".into());
            assert_eq!(replay.status, MachineStatus::Unavailable);
            assert_eq!(replay.error.unwrap().code, ErrorCode::DaemonUnavailable);
            assert_eq!(mode(private_directory), 0o755);
            std::fs::set_permissions(private_directory, std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            flow::run(&script, &receipts, invocation(&args), "request".into()).status,
            MachineStatus::Unavailable
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(&path, b"incomplete").unwrap();
    assert_eq!(
        flow::run(&script, &receipts, invocation(&args), "request".into()).status,
        MachineStatus::Unavailable
    );
    script.done();
}
