use std::sync::Arc;

use hiroute_domain::{
    FactsCompleteness, ObservationQueryPort, ReceiptId, RequestOutcome, WorkspaceId,
};
use hiroute_gateway::server::core_runtime::observation::{
    EXECUTION_FACT_PORT_DIGEST, EXECUTION_FACT_SCHEMA,
};
use hiroute_observation::{DigestAuthority, LocalObservationStore, writer::IngestOutcome};
use rusqlite::Connection;
use serde_json::{Value, json};

use super::super::GatewayExecutionFactSink;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn current_credential_denials_reach_real_product_ingestion_with_released_outcomes() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        LocalObservationStore::open(directory.path(), DigestAuthority::new([32; 32])).unwrap(),
    );
    let sink = GatewayExecutionFactSink::new(store);
    for (index, outcome) in [
        "leased",
        "exhausted",
        "authority_error",
        "source_disabled",
        "credential_disabled",
    ]
    .into_iter()
    .enumerate()
    {
        let sequence = u64::try_from(index).unwrap() + 1;
        let mut payload = execution_payload(sequence, &format!("request-{sequence}"), None);
        payload.as_object_mut().unwrap().remove("attempt_id");
        payload["fact"] = json!({
            "kind": "credential_lease", "stable_binding_id": "binding-main", "credential_ref": "credential-main",
            "key_id": if outcome == "leased" { json!("key-main") } else { Value::Null },
            "credential_generation": if outcome == "leased" { json!(1) } else { Value::Null },
            "excluded_key_count": 0, "outcome": outcome
        });
        let result = sink
            .project_and_ingest_payload_for_test(&serde_json::to_vec(&payload).unwrap())
            .unwrap();
        assert!(matches!(result, IngestOutcome::Ack(_)), "outcome {outcome}");
    }
}

#[test]
fn current_credential_denials_close_complete_zero_attempt_receipts_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        LocalObservationStore::open(directory.path(), DigestAuthority::new([33; 32])).unwrap(),
    );
    let sink = GatewayExecutionFactSink::new(store.clone());
    let mut sequence = 0;
    let mut receipts = Vec::new();
    for outcome in ["source_disabled", "credential_disabled"] {
        for stream in [false, true] {
            let request_id = format!("request-{outcome}-{stream}");
            let lease = json!({
                "kind": "credential_lease", "stable_binding_id": "binding-main",
                "credential_ref": "credential-main", "key_id": null,
                "credential_generation": null, "excluded_key_count": 0, "outcome": outcome
            });
            let finished = json!({
                "kind": "request_finished", "outcome": "failed", "attempts_started": 0,
                "attempts_finished": 0, "accepted_attempt_ordinal": null,
                "facts_completeness": "complete"
            });
            for fact in [route_decision(stream), lease.clone(), finished.clone()] {
                sequence += 1;
                let mut payload = execution_payload(sequence, &request_id, None);
                payload.as_object_mut().unwrap().remove("attempt_id");
                payload["fact"] = fact;
                let result = sink
                    .project_and_ingest_payload_for_test(&serde_json::to_vec(&payload).unwrap())
                    .unwrap();
                assert!(matches!(result, IngestOutcome::Ack(_)));
            }
            let receipt_id = ReceiptId::parse(request_id.clone()).unwrap();
            let receipt = store
                .get_receipt(&WorkspaceId::default(), &receipt_id)
                .unwrap();
            receipt.validate().unwrap();
            assert_eq!(receipt.request_id.as_str(), request_id);
            assert_eq!(receipt.outcome, RequestOutcome::Failed);
            assert_eq!(receipt.facts_completeness, FactsCompleteness::Complete);
            assert!(receipt.final_attempt_id.is_none());
            assert_eq!(receipt.ordered_facts.len(), 3);
            assert!(
                receipt
                    .ordered_facts
                    .iter()
                    .all(|event| event.attempt_id.is_none())
            );
            assert_eq!(
                serde_json::to_value(&receipt.ordered_facts[0].fact).unwrap(),
                route_decision(stream)
            );
            assert_eq!(
                serde_json::to_value(&receipt.ordered_facts[1].fact).unwrap(),
                lease
            );
            assert_eq!(
                serde_json::to_value(&receipt.ordered_facts[2].fact).unwrap(),
                finished
            );
            let digest = receipt.digest().unwrap();
            receipts.push((receipt_id, receipt, digest));
        }
    }
    drop(sink);
    drop(store);
    let reopened =
        LocalObservationStore::open(directory.path(), DigestAuthority::new([33; 32])).unwrap();
    for (receipt_id, receipt, digest) in receipts {
        let read = reopened
            .get_receipt(&WorkspaceId::default(), &receipt_id)
            .unwrap();
        assert_eq!(read, receipt);
        assert_eq!(read.digest().unwrap(), digest);
    }
}

