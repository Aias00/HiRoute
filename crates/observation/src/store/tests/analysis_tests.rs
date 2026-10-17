use super::*;

fn reader() -> ObservationReaderContext {
    ObservationReaderContext::local_user(
        WorkspaceId::default(),
        "local-analysis".into(),
        1,
        20_000,
        true,
        true,
    )
    .unwrap()
}
fn range() -> AnalysisTimeRangeV1 {
    AnalysisTimeRangeV1 {
        requested_from_ms: 0,
        requested_to_ms: 1000,
        effective_from_ms: 0,
        effective_to_ms: 1000,
        from_utc_offset_seconds: 0,
        to_utc_offset_seconds: 0,
        clipped_for_retention: false,
    }
}
fn current_price(binding: &str, model: &str, rate: u64) -> ApiEquivalentPriceV1 {
    let mut quote = price(100, UsageFrameKindV1::Cumulative).quote.unwrap();
    quote.exact_target.valuation_kind = PriceValuationKindV1::ApiEquivalent;
    quote.valuation_kind = PriceValuationKindV1::ApiEquivalent;
    quote.exact_target.model_identity = PriceModelIdentityV1::CatalogModel(model.into());
    quote.rates = TokenRatesV1 {
        input_uncached: TokenRateV1::known(rate),
        output: TokenRateV1::known(rate),
        cache_read: TokenRateV1::known(rate),
        cache_write: TokenRateV1::known(rate),
    };
    quote.quote_digest = quote.computed_digest().unwrap();
    ApiEquivalentPriceV1 {
        binding_id: binding.into(),
        execution_model_configuration_id: None,
        quote,
    }
}
fn ingest(store: &LocalObservationStore, suffix: &str) -> Fixture {
    let f = Fixture::new(suffix);
    for e in facts(&f, UsageFrameKindV1::Cumulative) {
        store.ingest_fact(&e, &[]).unwrap();
    }
    store.settle_pending_valuations(16).unwrap();
    f
}

#[test]
fn analysis_freezes_prices_and_attempt_inputs_and_uses_the_existing_arithmetic() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let f = ingest(&store, "analysis-kernel");
    let actual = current_price("binding-a", "model-a", 1_000_000);
    let reference = current_price("binding-ref", "model-ref", 4_000_000);
    let legacy = store
        .observed_api_equivalent_value(
            &reader(),
            &ObservationValueQueryV2 {
                from_ms: 0,
                to_ms: 1000,
                session_id: None,
                plan_id: None,
                currency: Some("USD".into()),
            },
            &reference,
            std::slice::from_ref(&actual),
            1000,
        )
        .unwrap();
    let capture = AnalysisPricesV1 {
        generation_ref: "test-generation".into(),
        evaluated_at_ms: 1000,
        prices: vec![actual, reference],
        reference_binding_id: Some("binding-ref".into()),
    };
    let mut q = ObservationAnalysisQueryV1::new(AnalysisViewV1::SavingsSummary);
    q.currency = Some("USD".into());
    let page = store
        .observed_analysis(&reader(), &q, Some(&range()), Some(&capture), 1000)
        .unwrap();
    assert_eq!(page.status, AnalysisStatusV1::Ready, "{page:?}");
    let savings = page.savings.unwrap();
    assert_eq!(
        savings.estimated_savings_micros,
        legacy.estimated_savings_micros
    );
    assert_eq!(
        savings.compared_routed_subtotal_micros,
        legacy.compared_routed_subtotal_micros
    );
    assert_eq!(savings.compared_request_count, 1);
    q.analysis_scope = page.analysis_scope;
    q.session_id = Some(f.session.to_string());
    let mut changed = capture;
    changed.prices.clear();
    changed.evaluated_at_ms = 2000;
    let repeated = store
        .observed_analysis(&reader(), &q, None, Some(&changed), 1001)
        .unwrap();
    assert_eq!(
        repeated.savings.unwrap().estimated_savings_micros,
        savings.estimated_savings_micros
    );
    assert_eq!(repeated.price_capture_ref, page.price_capture_ref);
}

#[test]
fn ready_reopen_keeps_checkpoint_and_scopes_do_not_follow_later_insertions() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let f = ingest(&store, "analysis-reopen");
    let q = ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations);
    let page = store
        .observed_analysis(&reader(), &q, Some(&range()), None, 1000)
        .unwrap();
    assert_eq!(page.conversations.len(), 1);
    drop(store);
    let store = open_store(dir.path());
    let queued: u64 = store
        .connection
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM observation_analysis_dirty_v1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(queued, 0);
    ingest(&store, "analysis-later");
    let mut detail = ObservationAnalysisQueryV1::new(AnalysisViewV1::CostRanking);
    detail.analysis_scope = page.analysis_scope;
    detail.currency = Some("USD".into());
    let result = store
        .observed_analysis(&reader(), &detail, None, None, 1001)
        .unwrap();
    assert_ne!(result.status, AnalysisStatusV1::Pending);
    assert_eq!(result.costs.len(), 1);
    assert_eq!(result.costs[0].id, f.session.to_string());
}

#[test]
fn unknown_analysis_format_does_not_reject_core_facts() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    store
        .connection
        .lock()
        .execute(
            "UPDATE observation_analysis_meta_v1 SET value='future' WHERE key='format'",
            [],
        )
        .unwrap();
    drop(store);
    let store = open_store(dir.path());
    ingest(&store, "analysis-future");
    let q = ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations);
    assert_eq!(
        store
            .observed_analysis(&reader(), &q, Some(&range()), None, 1000)
            .unwrap_err(),
        crate::query_v2::ObservationV2Error::Unavailable
    );
}

