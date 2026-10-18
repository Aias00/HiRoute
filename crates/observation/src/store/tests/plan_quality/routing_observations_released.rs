//! Exact v0.3.1 producer bytes, authenticated before refreshing rebuildable safe evidence.
use super::*;
use crate::store::tests::support::route_decision;
use hiroute_domain::*;
use rusqlite::params;

fn released() -> (ExecutionFactEnvelopeV1, CanonicalDigest) {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/released-v0.3.1-route-decision.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["source_revision"],
        "13856abd873da5b4373d39322493b4cc105c4867"
    );
    let envelope: ExecutionFactEnvelopeV1 =
        serde_json::from_value(fixture["envelope"].clone()).unwrap();
    let digest = CanonicalDigest::parse(fixture["envelope_digest"].as_str().unwrap()).unwrap();
    assert_eq!(CanonicalDigest::of(&envelope).unwrap(), digest);
    assert!(envelope.validate().is_err());
    envelope.validate_persisted_contract().unwrap();
    (envelope, digest)
}

fn retain_released(
    store: &crate::LocalObservationStore,
    envelope: &ExecutionFactEnvelopeV1,
    digest: &CanonicalDigest,
) {
    let mut connection = store.connection.lock();
    let tx = connection.transaction().unwrap();
    crate::store::fact_log::insert(&tx, envelope, digest).unwrap();
    crate::receipt::apply_fact(&tx, envelope).unwrap();
    // Reproduce the released derived projection without changing the frozen source carrier.
    tx.execute("UPDATE observation_safe_facts_v2 SET body_json=json_set(json_remove(body_json,'$.pre_applicability_schema_digest','$.routing_context'),'$.occurred_at_ms',NULL) WHERE original_digest=?1",[digest.as_str()]).unwrap();
    // The earlier inline backfill already ran in this store; the new bounded pass must
    // still inspect managed carriers rather than relying on the old cursor or inline JSON.
    tx.execute("INSERT OR REPLACE INTO observation_meta(key,value) VALUES('inline-backfill-fact-rowid','999999')",[]).unwrap();
    tx.commit().unwrap();
}

fn scoped(
    envelope: &ExecutionFactEnvelopeV1,
) -> (ObservationReaderContext, PlanQualitySamplesQuery, i64) {
    let now = envelope.occurred_at_ms().unwrap() + 100;
    let reader = ObservationReaderContext::local_user(
        envelope.correlation.workspace_id.clone(),
        "user".into(),
        1,
        now + 1000,
        false,
        false,
    )
    .unwrap();
    let mut q = query(None);
    q.plan_id = Some(
        envelope
            .trust
            .agent_plan_id
            .as_ref()
            .unwrap()
            .as_str()
            .to_owned(),
    );
    q.from_ms = Some(now - 1000);
    q.to_ms = Some(now + 1);
    q.include_routing_observations = true;
    (reader, q, now)
}

