//! Native receipt regressions through the production freeze/load and replay flow.
//! Control is recorded only to prove that a rejected receipt cannot trigger a
//! fresh discovery, Preview, or submission. Files and ACLs are actual Windows objects.
use std::cell::RefCell;
use std::path::{Path, PathBuf};

use hiroute_application_api::{
    AgentSettingsApplyV2, CanonicalDigest, ErrorCode, LocalControlWireRequestV2, MachineEnvelopeV2,
    MachineStatus, RevisionSetV1,
};
use hiroute_client_core::ClientFailure;
use hiroute_platform::{file_security, windows::security};
use hiroute_platform_test_support::{grant_everyone_read, junction};
use serde_json::{Value, json};

use super::{Invocation, Selection, flow, parse, receipt};

const KEY: &str = "native-receipt-retry";

fn invocation(key: &str, trigger: &str) -> Invocation {
    parse(
        &[
            "configure",
            "agent_codex_default",
            "--trigger",
            trigger,
            "--idempotency-key",
            key,
        ]
        .map(str::to_owned),
    )
    .unwrap()
}

fn apply(key: &str, trigger: &str, preview: &[u8]) -> AgentSettingsApplyV2 {
    let super::Intent::Configure { trigger_mode } = invocation(key, trigger).selection.intent
    else {
        panic!("configure fixture")
    };
    AgentSettingsApplyV2 {
        spec: serde_json::from_value(json!({
            "schema_version":{"major":2,"minor":0},
            "context_id":"agent-context/root",
            "model":{"intent":"keep"},
            "access_token":{"intent":"keep"},
            "collaboration":{"intent":"configure","settings":{"trigger_mode":trigger_mode}}
        }))
        .unwrap(),
        accept_digest: CanonicalDigest::of_bytes(preview),
        dependency_digest: CanonicalDigest::of_bytes(preview),
        expected_revisions: RevisionSetV1 {
            target: 0,
            dependencies: Default::default(),
        },
        idempotency_key: key.into(),
        login_item: None,
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    root: PathBuf,
    selection: Selection,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = receipt::root(
            &directory.path().join("state"),
            &directory.path().join("run/control.sock"),
        )
        .unwrap();
        Self {
            directory,
            root,
            selection: invocation(KEY, "explicit").selection,
        }
    }

    fn freeze(&self) -> AgentSettingsApplyV2 {
        receipt::freeze(
            &self.root,
            &self.selection,
            apply(KEY, "explicit", b"first-preview"),
        )
        .unwrap()
    }

    fn record(&self) -> PathBuf {
        let entries: Vec<_> = std::fs::read_dir(&self.root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "one frozen receipt and no unpublished stages"
        );
        entries.into_iter().next().unwrap()
    }
}

#[derive(Default)]
struct ReplayControl(RefCell<Vec<LocalControlWireRequestV2>>);

impl flow::Control for ReplayControl {
    fn call(
        &self,
        request: LocalControlWireRequestV2,
    ) -> Result<MachineEnvelopeV2<Value>, ClientFailure> {
        assert_eq!(request.operation_id, "ApplyAgentConnectionChange");
        assert!(request.protected_grant.is_none());
        self.0.borrow_mut().push(request);
        let mut response = MachineEnvelopeV2::succeeded(
            json!({"operation_id":"operation/native-replay"}),
            Some("request".into()),
        );
        response.status = MachineStatus::Accepted;
        Ok(response)
    }
}

struct NoControl;
impl flow::Control for NoControl {
    fn call(
        &self,
        request: LocalControlWireRequestV2,
    ) -> Result<MachineEnvelopeV2<Value>, ClientFailure> {
        panic!("rejected receipt reached Control: {}", request.operation_id);
    }
}

fn rejected_replay(root: &Path, expected: ErrorCode, trigger: &str) {
    let response = flow::run(&NoControl, root, invocation(KEY, trigger), "request".into());
    assert_eq!(response.error.unwrap().code, expected);
    assert_eq!(
        response.status,
        if expected == ErrorCode::IdempotencyKeyReused {
            MachineStatus::Conflict
        } else {
            MachineStatus::Unavailable
        }
    );
}

