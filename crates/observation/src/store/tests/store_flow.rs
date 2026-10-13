use std::sync::Arc;

use hiroute_domain::{
    ExecutionFactV1, ObservationNackDetailV1, ObservationQueryError, ObservationQueryPort,
};
use tempfile::tempdir;

use crate::{
    FactChannel, LocalObservationStore, LocalObservationWriter, OfferOutcome, WriterCycleOutcome,
};

use super::support::*;

#[test]
fn receipt_value_and_checkpoint_are_frozen_and_restart_safe() {
    let temporary = tempdir().unwrap();
    let fixture = Fixture::new("receipt");
    let store = open_store(temporary.path());
    let writer = writer(&store);
    let channel = fact_channel(&fixture, 512 * 1024);
    let facts = request_facts(&fixture, "attempt-1", finished("attempt-1"), 100);
    for fact in facts.clone() {
        assert!(matches!(
            offer_fact(&writer, &channel, fact),
            WriterCycleOutcome::Ack(_)
        ));
    }
    let receipt = store
        .get_receipt(
            &fixture.workspace,
            &hiroute_domain::ReceiptId::parse(fixture.request.as_str()).unwrap(),
        )
        .unwrap();
    assert_eq!(receipt.trust.route.plan_revision(), Some(17));
    assert_eq!(receipt.trust.gateway_publication_revision, "41");
    assert_eq!(receipt.trust.grant_generation, 3);
    assert_eq!(receipt.ordered_facts.len(), 10);
    assert!(receipt.ordered_facts.iter().any(|event| matches!(
        &event.fact,
        ExecutionFactV1::AttemptStarted { model_configuration_id, .. }
            if model_configuration_id == "model-a"
    )));
    let value = store
        .get_value(
            &fixture.workspace,
            &value_query("plan/codex-daily", "USD", Some(fixture.session.clone())),
        )
        .unwrap();
    assert_eq!(value.baseline_api_equivalent_cost_micros, Some(1_200));
    assert_eq!(value.chosen_api_equivalent_cost_micros, Some(900));
    assert_eq!(value.actual_incremental_cost_micros, Some(300));
    assert_eq!(value.routing_savings_micros, Some(300));
    assert_eq!(value.entitlement_savings_micros, Some(600));
    assert_eq!(value.estimated_total_savings_micros, Some(900));

    drop(channel);
    drop(writer);
    drop(store);
    let reopened = Arc::new(LocalObservationStore::open(temporary.path(), authority()).unwrap());
    let writer = LocalObservationWriter::new(reopened.clone());
    let replay_channel = FactChannel::new(fixture.fact_stream.clone(), 64 * 1024);
    assert_eq!(
        replay_channel.offer(facts[9].clone()),
        OfferOutcome::Accepted
    );
    let WriterCycleOutcome::Ack(replay_ack) = writer.consume_fact(&replay_channel) else {
        panic!("replayed event must be acknowledged")
    };
    assert_eq!(replay_ack.highest_contiguous_sequence, 10);
    assert_eq!(replay_ack.highest_accounted_sequence, 10);
    let persisted_facts: i64 = reopened
        .connection
        .lock()
        .query_row("SELECT COUNT(*) FROM execution_fact_events", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(persisted_facts, 10, "dedup must not append a second fact");
    assert_eq!(
        reopened
            .get_receipt(
                &fixture.workspace,
                &hiroute_domain::ReceiptId::parse(fixture.request.as_str()).unwrap(),
            )
            .unwrap(),
        receipt
    );
}

#[test]
fn fact_log_tamper_and_unknown_contract_version_fail_closed() {
    let temporary = tempdir().unwrap();
    let fixture = Fixture::new("integrity");
    let store = open_store(temporary.path());
    let writer = writer(&store);
    let channel = fact_channel(&fixture, 512 * 1024);
    let facts = request_facts(
        &fixture,
        "attempt-integrity",
        finished("attempt-integrity"),
        1_000,
    );

    let mut unknown = facts[0].clone();
    unknown.schema_version = "hiroute.observation.execution-fact-envelope/v3".to_owned();
    assert_eq!(channel.offer(unknown), OfferOutcome::Accepted);
    let WriterCycleOutcome::Nack(nack) = writer.consume_fact(&channel) else {
        panic!("an unknown fact version must be rejected")
    };
    assert!(matches!(
        nack.detail,
        ObservationNackDetailV1::UnsupportedSchema { .. }
    ));

    for envelope in facts.iter().cloned() {
        assert!(matches!(
            offer_fact(&writer, &channel, envelope),
            WriterCycleOutcome::Ack(_)
        ));
    }
    let receipt_id = hiroute_domain::ReceiptId::parse(fixture.request.as_str()).unwrap();
    assert!(store.get_receipt(&fixture.workspace, &receipt_id).is_ok());
    {
        let connection = store.connection.lock();
        let body: String = connection
            .query_row(
                "SELECT body_json FROM observation_sensitive_payloads_v2 WHERE id=(SELECT 'fact:'||envelope_digest FROM execution_fact_events WHERE sequence=1)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let tampered = body.replace(
            "\"authority_id\":\"authority-local\"",
            "\"authority_id\":\"authority-tampered\"",
        );
        assert_ne!(tampered, body);
        connection
            .execute(
                "UPDATE observation_sensitive_payloads_v2 SET body_json=?1 WHERE id=(SELECT 'fact:'||envelope_digest FROM execution_fact_events WHERE sequence=1)",
                [tampered],
            )
            .unwrap();
    }
    assert_eq!(
        store
            .get_receipt(&fixture.workspace, &receipt_id)
            .unwrap_err(),
        ObservationQueryError::Corrupt
    );
    assert_eq!(
        offer_fact(&writer, &channel, facts[0].clone()),
        WriterCycleOutcome::StoreFailed(crate::writer::ObservationStoreError::Corrupt),
        "dedup must validate the authoritative raw fact instead of trusting its ACK ledger"
    );
}

#[test]
fn named_tool_request_closes_before_agent_turn_and_reopens_without_digest_drift() {
    use hiroute_domain::{CanonicalDigest, ToolChoiceRequirementV1, ToolKindRequirementV1};

    for tool_kind in [
        ToolKindRequirementV1::Function,
        ToolKindRequirementV1::Custom,
    ] {
        let temporary = tempdir().unwrap();
        let fixture = Fixture::new("named-tool");
        let store = open_store(temporary.path());
        let fact_writer = writer(&store);
        let channel = fact_channel(&fixture, 512 * 1024);
        let mut facts = request_facts(&fixture, "attempt-named", finished("attempt-named"), 100);
        let ExecutionFactV1::RouteDecision(route) = &mut facts[0].fact else {
            unreachable!()
        };
        route.requirements.tool_choice = ToolChoiceRequirementV1::RequiredNamed {
            tool_kind,
            name: "read_number".into(),
        };
        assert!(
            !facts
                .iter()
                .any(|event| matches!(event.fact, ExecutionFactV1::AgentTurnFinished { .. }))
        );
        for fact in facts.clone() {
            assert!(matches!(
                offer_fact(&fact_writer, &channel, fact),
                WriterCycleOutcome::Ack(_)
            ));
        }
        let receipt_id = hiroute_domain::ReceiptId::parse(fixture.request.as_str()).unwrap();
        let receipt = store.get_receipt(&fixture.workspace, &receipt_id).unwrap();
        assert_eq!(receipt.outcome, hiroute_domain::RequestOutcome::Accepted);
        assert_eq!(
            receipt.facts_completeness,
            hiroute_domain::FactsCompleteness::Complete
        );
        assert_eq!(receipt.ordered_facts[0].fact, facts[0].fact);
        let before = receipt.digest().unwrap();
        let original_fact_digest = CanonicalDigest::of(&facts[0]).unwrap();
        drop(channel);
        drop(fact_writer);
        drop(store);
        let reopened = open_store(temporary.path());
        let read = reopened
            .get_receipt(&fixture.workspace, &receipt_id)
            .unwrap();
        assert_eq!(read, receipt);
        assert_eq!(read.digest().unwrap(), before);
        let stored = crate::store::fact_log::load_request(
            &reopened.connection.lock(),
            &fixture.workspace,
            &fixture.request,
        )
        .unwrap();
        assert_eq!(stored, facts);
        assert_eq!(
            CanonicalDigest::of(&stored[0]).unwrap(),
            original_fact_digest
        );
        assert!(matches!(
            offer_fact(
                &writer(&reopened),
                &fact_channel(&fixture, 512 * 1024),
                facts[9].clone()
            ),
            WriterCycleOutcome::Ack(_)
        ));
    }
}

#[test]
fn released_function_only_facts_and_receipt_reopen_alongside_current_writes() {
    use hiroute_domain::{CanonicalDigest, ExecutionFactEnvelopeV1};

    let temporary = tempdir().unwrap();
    let fixture = Fixture::new("released-named-tool");
    let store = open_store(temporary.path());
    let facts = request_facts(&fixture, "attempt-old", finished("attempt-old"), 100);
    let mut released = Vec::new();
    // Freeze only the changed fields from the supported 0.3.0/0.3.1 Product serializer;
    // the unchanged fixture supplies the surrounding valid request and attempt identities.
    for mut bytes in facts.iter().map(|fact| serde_json::to_value(fact).unwrap()) {
        bytes["schema_digest"] = serde_json::json!(
            "sha256:80b4ff13af8abc79d7721fea42141300beade2acf1d2659983ceaafcdc6e4799"
        );
        if bytes["fact"]["kind"] == "route_decision" {
            bytes["fact"]["requirements"]["tool_choice"] =
                serde_json::json!({"kind": "required_named", "name": "read_number"});
        }
        let original_digest = CanonicalDigest::of(&bytes).unwrap();
        let decoded: ExecutionFactEnvelopeV1 = serde_json::from_value(bytes.clone()).unwrap();
        assert!(
            decoded.validate().is_err(),
            "old contracts are recovery only"
        );
        decoded.validate_persisted_contract().unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), bytes);
        assert_eq!(CanonicalDigest::of(&decoded).unwrap(), original_digest);
        released.push((decoded, original_digest));
    }
    // Model retained rows through the existing immutable writer, without admitting the old
    // contract into live ingestion. Reads still authenticate the original row digests.
    {
        let mut connection = store.connection.lock();
        let transaction = connection.transaction().unwrap();
        for (fact, digest) in &released {
            crate::store::fact_log::insert(&transaction, fact, digest).unwrap();
            crate::receipt::apply_fact(&transaction, fact).unwrap();
        }
        transaction.commit().unwrap();
    }
    let receipt_id = hiroute_domain::ReceiptId::parse(fixture.request.as_str()).unwrap();
    let before = store.get_receipt(&fixture.workspace, &receipt_id).unwrap();
    let before_digest = before.digest().unwrap();
    drop(store);
    let reopened = open_store(temporary.path());
    assert_eq!(
        reopened
            .get_receipt(&fixture.workspace, &receipt_id)
            .unwrap(),
        before
    );
    assert_eq!(before.digest().unwrap(), before_digest);
    let old_facts = crate::store::fact_log::load_request(
        &reopened.connection.lock(),
        &fixture.workspace,
        &fixture.request,
    )
    .unwrap();
    for (read, (original, digest)) in old_facts.iter().zip(&released) {
        assert_eq!(read, original);
        assert_eq!(CanonicalDigest::of(read).unwrap(), *digest);
    }
    let current = Fixture::new("current-after-released");
    let fact_writer = writer(&reopened);
    let channel = fact_channel(&current, 512 * 1024);
    for fact in request_facts(&current, "attempt-new", finished("attempt-new"), 200) {
        assert!(matches!(
            offer_fact(&fact_writer, &channel, fact),
            WriterCycleOutcome::Ack(_)
        ));
    }
    assert!(
        reopened
            .get_receipt(
                &current.workspace,
                &hiroute_domain::ReceiptId::parse(current.request.as_str()).unwrap()
            )
            .is_ok()
    );
    assert_eq!(
        reopened
            .get_receipt(&fixture.workspace, &receipt_id)
            .unwrap(),
        before
    );
    assert!(matches!(
        offer_fact(
            &fact_writer,
            &fact_channel(&fixture, 512 * 1024),
            released[0].0.clone()
        ),
        WriterCycleOutcome::Nack(_)
    ));
}
