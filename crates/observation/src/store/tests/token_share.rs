use super::*;
use hiroute_domain::ObservationTokenShareHeadlineV1 as Headline;

fn record(
    store: &LocalObservationStore,
    suffix: &str,
    class: CostClassV1,
    usage: Option<[u64; 4]>,
    agent: &str,
    group: Option<ExecutionGroupV1>,
    unmapped: bool,
) -> Fixture {
    let fixture = Fixture::new(suffix);
    store
        .link_observed_request(&RunObservationLink {
            workspace_id: fixture.workspace.clone(),
            request_id: fixture.request.clone(),
            task_id: format!("task-{suffix}"),
            run_id: format!("run-{suffix}"),
            producer_epoch: "trusted-epoch".into(),
            source_event_id: format!("admission-{suffix}"),
            plan_id: "plan/codex-daily".into(),
            plan_revision: "17".into(),
            publication_ref: "publication".into(),
            harness_id: agent.into(),
            protocol_kind: "responses".into(),
            native_session_id: None,
            native_turn_id: None,
            parent_context_ref: None,
            continued_from_run_id: None,
        })
        .unwrap();
    let mut events = facts(&fixture, UsageFrameKindV1::Cumulative);
    events.retain(|e| {
        !(usage.is_none() && matches!(e.fact, ExecutionFactV1::UsageAndCache { .. })
            || unmapped && matches!(e.fact, ExecutionFactV1::CandidateDecision(_)))
    });
    for e in &mut events {
        if unmapped
            && let ExecutionFactV1::RequestFinished {
                facts_completeness, ..
            } = &mut e.fact
        {
            *facts_completeness = FactsCompleteness::Partial;
        }
        if let ExecutionFactV1::CandidateDecision(candidate) = &mut e.fact {
            candidate.cost_class = class;
        }
        if let (
            Some([input, read, write, output]),
            ExecutionFactV1::UsageAndCache {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                input_provenance,
                output_provenance,
                cache_read_provenance,
                cache_write_provenance,
                cost_class,
                ..
            },
        ) = (usage, &mut e.fact)
        {
            *input_tokens = Some(input + read + write);
            *output_tokens = Some(output);
            *cache_read_tokens = Some(read);
            *cache_write_tokens = Some(write);
            *input_provenance = UsageProvenanceV1::Reported;
            *output_provenance = UsageProvenanceV1::Reported;
            *cache_read_provenance = UsageProvenanceV1::Reported;
            *cache_write_provenance = UsageProvenanceV1::Reported;
            // A runtime cost hint is not the frozen candidate's billing authority.
            *cost_class = Some(CostClassV1::Unknown);
        }
    }
    let turn = ExecutionFactV1::AgentTurnFinished {
        agent_turn_id: format!("agent-turn-{suffix}"),
        segment_id: format!("segment-{suffix}"),
        ordinal: 1,
        plan_id: AgentPlanId::parse("plan/codex-daily").unwrap(),
        plan_revision: 17,
        selected_branch_id: "smart_saving".into(),
        executed_branch_id: Some("smart_saving".into()),
        model_configuration_id: Some("model-a".into()),
        profile_digest: Some(ObservedProfileDigestV1::from(CanonicalDigest::of_bytes(
            b"profile-a",
        ))),
        attribution: AgentTurnAttributionV1::Single,
        started_at_ms: 100,
        finished_at_ms: 120,
        status: AgentTurnStatusV1::Completed,
        history_partial: false,
        first_request_id: Some(fixture.request.clone()),
        last_request_id: Some(fixture.request.clone()),
        branch_execution: group.map(|group| BranchExecutionV1 {
            policy: BranchExecutionPolicyV1 {
                name: "Trial".into(),
                floor_millis: 500,
                criteria_digest: None,
            },
            group,
            candidate_index: 0,
        }),
    };
    for (index, mut event) in events.into_iter().enumerate() {
        event.sequence = index as u64 + 1;
        let result = store.ingest_fact(&event, &[]).unwrap();
        assert!(
            matches!(result, IngestOutcome::Ack(_)),
            "fixture fact was not admitted: {result:?}"
        );
    }
    // Session-history evidence arrives on its own stream, after the Gateway request
    // has finished. Appending it to the frozen request stream is correctly refused.
    let mut stage = Fixture::new(&format!("stage-{suffix}"));
    stage.session = fixture.session.clone();
    let result = store
        .ingest_fact(
            &stage.fact_for_plan(1, turn, 120, "plan/codex-daily", None),
            &[],
        )
        .unwrap();
    assert!(
        matches!(result, IngestOutcome::Ack(_)),
        "turn fact was not admitted: {result:?}"
    );
    fixture
}

