use super::*;
use hiroute_domain::delegation::RunStateV1;
use serde_json::json;

fn stage(id: &str, assessment: Option<(f64, bool)>, later: bool) -> TrustObservedStageV1 {
    let sample = serde_json::from_value(json!({
        "selection": null, "branch_execution": {"policy":{"name":"Trial", "floor_millis":500, "criteria_digest":null}, "group":"regular", "candidate_index":0}, "upgrade":null,
        "segment_id":id, "session_id":"session/a", "plan_id":"plan/a", "plan_revision":1,
        "selected_branch_id":"smart_saving", "executed_branch_id":"smart_saving", "model_configuration_id":"model/trial", "profile_digest":null, "native_model":"trial", "reasoning_profile_id":null,
        "attribution":"single", "first_turn_id":id, "first_turn_ordinal":1, "last_observed_turn_id":id, "last_observed_turn_ordinal":1,
        "first_at_ms":1000, "last_at_ms":1010, "history_partial":false, "first_request_id":"request/1", "last_request_id":"request/1", "execution_evidence_available":true,
        "task_id":"task/a", "run_id":"run/a",
        "assessment": assessment.map(|(score, partial)| json!({"event_id":"assessment/1", "trigger_request_id":"request/2", "assessed_at_ms":1100, "target_from_turn_id":id, "target_through_turn_id":id, "target_from_ordinal":1, "target_through_ordinal":1, "score":score, "partial":partial, "reason":null, "evidence_available":true}))
    })).unwrap();
    TrustObservedStageV1 {
        sample,
        executor: Some("deepseek_harness".into()),
        has_later_turn: later,
        request_durations_ms: vec![],
        output_tokens_per_second: vec![],
    }
}
#[test]
fn score_states_are_exclusive_and_terminal_completion_does_not_mean_competence() {
    let stages = [
        stage("complete", Some((0.5, false)), false),
        stage("partial", Some((1.0, true)), false),
        stage("waiting", None, false),
        stage("unscored", None, true),
    ];
    let refs = stages.iter().collect::<Vec<_>>();
    let measured = counts(&refs, &BTreeMap::new());
    assert_eq!(
        measured,
        TrustCountsV1 {
            executed: 4,
            complete: 1,
            meets_floor: 1,
            partial: 1,
            awaiting_assessment: 1,
            unscored: 1,
            sessions: 1
        }
    );
    assert_eq!(measured.scoring_coverage(), Some(0.25));
    assert_eq!(measured.competence_rate(), Some(1.0));
    let runs = BTreeMap::from([(
        "run/a".into(),
        TrustRunV1 {
            run_id: "run/a".into(),
            task_id: "task/a".into(),
            state: RunStateV1::Succeeded,
            ordinal: 1,
        },
    )]);
    let terminal = counts(&refs, &runs);
    assert_eq!(terminal.awaiting_assessment, 0);
    assert_eq!(terminal.unscored, 2);
    assert_eq!(terminal.complete, 1);
    assert_eq!(terminal.partial, 1);
}
fn selection(probability: f64, reason: &str, group: &str) -> BranchDecisionV1 {
    serde_json::from_value(json!({"policy":null,"competence_trigger":null,"execution_group":group,"simple_probability":probability,"simple_threshold_millis":800,"selection_reason":reason,"strategy_id":"test", "schema_version":"test", "payload_digest":CanonicalDigest::of_bytes(b"test"),"branch_id":"smart_saving", "complexity_score":null,"threshold":null,"decision_source":"external_classifier","reason_codes":[],"matched_user_phrase_ids":[],"fallback_used":false,"classification_duration_micros":100,"fallback_reason":null})).unwrap()
}
#[test]
fn replay_keeps_equality_protection_and_actual_relay_separate() {
    let mut equality = stage("equal", None, false);
    equality.sample.selection = Some(selection(0.8, "simple_task", "regular"));
    let mut protected = stage("protected", None, false);
    protected.sample.selection = Some(selection(0.99, "low_competence", "primary"));
    protected.sample.branch_execution.as_mut().unwrap().group = ExecutionGroupV1::Primary;
    let mut newly_regular = stage("new", None, false);
    newly_regular.sample.selection = Some(selection(0.72, "complex_task", "primary"));
    newly_regular
        .sample
        .branch_execution
        .as_mut()
        .unwrap()
        .group = ExecutionGroupV1::Primary;
    let mut relayed = equality.clone();
    relayed.sample.branch_execution.as_mut().unwrap().group = ExecutionGroupV1::Primary;
    let result = replay(&[equality, protected, newly_regular, relayed], &[800, 650]);
    assert_eq!(result.included, 3);
    assert_eq!(
        result.excluded_by_reason.get("availability_relay"),
        Some(&1)
    );
    assert_eq!(result.thresholds[0].regular, 1);
    assert_eq!(result.thresholds[1].regular, 2);
    assert_eq!(result.thresholds[1].newly_regular, 1);
    assert_eq!(result.thresholds[1].protected_as_primary, 1);
}