#[test]
fn proven_released_format_keeps_mixed_upgrade_rates_and_safe_deletion_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path());
    let (envelope, digest) = released();
    retain_released(&store, &envelope, &digest);
    let (reader, q, now) = scoped(&envelope);
    let original: String = store
        .connection
        .lock()
        .query_row(
            "SELECT body_json FROM observation_sensitive_payloads_v2 WHERE id=?1",
            [format!("fact:{digest}")],
            |r| r.get(0),
        )
        .unwrap();
    let mut fixture = Fixture::new("proven-upgrade-mixed");
    fixture.workspace = envelope.correlation.workspace_id.clone();
    fixture.session = envelope.correlation.conversation_id.clone();
    let plan = q.plan_id.as_deref().unwrap();
    let mut fact = route_decision(plan);
    let ExecutionFactV1::RouteDecision(route) = &mut fact else {
        unreachable!()
    };
    let decision = route.complexity.as_mut().unwrap();
    decision.decision_protocol = Some(DecisionProtocolV1::SystemOneC);
    decision.routing_observation = Some(super::routing_observations::observation(3));
    let writer = writer(&store);
    let channel = fact_channel(&fixture, 1024 * 1024);
    assert!(matches!(
        offer_fact(
            &writer,
            &channel,
            fixture.fact_for_plan(1, fact, now - 1, plan, None)
        ),
        WriterCycleOutcome::Ack(_)
    ));
    let before = store
        .observed_plan_quality_samples(&reader, &q, now)
        .unwrap()
        .routing_observations
        .unwrap();
    assert_eq!(before.coverage, "partial");
    assert_eq!(before.cohorts[0].counts.suppressed_per_c_decision, None);
    assert!(store.backfill_inline_payloads().unwrap() > 0);
    assert_eq!(
        store.backfill_inline_payloads().unwrap(),
        0,
        "bounded refresh must be idempotent"
    );
    let report = store
        .observed_plan_quality_samples(&reader, &q, now)
        .unwrap()
        .routing_observations
        .unwrap();
    assert_eq!(
        (
            report.retained_decisions,
            report.known_non_c_decisions,
            report.legacy_or_unknown_decisions,
            report.unknown_scope_decisions
        ),
        (2, 1, 0, 0)
    );
    assert_eq!(report.coverage, "complete");
    let counts = &report.cohorts[0].counts;
    assert_eq!(
        (
            counts.c_decisions,
            counts.requested,
            counts.gate_opportunities,
            counts.suppressed
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(counts.suppressed_per_opportunity, Some(1.0));
    assert_eq!(counts.unavailable_per_requested, Some(0.0));
    assert_eq!(
        store
            .connection
            .lock()
            .query_row::<String, _, _>(
                "SELECT body_json FROM observation_sensitive_payloads_v2 WHERE id=?1",
                [format!("fact:{digest}")],
                |r| r.get(0)
            )
            .unwrap(),
        original
    );
    for opt_in in [false, true] {
        let facts = store
            .observed_facts(
                &reader,
                &ObservationFactsQueryV2 {
                    include_routing_observations: opt_in,
                    request_id: envelope.correlation.request_id.clone(),
                    limit: 100,
                    cursor: None,
                },
                now,
            )
            .unwrap();
        assert!(
            serde_json::to_value(&facts).unwrap()["facts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f.get("pre_applicability_schema_digest").is_none())
        );
    }
    let principal = ObservationPrincipalV1::local_user(fixture.workspace.clone());
    let mut spec = SessionDeletionSpecV1 {
        workspace_id: fixture.workspace.clone(),
        session_id: fixture.session.clone(),
        data_class: DeletionDataClass::ContentOnly,
        delete_rollups: false,
    };
    let preview = store.preview_session_deletion(&principal, &spec).unwrap();
    store
        .apply_session_deletion(
            &principal,
            &spec,
            preview.store_revision,
            &preview.change_digest,
            now,
        )
        .unwrap();
    drop(channel);
    drop(writer);
    drop(store);
    let reopened = open_store(directory.path());
    assert_eq!(
        reopened
            .observed_plan_quality_samples(&reader, &q, now)
            .unwrap()
            .routing_observations
            .unwrap(),
        report
    );
    let mut authenticated = envelope.clone();
    // Omission is still omission: no marker or fabricated model judgment was written.
    let ExecutionFactV1::RouteDecision(route) = &mut authenticated.fact else {
        unreachable!()
    };
    assert!(
        route
            .complexity
            .as_ref()
            .unwrap()
            .decision_protocol
            .is_none()
    );
    assert_eq!(CanonicalDigest::of(&authenticated).unwrap(), digest);
    spec.data_class = DeletionDataClass::FactsAndContent;
    let preview = reopened
        .preview_session_deletion(&principal, &spec)
        .unwrap();
    reopened
        .apply_session_deletion(
            &principal,
            &spec,
            preview.store_revision,
            &preview.change_digest,
            now + 1,
        )
        .unwrap();
    assert_eq!(
        reopened
            .observed_plan_quality_samples(&reader, &q, now + 1)
            .unwrap()
            .routing_observations
            .unwrap()
            .retained_decisions,
        0
    );
}

#[test]
fn missing_or_corrupt_released_carriers_cannot_create_format_evidence() {
    for missing in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let store = open_store(directory.path());
        let (envelope, digest) = released();
        retain_released(&store, &envelope, &digest);
        let id = format!("fact:{digest}");
        if missing {
            store
                .connection
                .lock()
                .execute(
                    "DELETE FROM observation_sensitive_payloads_v2 WHERE id=?1",
                    [id],
                )
                .unwrap();
        } else {
            store.connection.lock().execute("UPDATE observation_sensitive_payloads_v2 SET body_json=json_set(body_json,'$.schema_digest',?2) WHERE id=?1",params![id,EXECUTION_FACT_PORT_DIGEST_V2]).unwrap();
        }
        store.backfill_inline_payloads().unwrap();
        let (reader, q, now) = scoped(&envelope);
        let summary = store
            .observed_plan_quality_samples(&reader, &q, now)
            .unwrap()
            .routing_observations
            .unwrap();
        assert_eq!(summary.known_non_c_decisions, 0);
        assert_eq!(summary.coverage, "partial");
        assert!(summary.unknown_scope_decisions + summary.legacy_or_unknown_decisions > 0);
    }
}

#[test]
fn evidence_refresh_preserves_released_facts_cursor_and_flag_binding() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path());
    let input: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/released-v0.3.1-route-pagination.json"
    ))
    .unwrap();
    let events: Vec<(ExecutionFactEnvelopeV1, CanonicalDigest)> = input["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let event: ExecutionFactEnvelopeV1 =
                serde_json::from_value(row["envelope"].clone()).unwrap();
            let digest = CanonicalDigest::parse(row["envelope_digest"].as_str().unwrap()).unwrap();
            event.validate_persisted_contract().unwrap();
            assert_eq!(CanonicalDigest::of(&event).unwrap(), digest);
            (event, digest)
        })
        .collect();
    for (event, digest) in &events {
        retain_released(&store, event, digest);
    }
    let (reader, _, now) = scoped(&events[0].0);
    let mut query = ObservationFactsQueryV2 {
        include_routing_observations: false,
        request_id: events[0].0.correlation.request_id.clone(),
        limit: 1,
        cursor: None,
    };
    let first = store.observed_facts(&reader, &query, now).unwrap();
    query.cursor = Some(first.next_cursor.unwrap());
    assert!(store.backfill_inline_payloads().unwrap() > 0);
    let next = store.observed_facts(&reader, &query, now).unwrap();
    assert_eq!(next.facts[0].original_digest, events[1].1.as_str());
    assert!(next.next_cursor.is_none());
    query.include_routing_observations = true;
    assert!(matches!(
        store.observed_facts(&reader, &query, now),
        Err(crate::query_v2::ObservationV2Error::Stale)
    ));
    // Restoring a missing row changes the visible population. Unlike metadata-only
    // enrichment, this must still invalidate a cursor whose insertion watermark predates it.
    query.include_routing_observations = false;
    query.cursor = None;
    let before_repair = store.observed_facts(&reader, &query, now).unwrap();
    query.cursor = before_repair.next_cursor;
    store
        .connection
        .lock()
        .execute(
            "DELETE FROM observation_safe_facts_v2 WHERE original_digest=?1",
            [events[1].1.as_str()],
        )
        .unwrap();
    store
        .connection
        .lock()
        .execute(
            "DELETE FROM observation_meta WHERE key='inline-backfill-fact-protocol-evidence-rowid'",
            [],
        )
        .unwrap();
    assert!(store.backfill_inline_payloads().unwrap() > 0);
    assert!(matches!(
        store.observed_facts(&reader, &query, now),
        Err(crate::query_v2::ObservationV2Error::Stale)
    ));
}