fn read(
    store: &LocalObservationStore,
    agent: Option<&str>,
    from: i64,
    to: i64,
    now: i64,
) -> ObservationTokenShareV1 {
    let reader = ObservationReaderContext::local_user(
        WorkspaceId::default(),
        "local".into(),
        1,
        now + 10000,
        false,
        false,
    )
    .unwrap();
    let report = store
        .observed_value_report(
            &reader,
            &ObservationValueQueryV2 {
                from_ms: from,
                to_ms: to,
                session_id: None,
                plan_id: None,
                currency: None,
                token_share: Some(ObservationTokenShareReadV1 {
                    kind: ObservationTokenSharePeriodKindV1::Today,
                    timezone: "local (UTC+08:00)".into(),
                    agent_filter: agent.map(str::to_owned),
                }),
            },
            ValueGroupByV1::None,
            now,
        )
        .unwrap();
    assert!(report.summary.token_share.is_none());
    report.token_share.unwrap()
}

#[test]
fn token_share_matches_the_mixed_day_and_keeps_exclusive_components_and_agent_turns() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    record(
        &store,
        "paid",
        CostClassV1::Paid,
        Some([2_500_000, 300_000, 200_000, 100_000]),
        "codex",
        Some(ExecutionGroupV1::Regular),
        false,
    );
    record(
        &store,
        "subscription",
        CostClassV1::Subscription,
        Some([6_000_000, 400_000, 100_000, 400_000]),
        "codex",
        Some(ExecutionGroupV1::Primary),
        false,
    );
    let report = read(&store, None, 0, 1000, 1000);
    assert_eq!(
        report.headline,
        Headline::Share {
            outside_subscription_ratio_millis: 310,
            known_classified_tokens: 10_000_000,
            unknown_usage_requests: 0
        }
    );
    assert_eq!(report.by_billing_class["paid"].tokens.total, 3_100_000);
    assert_eq!(report.by_billing_class["paid"].tokens.input, 2_500_000);
    assert_eq!(
        report.by_billing_class["paid"].requests, 1,
        "repeated cumulative usage is one attempt"
    );
    assert_eq!(report.by_group["regular"].turns, 1);
    assert_eq!(report.by_group["primary"].turns, 1);
    assert_eq!(report.basis, "input_uncached+cache_read+cache_write+output");
}

#[test]
fn token_share_headline_uses_request_presence_and_recorded_zero_before_token_totals() {
    let cases: Vec<(Vec<(CostClassV1, Option<u64>)>, Headline)> = vec![
        (vec![], Headline::NoRoutedRequests),
        (
            vec![(CostClassV1::Paid, Some(200))],
            Headline::NoSubscriptionInPeriod,
        ),
        (
            vec![
                (CostClassV1::Subscription, None),
                (CostClassV1::Paid, Some(200)),
            ],
            Headline::SubscriptionUsageUnknown {
                unknown_usage_requests: 1,
            },
        ),
        (
            vec![
                (CostClassV1::Subscription, None),
                (CostClassV1::Paid, None),
                (CostClassV1::Free, None),
                (CostClassV1::Unknown, None),
            ],
            Headline::SubscriptionUsageUnknown {
                unknown_usage_requests: 4,
            },
        ),
        (
            vec![(CostClassV1::Unknown, Some(200))],
            Headline::NoComparableUsage,
        ),
        (
            vec![
                (CostClassV1::Subscription, Some(0)),
                (CostClassV1::Paid, Some(200)),
            ],
            Headline::Share {
                outside_subscription_ratio_millis: 1000,
                known_classified_tokens: 200,
                unknown_usage_requests: 0,
            },
        ),
        (
            vec![
                (CostClassV1::Subscription, Some(0)),
                (CostClassV1::Paid, Some(0)),
            ],
            Headline::NoComparableUsage,
        ),
    ];
    for (index, (entries, headline)) in cases.into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let store = open_store(dir.path());
        for (n, (class, usage)) in entries.into_iter().enumerate() {
            record(
                &store,
                &format!("case-{index}-{n}"),
                class,
                usage.map(|n| [n, 0, 0, 0]),
                "codex",
                None,
                false,
            );
        }
        assert_eq!(
            read(&store, None, 0, 1000, 1000).headline,
            headline,
            "case {index}"
        );
    }
}