#[test]
fn native_concurrent_freeze_replays_one_exact_apply_and_isolates_endpoints() {
    let fixture = Fixture::new();
    let candidates: Vec<_> = (0..4)
        .map(|writer| apply(KEY, "explicit", format!("preview/{writer}").as_bytes()))
        .collect();
    let barrier = std::sync::Barrier::new(candidates.len());
    let winners = std::thread::scope(|scope| {
        candidates
            .iter()
            .map(|candidate| {
                let barrier = &barrier;
                let fixture = &fixture;
                scope.spawn(move || {
                    barrier.wait();
                    receipt::freeze(&fixture.root, &fixture.selection, candidate.clone()).unwrap()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(candidates.contains(&winners[0]));
    assert!(winners.iter().all(|winner| winner == &winners[0]));
    assert_eq!(
        receipt::load(&fixture.root, KEY, &fixture.selection).unwrap(),
        Some(winners[0].clone())
    );
    let record = fixture.record();
    let original = std::fs::read(&record).unwrap();
    let held = security::open_owner_only(&record, false).unwrap();
    assert_eq!(security::file_link_count(&held).unwrap(), 1);
    drop(held);
    for directory in [fixture.root.parent().unwrap(), fixture.root.as_path()] {
        security::open_owner_only(directory, true).unwrap();
    }
    let control = ReplayControl::default();
    let response = flow::run(
        &control,
        &fixture.root,
        invocation(KEY, "explicit"),
        "request".into(),
    );
    assert_eq!(response.status, MachineStatus::Accepted);
    assert_eq!(control.0.borrow().len(), 1);
    assert_eq!(
        control.0.borrow()[0].payload,
        serde_json::to_value(&winners[0]).unwrap()
    );
    assert_eq!(std::fs::read(&record).unwrap(), original);

    let other = receipt::root(
        &fixture.directory.path().join("state"),
        &fixture.directory.path().join("other/control.sock"),
    )
    .unwrap();
    assert!(
        receipt::load(&other, KEY, &fixture.selection)
            .unwrap()
            .is_none()
    );
    security::ensure_private_directory(&other).unwrap();
    let copied = other.join(record.file_name().unwrap());
    hiroute_platform::durable::write_private_new(&copied, &original).unwrap();
    assert_eq!(
        receipt::load(&other, KEY, &fixture.selection).unwrap_err(),
        ErrorCode::DaemonUnavailable
    );
    rejected_replay(&other, ErrorCode::DaemonUnavailable, "explicit");
    assert_eq!(std::fs::read(copied).unwrap(), original);
}

#[test]
fn native_wrong_selection_rejects_before_control_and_preserves_frozen_apply() {
    let fixture = Fixture::new();
    let frozen = fixture.freeze();
    let record = fixture.record();
    let original = std::fs::read(&record).unwrap();
    let other = invocation(KEY, "delegate-by-default").selection;
    assert_eq!(
        receipt::load(&fixture.root, KEY, &other).unwrap_err(),
        ErrorCode::IdempotencyKeyReused
    );
    assert_eq!(
        receipt::freeze(
            &fixture.root,
            &other,
            apply(KEY, "delegate-by-default", b"new-preview")
        )
        .unwrap_err(),
        ErrorCode::IdempotencyKeyReused
    );
    rejected_replay(
        &fixture.root,
        ErrorCode::IdempotencyKeyReused,
        "delegate-by-default",
    );
    assert_eq!(std::fs::read(record).unwrap(), original);
    assert_eq!(
        receipt::load(&fixture.root, KEY, &fixture.selection).unwrap(),
        Some(frozen)
    );
}

#[test]
fn native_foreign_receipt_and_namespace_dacls_are_rejected_without_repair() {
    for part in ["record", "endpoint", "namespace"] {
        let fixture = Fixture::new();
        fixture.freeze();
        let record = fixture.record();
        let original = std::fs::read(&record).unwrap();
        let (target, directory) = match part {
            "record" => (record.as_path(), false),
            "endpoint" => (fixture.root.as_path(), true),
            "namespace" => (fixture.root.parent().unwrap(), true),
            _ => unreachable!(),
        };
        grant_everyone_read(target, directory).unwrap();
        let foreign =
            file_security::capture(&security::open_identity(target, directory).unwrap()).unwrap();
        assert!(security::open_owner_only(target, directory).is_err());
        assert_eq!(
            receipt::load(&fixture.root, KEY, &fixture.selection).unwrap_err(),
            ErrorCode::DaemonUnavailable
        );
        rejected_replay(&fixture.root, ErrorCode::DaemonUnavailable, "explicit");
        assert_eq!(
            receipt::freeze(
                &fixture.root,
                &fixture.selection,
                apply(KEY, "explicit", b"retry-preview")
            )
            .unwrap_err(),
            ErrorCode::DaemonUnavailable
        );
        assert_eq!(
            file_security::capture(&security::open_identity(target, directory).unwrap()).unwrap(),
            foreign
        );
        assert_eq!(std::fs::read(record).unwrap(), original);
    }
}

#[test]
fn native_hardlinked_receipt_is_rejected_and_both_aliases_are_preserved() {
    let fixture = Fixture::new();
    fixture.freeze();
    let record = fixture.record();
    let original = std::fs::read(&record).unwrap();
    let alias = fixture.directory.path().join("second-receipt-link.json");
    std::fs::hard_link(&record, &alias).unwrap();
    let held = security::open_owner_only(&record, false).unwrap();
    assert_eq!(security::file_link_count(&held).unwrap(), 2);
    drop(held);
    assert_eq!(
        receipt::load(&fixture.root, KEY, &fixture.selection).unwrap_err(),
        ErrorCode::DaemonUnavailable
    );
    rejected_replay(&fixture.root, ErrorCode::DaemonUnavailable, "explicit");
    assert_eq!(
        receipt::freeze(
            &fixture.root,
            &fixture.selection,
            apply(KEY, "explicit", b"retry-preview")
        )
        .unwrap_err(),
        ErrorCode::DaemonUnavailable
    );
    assert_eq!(std::fs::read(record).unwrap(), original);
    assert_eq!(std::fs::read(alias).unwrap(), original);
}

#[test]
fn native_junction_ancestor_cannot_replay_or_publish_a_receipt() {
    let fixture = Fixture::new();
    fixture.freeze();
    let record = fixture.record();
    let original = std::fs::read(&record).unwrap();
    let namespace = fixture.root.parent().unwrap();
    let moved = fixture.directory.path().join("displaced-namespace");
    std::fs::rename(namespace, &moved).unwrap();
    junction::create(namespace, &moved).unwrap();
    // The lexical root and Record.instance stay identical. An ordinary read
    // succeeds, so rejection must come from the reparse-free admission boundary.
    assert_eq!(std::fs::read(&record).unwrap(), original);
    assert!(security::open_identity(namespace, true).is_err());
    assert_eq!(
        receipt::load(&fixture.root, KEY, &fixture.selection).unwrap_err(),
        ErrorCode::DaemonUnavailable
    );
    rejected_replay(&fixture.root, ErrorCode::DaemonUnavailable, "explicit");
    assert_eq!(
        receipt::freeze(
            &fixture.root,
            &fixture.selection,
            apply("fresh-through-junction", "explicit", b"new-preview")
        )
        .unwrap_err(),
        ErrorCode::DaemonUnavailable
    );
    let physical_root = moved.join(fixture.root.file_name().unwrap());
    assert_eq!(std::fs::read_dir(&physical_root).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(physical_root.join(record.file_name().unwrap())).unwrap(),
        original
    );
    std::fs::remove_dir(namespace).unwrap();
}

#[test]
fn native_corrupt_receipt_never_rebuilds_or_submits_a_new_apply() {
    let fixture = Fixture::new();
    fixture.freeze();
    let record = fixture.record();
    let before =
        security::file_identity(&security::open_identity(&record, false).unwrap()).unwrap();
    let corrupt = b"{interrupted-record";
    std::fs::write(&record, corrupt).unwrap();
    assert_eq!(
        receipt::load(&fixture.root, KEY, &fixture.selection).unwrap_err(),
        ErrorCode::DaemonUnavailable
    );
    rejected_replay(&fixture.root, ErrorCode::DaemonUnavailable, "explicit");
    assert_eq!(
        receipt::freeze(
            &fixture.root,
            &fixture.selection,
            apply(KEY, "explicit", b"fresh-preview")
        )
        .unwrap_err(),
        ErrorCode::DaemonUnavailable
    );
    assert_eq!(std::fs::read(&record).unwrap(), corrupt);
    assert_eq!(
        security::file_identity(&security::open_identity(&record, false).unwrap()).unwrap(),
        before
    );
    assert_eq!(fixture.record(), record);
}