#[test]
fn current_gateway_payloads_collect_priced_and_unpriced_attempts_in_one_product_table() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        LocalObservationStore::open(directory.path(), DigestAuthority::new([31; 32])).unwrap(),
    );
    let sink = GatewayExecutionFactSink::new(store);

    for (sequence, request, pricing) in [
        (1, "request-priced", Some(pricing())),
        (2, "request-unpriced", None),
    ] {
        let payload = execution_payload(sequence, request, pricing);
        let outcome = sink
            .project_and_ingest_payload_for_test(&serde_json::to_vec(&payload).unwrap())
            .unwrap();
        assert!(matches!(outcome, IngestOutcome::Ack(_)));
    }

    let connection = Connection::open(directory.path().join("activity.db")).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT request_id, pricing_json IS NOT NULL
             FROM valuation_attempt_inputs_v2 ORDER BY request_id",
        )
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("request-priced".to_owned(), true),
            ("request-unpriced".to_owned(), false),
        ]
    );
}

fn execution_payload(sequence: u64, request_id: &str, pricing: Option<Value>) -> Value {
    json!({
        "schema_version": EXECUTION_FACT_SCHEMA,
        "schema_digest": EXECUTION_FACT_PORT_DIGEST,
        "channel": "execution_fact",
        "producer": {
            "component": "gateway-execution",
            "revision": "hiroute-gateway-observation/1",
            "producer_id": "producer-main",
            "producer_epoch": "epoch-main",
            "stream_id": "stream-main"
        },
        "sequence": sequence,
        "event_id": format!("execution-event-{sequence}"),
        "correlation": {
            "workspace_id": "personal/default",
            "conversation_id": "conversation-main",
            "session_scope": "conversation",
            "correlation_provenance": "agent_supplied",
            "turn_id": "turn-main",
            "request_id": request_id
        },
        "attempt_id": format!("attempt-{sequence}"),
        "authority_id": "authority-main",
        "authority_epoch": 3,
        "served_model_id": "hiroute/coding",
        "selector_source": "trusted_model_alias",
        "agent_plan_id": "plan/coding",
        "route": {"kind": "plan", "revision": 7, "semantic_digest": DIGEST},
        "plan_display_name": "Coding route",
        "gateway_publication_revision": "11",
        "gateway_publication_digest": DIGEST,
        "grant_id": "grant-main",
        "grant_generation": 2,
        "ingress_protocol": "responses",
        "occurred_at_unix_nanos": sequence * 1_000_000,
        "pricing": pricing,
        "fact": {
            "kind": "attempt_started",
            "ordinal": 1,
            "candidate_id": format!("candidate-{sequence}"),
            "stable_binding_id": format!("binding-{sequence}"),
            "profile_digest": DIGEST,
            "credential_ref": format!("credential-{sequence}"),
            "key_id": format!("key-{sequence}"),
            "provider_name": "provider-main",
            "request_model": "native-model-main",
            "upstream_protocol": "responses",
            "model_configuration_id": "model-config-main",
            "adapter_revision": "protocol-adapter/v1",
            "start_reason": "initial_candidate",
            "previous_attempt_id": null
        },
        "loss_watermark": null,
        "completeness_delta": null
    })
}

fn route_decision(stream: bool) -> Value {
    json!({
        "kind": "route_decision", "planner_version": "hiroute-deterministic-planner/v1",
        "plan_id": "plan/coding",
        "route": {"kind": "plan", "revision": 7, "semantic_digest": DIGEST},
        "input_digest": DIGEST, "policy_digest": DIGEST, "output_digest": DIGEST,
        "branch": "custom_exact_order", "complexity": null, "groups": [], "reason_ledger": [],
        "requirements": {
            "text": true, "image_url": false, "image_base64": false, "image_media_types": [],
            "function_tools": false, "strict_tools": false, "parallel_tools": false,
            "tool_choice": {"kind": "auto"}, "tool_result_text": false, "tool_result_json": false,
            "tool_roundtrip": false, "logical_tool_id_mapping": false, "provider_state": false,
            "initial_instructions": false, "mid_conversation_instructions": false,
            "streaming": stream, "stream_text": stream, "stream_reasoning": false,
            "stream_tool_arguments": false, "stream_usage": false, "ingress_protocol": "responses"
        },
        "requested_reasoning_disposition": "absent", "requested_reasoning_value": null,
        "requested_max_output_tokens": null, "stream": stream, "outcome": "ready",
        "outcome_code": null, "max_attempts": 1
    })
}

fn pricing() -> Value {
    json!({
        "schema_version": "hiroute.observation.execution-pricing/v1",
        "request_generation": null,
        "captured_at_ms": 0,
        "attempt_execution_at_ms": 1,
        "quote": null,
        "reference_quote": null,
        "unknown_reason": "snapshot_unavailable"
    })
}
