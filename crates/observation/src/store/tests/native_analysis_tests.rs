use super::*;

fn native(store: &LocalObservationStore, id: &str) -> Fixture {
    let f = Fixture::new(id);
    for e in facts(&f, UsageFrameKindV1::Cumulative) {
        store.ingest_fact(&e, &[]).unwrap();
    }
    f
}
fn event(f: &Fixture, ids: Vec<String>, seq: u64) -> NativeContinuationEvidenceV1 {
    NativeContinuationEvidenceV1 {
        schema_version: NATIVE_CONTINUATION_SCHEMA_V1.into(),
        workspace_id: f.workspace.to_string(),
        request_id: f.request.to_string(),
        current_session_id: f.session.to_string(),
        ids,
        complete: true,
        proof_observation: Some(NativeProofObservationV1 {
            observer_id: "native-observer".into(),
            sequence: seq,
            observed_at_ms: seq as i64,
            age_ms: 0,
        }),
        current_state: None,
        invalid_edges: None,
        ingress_at_ms: 1000,
        producer_id: "test-producer".into(),
        producer_epoch: "test-epoch".into(),
        stream_id: "test-stream".into(),
        sequence: seq,
        event_id: format!("native-event-{}-{seq}", f.request),
    }
}
fn page(store: &LocalObservationStore) -> ObservationAnalysisPageV1 {
    let reader = ObservationReaderContext::local_user(
        WorkspaceId::default(),
        "native-test".into(),
        1,
        10_000,
        false,
        false,
    )
    .unwrap();
    let range = AnalysisTimeRangeV1 {
        requested_from_ms: 0,
        requested_to_ms: 1000,
        effective_from_ms: 0,
        effective_to_ms: 1000,
        from_utc_offset_seconds: 0,
        to_utc_offset_seconds: 0,
        clipped_for_retention: false,
    };
    store
        .observed_analysis(
            &reader,
            &ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations),
            Some(&range),
            None,
            1000,
        )
        .unwrap()
}

#[test]
fn cached_ancestor_proof_cannot_override_parent_reopen_but_fresh_verification_can() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let a = native(&store, "native-a");
    let b = native(&store, "native-b");
    let proof = event(&b, vec![a.session.to_string(), b.session.to_string()], 1);
    store.ingest_native_continuation(&proof).unwrap();
    assert_eq!(page(&store).conversations.len(), 1);
    let mut reopened = event(&a, vec![a.session.to_string()], 2);
    reopened.current_state = Some(NativeCurrentStateV1 {
        value: NativeSessionStateV1::Open,
        observation: reopened.proof_observation.clone().unwrap(),
    });
    store.ingest_native_continuation(&reopened).unwrap();
    assert_eq!(page(&store).conversations.len(), 2);
    let mut cached = proof.clone();
    cached.event_id = "another-http-request".into();
    cached.sequence = 3;
    cached.ingress_at_ms = 2000;
    cached.proof_observation.as_mut().unwrap().age_ms = 5000;
    store.ingest_native_continuation(&cached).unwrap();
    assert_eq!(page(&store).conversations.len(), 2);
    let verified = event(&b, proof.ids, 4);
    store.ingest_native_continuation(&verified).unwrap();
    assert_eq!(page(&store).conversations.len(), 1);
}

#[test]
fn edge_ordering_is_independent_of_unrelated_observer_sequences_and_cached_replay() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let a = native(&store, "native-order-a");
    let b = native(&store, "native-order-b");
    let c = native(&store, "native-order-c");
    let d = native(&store, "native-order-d");
    let first = event(&b, vec![a.session.to_string(), b.session.to_string()], 100);
    store.ingest_native_continuation(&first).unwrap();
    let other = event(&d, vec![c.session.to_string(), d.session.to_string()], 1);
    store.ingest_native_continuation(&other).unwrap();
    assert_eq!(page(&store).conversations.len(), 2);
    let newer = event(&b, first.ids.clone(), 200);
    store.ingest_native_continuation(&newer).unwrap();
    let mut stale = event(&a, vec![a.session.to_string()], 150);
    stale.invalid_edges = Some(NativeInvalidEdgesV1 {
        edges: vec![[a.session.to_string(), b.session.to_string()]],
        observation: stale.proof_observation.clone().unwrap(),
    });
    store.ingest_native_continuation(&stale).unwrap();
    assert_eq!(
        page(&store).conversations.len(),
        2,
        "a newer verified edge survives older negative delivery"
    );
}

#[test]
fn a_deleted_middle_member_cannot_join_ancestors_and_descendants_before_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let a = native(&store, "native-delete-a");
    let b = native(&store, "native-delete-b");
    let c = native(&store, "native-delete-c");
    store
        .ingest_native_continuation(&event(
            &c,
            vec![
                a.session.to_string(),
                b.session.to_string(),
                c.session.to_string(),
            ],
            1,
        ))
        .unwrap();
    assert_eq!(page(&store).conversations.len(), 1);
    // Model the retained source state left by a compatible old full deletion:
    // no new projection cleanup or candidate serialization is invoked here.
    let db = store.connection.lock();
    db.execute(
        "DELETE FROM logical_requests WHERE workspace_id=?1 AND session_id=?2",
        rusqlite::params![b.workspace.as_str(), b.session.as_str()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO observation_session_barriers_v2 VALUES(?1,?2,1000)",
        rusqlite::params![b.workspace.as_str(), b.session.as_str()],
    )
    .unwrap();
    // No typed delete evidence remains. This is ancestry-unavailable, not a
    // claim that a generic retention barrier proves a user deletion.
    drop(db);
    let after = page(&store);
    assert_eq!(after.conversations.len(), 2);
    assert!(after.conversations.iter().any(|s| {
        s.members
            .iter()
            .any(|m| m.gap_before.as_deref() == Some("predecessor_unavailable"))
    }));
}

#[test]
fn missing_native_proof_is_a_gap_and_does_not_prevent_requests_or_fee_queries() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let a = native(&store, "native-unobserved");
    let mut missing = event(&a, vec![a.session.to_string()], 1);
    missing.complete = false;
    missing.proof_observation = None;
    store.ingest_native_continuation(&missing).unwrap();
    let before = page(&store);
    assert_eq!(before.conversations.len(), 1);
    assert!(before.conversations[0].lineage_partial);
    assert_eq!(
        before.conversations[0].members[0].gap_before.as_deref(),
        Some("predecessor_unknown")
    );
    store
        .ingest_native_continuation(&event(&a, vec![a.session.to_string()], 2))
        .unwrap();
    assert!(!page(&store).conversations[0].lineage_partial);
}