#[test]
fn token_share_unknown_usage_and_unmapped_billing_are_separate_and_agent_filter_is_exact() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    record(
        &store,
        "sub",
        CostClassV1::Subscription,
        Some([100, 0, 0, 0]),
        "codex",
        None,
        false,
    );
    record(
        &store,
        "paid-unknown",
        CostClassV1::Paid,
        None,
        "codex",
        None,
        false,
    );
    record(
        &store,
        "free",
        CostClassV1::Free,
        Some([100, 0, 0, 0]),
        "codex",
        None,
        false,
    );
    record(
        &store,
        "unmapped",
        CostClassV1::Paid,
        Some([900, 0, 0, 0]),
        "claude-code",
        None,
        true,
    );
    let report = read(&store, None, 0, 1000, 1000);
    assert_eq!(
        report.headline,
        Headline::Share {
            outside_subscription_ratio_millis: 500,
            known_classified_tokens: 200,
            unknown_usage_requests: 1
        }
    );
    assert_eq!(report.by_billing_class["unknown"].tokens.total, 900);
    let filtered = read(&store, Some("codex"), 0, 1000, 1000);
    assert_eq!(filtered.agent_filter.as_deref(), Some("codex"));
    assert_eq!(filtered.by_billing_class["unknown"].requests, 0);
    assert_eq!(
        read(&store, Some("missing-agent"), 0, 1000, 1000).headline,
        Headline::NoRoutedRequests
    );
    assert_eq!(
        read(&store, None, 101, 1000, 1000).headline,
        Headline::NoRoutedRequests,
        "half-open request start scope"
    );
    let clipped = read(
        &store,
        None,
        0,
        8 * SEVEN_DAYS_MILLIS,
        8 * SEVEN_DAYS_MILLIS,
    );
    assert!(clipped.retention_boundary_partial);
    assert!(clipped.disclosure.contains("expired details"));
}

#[test]
fn token_share_unknown_usage_in_each_class_stays_outside_the_known_component_totals() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    for (index, class) in [
        CostClassV1::Subscription,
        CostClassV1::Paid,
        CostClassV1::Free,
        CostClassV1::Unknown,
    ]
    .into_iter()
    .enumerate()
    {
        record(
            &store,
            &format!("known-{index}"),
            class,
            Some([100, 0, 0, 0]),
            "codex",
            None,
            false,
        );
        record(
            &store,
            &format!("unknown-{index}"),
            class,
            None,
            "codex",
            None,
            false,
        );
    }
    let report = read(&store, None, 0, 1000, 1000);
    assert_eq!(
        report.headline,
        Headline::Share {
            outside_subscription_ratio_millis: 667,
            known_classified_tokens: 300,
            unknown_usage_requests: 4
        }
    );
    for class in ["subscription", "paid", "free", "unknown"] {
        assert_eq!(report.by_billing_class[class].requests, 2);
        assert_eq!(report.by_billing_class[class].tokens.total, 100);
    }
}

#[test]
fn token_share_counts_failed_attempt_consumption_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let fixture = Fixture::new("failed-consumption");
    let mut events = facts(&fixture, UsageFrameKindV1::Cumulative);
    for event in &mut events {
        match &mut event.fact {
            ExecutionFactV1::AttemptFinished(attempt) => {
                attempt.outcome = AttemptOutcomeV1::PostcommitTransportFailed;
                attempt.downstream_outcome = AttemptDownstreamOutcomeV1::Failed;
                attempt.stream_outcome = AttemptStreamOutcomeV1::StreamStartedNoRetry;
                attempt.termination_reason = AttemptTerminationReasonV1::StreamStartedNoRetry;
                attempt.error_class = Some("stream_started_no_retry".into());
                attempt.retryable = Some(false);
            }
            ExecutionFactV1::RequestFinished { outcome, .. } => {
                *outcome = ExecutionRequestOutcomeV1::PostcommitTransportFailed
            }
            _ => {}
        }
        let result = store.ingest_fact(event, &[]).unwrap();
        assert!(matches!(result, IngestOutcome::Ack(_)), "{result:?}");
    }
    let report = read(&store, None, 0, 1000, 1000);
    assert_eq!(report.by_billing_class["subscription"].requests, 1);
    assert_eq!(report.by_billing_class["subscription"].tokens.total, 125);
    assert_eq!(report.unknown_usage_requests, 0);
}
