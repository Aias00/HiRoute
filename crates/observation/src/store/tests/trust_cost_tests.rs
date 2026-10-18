use super::*;

fn trial_request(
    store: &LocalObservationStore,
    suffix: &str,
    revision: u64,
    group: ExecutionGroupV1,
) -> Fixture {
    let f = Fixture::new(suffix);
    let mut all = facts(&f, UsageFrameKindV1::Cumulative);
    let mut terminal = all.pop().unwrap();
    for e in all {
        store.ingest_fact(&e, &[]).unwrap();
    }
    let turn = ExecutionFactV1::AgentTurnFinished {
        branch_execution: Some(BranchExecutionV1 {
            policy: BranchExecutionPolicyV1 {
                name: "Trial".into(),
                floor_millis: 500,
                criteria_digest: None,
            },
            group,
            candidate_index: 0,
        }),
        agent_turn_id: format!("turn-{suffix}"),
        segment_id: format!("stage-{suffix}"),
        ordinal: 1,
        plan_id: AgentPlanId::parse("plan/codex-daily").unwrap(),
        plan_revision: revision,
        selected_branch_id: "smart_saving".into(),
        executed_branch_id: Some("smart_saving".into()),
        model_configuration_id: Some("model-a".into()),
        profile_digest: Some(ObservedProfileDigestV1::from(CanonicalDigest::of_bytes(
            b"profile",
        ))),
        attribution: AgentTurnAttributionV1::Single,
        started_at_ms: 101,
        finished_at_ms: 110,
        status: AgentTurnStatusV1::Completed,
        history_partial: false,
        first_request_id: Some(f.request.clone()),
        last_request_id: Some(f.request.clone()),
    };
    assert!(matches!(
        store.ingest_fact(&f.fact(10, turn, 110), &[]).unwrap(),
        IngestOutcome::Ack(_)
    ));
    terminal.sequence = 11;
    terminal.event_id = EventId::parse("fact-event-11").unwrap();
    terminal.occurred_at_unix_nanos = 111_000_000;
    assert!(matches!(
        store.ingest_fact(&terminal, &[]).unwrap(),
        IngestOutcome::Ack(_)
    ));
    store.settle_pending_valuations(16).unwrap();
    f
}
#[test]
fn trial_comparison_scopes_revision_and_group_and_keeps_missing_prices_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    trial_request(&store, "regular-current", 17, ExecutionGroupV1::Regular);
    trial_request(&store, "primary-current", 17, ExecutionGroupV1::Primary);
    trial_request(&store, "regular-old", 16, ExecutionGroupV1::Regular);
    let scope = TrustCostScopeV1 {
        plan_id: "plan/codex-daily".into(),
        revision: Some(17),
        branch_id: Some("smart_saving".into()),
        group: ExecutionGroupV1::Regular,
        from_ms: 0,
        to_ms: 1000,
        currency: "USD".into(),
    };
    let actual = current_price("actual", "model-a", 1_000_000);
    let reference = current_price("reference", "model-ref", 4_000_000);
    let report = store
        .observed_trust_cost(&reader(), &scope, &reference, &[actual.clone()], 1000)
        .unwrap();
    assert_eq!(report.request_count, 1);
    assert_eq!(report.coverage, ObservationMetricCoverageV2::Complete);
    assert_eq!(
        report.reference_estimate_micros,
        report.routed_estimate_micros.map(|n| n * 4)
    );
    let mut all = scope.clone();
    all.revision = None;
    assert_eq!(
        store
            .observed_trust_cost(&reader(), &all, &reference, &[actual], 1000)
            .unwrap()
            .request_count,
        2
    );
    let expensive = current_price("actual", "model-a", 8_000_000);
    let extra = store
        .observed_trust_cost(&reader(), &scope, &reference, &[expensive], 1000)
        .unwrap();
    assert!(extra.estimated_savings_micros.unwrap() < 0);
    let unknown = store
        .observed_trust_cost(&reader(), &scope, &reference, &[], 1000)
        .unwrap();
    assert_eq!(unknown.missing_request_count, 1);
    assert_eq!(unknown.routed_estimate_micros, None);
    assert_eq!(unknown.estimated_savings_micros, None);
    let f = ingest(&store, "unassigned");
    assert!(
        store
            .observed_trust_cost(&reader(), &scope, &reference, &[], 1000)
            .is_err(),
        "unassigned requests cannot silently vanish from the denominator"
    );
    assert!(
        store
            .connection
            .lock()
            .execute(
                "DELETE FROM logical_requests WHERE workspace_id=?1 AND request_id=?2",
                rusqlite::params![f.workspace.as_str(), f.request.as_str()]
            )
            .is_ok()
    );
}