#[test]
fn auto_selected_price_options_survive_pagination_without_repeating_flags() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    ingest(&store, "auto-price-a");
    ingest(&store, "auto-price-b");
    let capture = AnalysisPricesV1 {
        generation_ref: "auto-generation".into(),
        evaluated_at_ms: 1000,
        prices: vec![current_price("actual", "model-a", 1_000_000)],
        reference_binding_id: None,
    };
    let mut q = ObservationAnalysisQueryV1::new(AnalysisViewV1::CostRanking);
    q.limit = 1;
    q.basis = Some(AnalysisCostBasisV1::ApiEquivalent);
    q.currency = Some("USD".into());
    let first = store
        .observed_analysis(&reader(), &q, Some(&range()), Some(&capture), 1000)
        .unwrap();
    assert_eq!(first.costs.len(), 1);
    q.cursor = first.next_cursor;
    q.currency = None;
    q.basis = None;
    let second = store
        .observed_analysis(&reader(), &q, None, None, 1001)
        .unwrap();
    assert_eq!(second.costs.len(), 1);
    assert_ne!(second.costs[0].id, first.costs[0].id);
    assert_eq!(first.analysis_scope, second.analysis_scope);
    assert_eq!(first.price_capture_ref, second.price_capture_ref);
}

#[test]
fn request_filter_applies_before_conversation_counts_and_member_previews() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let a = ingest(&store, "member-filter-a");
    ingest(&store, "member-filter-b");
    let mut q = ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations);
    q.request_id = Some(a.request.to_string());
    let page = store
        .observed_analysis(&reader(), &q, Some(&range()), None, 1000)
        .unwrap();
    assert_eq!(page.conversations.len(), 1);
    let row = &page.conversations[0];
    assert_eq!(row.request_count, 1);
    assert_eq!(row.member_count, 1);
    assert_eq!(row.members[0].session_id, a.session.to_string());
    assert!(!row.members_partial);
}

#[test]
fn a_recorded_stage_is_evidence_for_a_selected_transition_not_a_switch() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let f = ingest(&store, "recorded-reason");
    store
        .ingest_fact(
            &f.fact(
                11,
                ExecutionFactV1::AgentTurnFinished {
                    branch_execution: None,
                    agent_turn_id: "native-turn-1".into(),
                    segment_id: "stage-1".into(),
                    ordinal: 1,
                    plan_id: AgentPlanId::parse("plan/codex-daily").unwrap(),
                    plan_revision: 1,
                    selected_branch_id: "economy".into(),
                    executed_branch_id: Some("economy".into()),
                    model_configuration_id: Some("model-a".into()),
                    profile_digest: Some(ObservedProfileDigestV1::from(CanonicalDigest::of_bytes(
                        b"profile",
                    ))),
                    attribution: AgentTurnAttributionV1::Single,
                    started_at_ms: 100,
                    finished_at_ms: 110,
                    status: AgentTurnStatusV1::Completed,
                    history_partial: false,
                    first_request_id: Some(f.request.clone()),
                    last_request_id: Some(f.request.clone()),
                },
                120,
            ),
            &[],
        )
        .unwrap();
    let mut q = ObservationAnalysisQueryV1::new(AnalysisViewV1::Routing);
    q.session_id = Some(f.session.to_string());
    let page = store
        .observed_analysis(&reader(), &q, Some(&range()), None, 1000)
        .unwrap();
    assert_eq!(page.transitions.len(), 1);
    let t = &page.transitions[0];
    assert_eq!(t.kind, AnalysisTransitionKindV1::InitialSelection);
    assert_eq!(t.reason.as_ref().unwrap().segment_id, "stage-1");
    assert!(!page.transition_counts.contains_key("between_turn_change"));
}

#[test]
fn numeric_legacy_comparison_preserves_the_original_reader_output() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    for suffix in ["legacy-numeric-a", "legacy-numeric-b", "legacy-numeric-c"] {
        ingest(&store, suffix);
    }
    let q = ObservationValueQueryV2 {
        from_ms: 0,
        to_ms: 1000,
        session_id: None,
        plan_id: None,
        currency: Some("USD".into()),
    };
    let prices = vec![current_price("actual", "model-a", 350_001)];
    let reference = current_price("reference", "another-model", 1_675_531);
    let projected = store
        .observed_api_equivalent_value(&reader(), &q, &reference, &prices, 1000)
        .unwrap();
    store
        .connection
        .lock()
        .execute(
            "UPDATE observation_analysis_meta_v1 SET value='future' WHERE key='format'",
            [],
        )
        .unwrap();
    let original = store
        .observed_api_equivalent_value(&reader(), &q, &reference, &prices, 1000)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&projected).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(projected.compared_request_count, 3);
}

#[test]
fn deleting_and_reusing_the_last_source_row_cannot_replace_a_frozen_population() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(dir.path());
    let a = ingest(&store, "population-old");
    let q = ObservationAnalysisQueryV1::new(AnalysisViewV1::Conversations);
    let first = store
        .observed_analysis(&reader(), &q, Some(&range()), None, 1000)
        .unwrap();
    store
        .connection
        .lock()
        .execute(
            "DELETE FROM logical_requests WHERE workspace_id=?1 AND request_id=?2",
            rusqlite::params![a.workspace.as_str(), a.request.as_str()],
        )
        .unwrap();
    let mut replacement = Fixture::new("population-replacement");
    replacement.session = a.session;
    for e in facts(&replacement, UsageFrameKindV1::Cumulative) {
        store.ingest_fact(&e, &[]).unwrap();
    }
    let mut detail = q;
    detail.analysis_scope = first.analysis_scope;
    assert_eq!(
        store
            .observed_analysis(&reader(), &detail, None, None, 1001)
            .unwrap_err(),
        crate::query_v2::ObservationV2Error::Stale
    );
}
