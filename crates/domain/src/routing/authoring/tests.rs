use super::*;

fn editor() -> PlanEditorStateV2 {
    serde_json::from_value(serde_json::json!({
        "schema": PLAN_EDITOR_SCHEMA_V2, "display_name": "代码整理", "purpose": "整理代码",
        "mode": "fixed_model", "candidates": [{"binding_id":"binding/a"}],
        "smart": {"economy":[],"primary":[],"judgment":crate::JudgmentSettingsV1::default(),"reselect_on_user_message":false,"classifier":{"kind":"local_rules"},"complex_keywords":[]},
        "free": {"candidates":[],"primary":[],"primary_fallback":false},
        "delegation_enabled": false,
        "requirements":{}, "limits":{"maximum_attempts":6,"request_timeout_ms":60000,"attempt_timeout_ms":30000}
    })).unwrap()
}

#[test]
fn incomplete_parked_modes_survive_and_only_selected_mode_is_published() {
    let mut draft = editor();
    draft.smart.economy = vec![CandidateSelectionV1 {
        binding_id: "".into(),
        reasoning: None,
    }];
    draft.validate_draft().unwrap();
    assert!(draft.effective().is_ok());
    draft.mode = PlanEditorMode::SmartSaving;
    assert!(draft.effective().is_err());
    draft.mode = PlanEditorMode::FixedModel;
    assert_eq!(draft.smart.economy[0].binding_id, "");
    assert!(draft.effective().is_ok());
}

#[test]
fn unfinished_judgment_can_be_saved_but_cannot_be_published() {
    let mut draft = editor();
    draft.mode = PlanEditorMode::SmartSaving;
    draft.smart.economy = draft.candidates.clone();
    draft.smart.primary = draft.candidates.clone();
    draft.smart.judgment.degree.simple.clear();
    draft.smart.judgment.competence.criteria[1].clear();
    draft.validate_draft().unwrap();
    assert!(draft.effective().is_err());
    draft.smart.judgment = Default::default();
    assert!(draft.effective().is_ok());
    draft.smart.judgment.degree.simple = "界".repeat(20_000);
    draft.validate_draft().unwrap();
    let authored = draft.effective().unwrap();
    let AgentPlanStrategyV2::SmartSaving { judgment, .. } = authored.strategy else {
        panic!("smart saving");
    };
    assert_eq!(judgment.degree.simple, draft.smart.judgment.degree.simple);
    draft.smart.judgment.degree.simple = "bad\u{0000}prompt".into();
    assert!(draft.validate_draft().is_err());
}

#[test]
fn fixed_model_keeps_an_explicit_order_and_delegation_requires_a_worker() {
    let mut draft = editor();
    draft.candidates.push(CandidateSelectionV1 {
        binding_id: "binding/b".into(),
        reasoning: None,
    });
    let effective = draft.effective().unwrap();
    assert!(matches!(
        effective.strategy,
        AgentPlanStrategyV2::Custom { candidates }
            if candidates.iter().map(|candidate| candidate.binding_id.as_str()).collect::<Vec<_>>()
                == ["binding/a", "binding/b"]
    ));
    draft.delegation_enabled = true;
    assert!(draft.effective().is_err());
    draft.work = Some(WorkerPlanV1 {
        harness: WorkerHarnessV1::CodexCli,
        protocol: crate::AgentIngressProtocolV1::Responses,
    });
    assert!(draft.effective().is_ok());
}

#[test]
fn smart_and_free_fallback_are_explicit_and_disabled_groups_are_parked() {
    let mut draft = editor();
    draft.mode = PlanEditorMode::FreeFirst;
    draft.free.candidates = draft.candidates.clone();
    draft.free.primary = vec![CandidateSelectionV1 {
        binding_id: "".into(),
        reasoning: None,
    }];
    let effective = draft.effective().unwrap();
    assert!(
        matches!(effective.strategy, AgentPlanStrategyV2::FreeFirst { primary, .. } if primary.is_empty())
    );
    draft.free.primary_fallback = true;
    assert!(draft.effective().is_err());
}

#[test]
fn saved_classifier_survives_editor_to_authoring_projection() {
    let mut draft = editor();
    draft.mode = PlanEditorMode::SmartSaving;
    draft.smart.reselect_on_user_message = true;
    draft.smart.economy = draft.candidates.clone();
    draft.smart.primary = draft.candidates.clone();
    draft.smart.classifier = ComplexityClassifierModeV1::DecisionService {
        service: Box::new(crate::DecisionServiceV1 {
            id: "decision-fixture".into(),
            revision: 1,
            name: "Fixture extension".into(),
            connection: crate::DecisionConnectionV1::Custom {
                endpoint: "https://classifier.example/v1/branch".into(),
                timeout_ms: crate::DEFAULT_REST_CLASSIFIER_TIMEOUT_MS,
                auth_header: Some(crate::ClassifierAuthHeaderV1 {
                    name: "Authorization".into(),
                    value_secret_ref: "classifier/main".into(),
                }),
            },
        }),
    };
    let effective = draft.effective().unwrap();
    assert!(
        effective
            .editor(None)
            .unwrap()
            .smart
            .reselect_on_user_message
    );
    assert!(matches!(
        effective.strategy,
        AgentPlanStrategyV2::SmartSaving {
            classifier: ComplexityClassifierModeV1::DecisionService { service },
            reselect_on_user_message: true,
            ..
        } if service.connection.transport().0 == "https://classifier.example/v1/branch"
    ));
}

#[test]
fn draft_bounds_do_not_require_a_publishable_name_purpose_or_budget() {
    let mut draft = editor();
    draft.display_name.clear();
    draft.purpose.clear();
    draft.candidates[0].reasoning = None;
    draft.validate_draft().unwrap();
    assert!(draft.effective().is_err());
    draft.candidates = vec![draft.candidates[0].clone(); 129];
    assert!(draft.validate_draft().is_err());
}

#[test]
fn context_window_round_trips_without_changing_omitted_legacy_limits() {
    let mut value = editor();
    let old = serde_json::to_value(&value).unwrap();
    assert!(old["limits"].get("context_window_tokens").is_none());
    value.limits.context_window_tokens = Some(500_000);
    let saved: PlanEditorStateV2 =
        serde_json::from_value(serde_json::to_value(&value).unwrap()).unwrap();
    assert_eq!(
        saved.effective().unwrap().limits.context_window_tokens,
        Some(500_000)
    );
    value.limits.context_window_tokens = None;
    assert_eq!(serde_json::to_value(&value).unwrap(), old);
    for invalid in [serde_json::json!(-1), serde_json::json!(1.5)] {
        let mut wire = old.clone();
        wire["limits"]["context_window_tokens"] = invalid;
        assert!(serde_json::from_value::<PlanEditorStateV2>(wire).is_err());
    }
}

#[test]
fn retired_follow_up_field_can_be_omitted_without_rewriting_legacy_values() {
    let released: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/released-plan-editor.v2.json")).unwrap();
    let mut draft: PlanEditorStateV2 = serde_json::from_value(released["editor"].clone()).unwrap();
    draft.validate_draft().unwrap();
    draft.mode = PlanEditorMode::SmartSaving;
    draft.smart.economy = draft.candidates.clone();
    draft.smart.primary = draft.candidates.clone();
    for legacy in [false, true] {
        draft.smart.reselect_on_user_message = legacy;
        let bytes = serde_json::to_vec(&draft).unwrap();
        let reopened: PlanEditorStateV2 = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
        let authored = reopened.effective().unwrap();
        assert_eq!(
            authored
                .editor(None)
                .unwrap()
                .smart
                .reselect_on_user_message,
            legacy
        );
    }
    let mut omitted = serde_json::to_value(&draft).unwrap();
    omitted["smart"]
        .as_object_mut()
        .unwrap()
        .remove("reselect_on_user_message");
    let current: PlanEditorStateV2 = serde_json::from_value(omitted.clone()).unwrap();
    assert!(!current.smart.reselect_on_user_message);
    let mut authoring = serde_json::to_value(current.effective().unwrap()).unwrap();
    authoring["strategy"]
        .as_object_mut()
        .unwrap()
        .remove("reselect_on_user_message");
    let authoring: AgentPlanAuthoringV2 = serde_json::from_value(authoring).unwrap();
    authoring.validate().unwrap();
    assert!(
        !authoring
            .editor(None)
            .unwrap()
            .smart
            .reselect_on_user_message
    );
    omitted["smart"]["reselect_on_user_message"] = serde_json::json!("true");
    assert!(serde_json::from_value::<PlanEditorStateV2>(omitted).is_err());
}

#[test]
fn protocol_transitions_use_choices_without_client_hashes_and_same_protocol_edits_need_none() {
    use crate::*;
    let mut draft = editor();
    draft.mode = PlanEditorMode::SmartSaving;
    draft.smart.economy = draft.candidates.clone();
    draft.smart.primary = draft.candidates.clone();
    draft.smart.classifier = ComplexityClassifierModeV1::DecisionService {
        service: Box::new(DecisionServiceV1 {
            id: "decision/transition".into(),
            revision: 1,
            name: "Transition fixture".into(),
            connection: DecisionConnectionV1::SystemOne {
                provider: "compatible".into(),
                model: "fixture".into(),
                endpoint: "https://decision.example/systemone".into(),
                timeout_ms: 3000,
                auth_header: ClassifierAuthHeaderV1 {
                    name: "Authorization".into(),
                    value_secret_ref: "decision/transition".into(),
                },
            },
        }),
    };
    let old = draft.effective().unwrap();
    draft.smart.native_protocol = Some(NativeProtocolV1::default());
    draft.smart.judgment.degree.simple_threshold_millis = 913;
    draft.smart.judgment.competence.floor_millis = 612;
    let (_, pending) =
        preview_assessment_transition(&draft, Some(&old), None, &[], &Default::default()).unwrap();
    assert!(
        matches!(pending.require_acknowledged(), Err(NativeProtocolError::Review {scope, ..}) if scope == SMART_SAVING_SCOPE_ID)
    );
    let choice = std::collections::BTreeMap::from([(
        SMART_SAVING_SCOPE_ID.into(),
        AssessmentChoiceV1 {
            method: AssessmentAdoptionV1::Stock,
            source: None,
        },
    )]);
    let (adopted, ready) =
        preview_assessment_transition(&draft, Some(&old), None, &[], &choice).unwrap();
    ready.require_acknowledged().unwrap();
    assert_eq!(
        ready.scopes[0].assessment,
        composed_assessment(
            adopted.smart.native_protocol.as_ref(),
            &adopted.smart.judgment
        )
    );
    assert_eq!(adopted.smart.judgment.degree.simple_threshold_millis, 913);
    assert_eq!(adopted.smart.judgment.competence.floor_millis, 612);
    assert!(
        serde_json::to_value(&adopted).unwrap()["smart"]
            .get("assessment_review")
            .is_none()
    );
    let before = adopted.effective().unwrap();
    let mut later = adopted.clone();
    later
        .smart
        .judgment
        .competence
        .instructions
        .push_str(" custom change");
    let (_, same) =
        preview_assessment_transition(&later, Some(&before), None, &[], &Default::default())
            .unwrap();
    assert!(same.scopes.is_empty());
    later.smart.native_protocol = None;
    let (_, rollback) =
        preview_assessment_transition(&later, Some(&before), None, &[], &Default::default())
            .unwrap();
    assert!(rollback.require_acknowledged().is_err());
    let (rolled_back, ready) =
        preview_assessment_transition(&later, Some(&before), None, &[], &choice).unwrap();
    ready.require_acknowledged().unwrap();
    assert_eq!(
        rolled_back.smart.judgment.competence.instructions,
        JudgmentSettingsV1::default().competence.instructions
    );
    let restored = rolled_back.effective().unwrap();
    let mut edit = rolled_back;
    edit.smart
        .judgment
        .competence
        .instructions
        .push_str(" ordinary edit");
    assert!(
        preview_assessment_transition(&edit, Some(&restored), None, &[], &Default::default())
            .unwrap()
            .1
            .scopes
            .is_empty()
    );
    let invalid = std::collections::BTreeMap::from([(
        SMART_SAVING_SCOPE_ID.into(),
        AssessmentChoiceV1 {
            method: AssessmentAdoptionV1::Historical,
            source: Some(HistoricalAssessmentSourceV1 {
                plan: PlanExecutionRef {
                    workspace_id: WorkspaceId::default(),
                    plan_id: AgentPlanId::parse("plan/transition").unwrap(),
                    content_revision: 1,
                    content_digest: CanonicalDigest::of_bytes(b"missing"),
                },
                scope: SMART_SAVING_SCOPE_ID.into(),
            }),
        },
    )]);
    assert!(
        matches!(preview_assessment_transition(&later, Some(&before), None, &[], &invalid), Err(NativeProtocolError::HistoricalUnavailable {scope}) if scope == SMART_SAVING_SCOPE_ID)
    );
}

#[test]
fn independent_category_named_default_cannot_replace_the_inherited_default_scope() {
    use crate::*;
    let mut value = editor();
    value.mode = PlanEditorMode::CustomBranches;
    let mut routing = BranchRoutingV1 {
        native_protocol: Some(NativeProtocolV1::default()),
        classifier: ComplexityClassifierModeV1::DecisionService {
            service: Box::new(DecisionServiceV1 {
                id: "scope-fixture".into(),
                revision: 1,
                name: "Fixture".into(),
                connection: DecisionConnectionV1::SystemOne {
                    provider: "compatible".into(),
                    model: "fixture".into(),
                    endpoint: "https://decision.example/systemone".into(),
                    timeout_ms: 3000,
                    auth_header: ClassifierAuthHeaderV1 {
                        name: "Authorization".into(),
                        value_secret_ref: "scope-fixture".into(),
                    },
                },
            }),
        },
        branches: vec![],
        default_branch_id: "code".into(),
        judgment: JudgmentSettingsV1::default(),
        reselect_on_user_message: false,
    };
    for id in ["default", "code"] {
        routing.branches.push(RouteBranchV1 {
            id: id.into(),
            name: id.into(),
            condition: format!("Handle {id}"),
            candidates: value.candidates.clone(),
            primary_candidates: vec![],
            judgment: (id == "default").then(JudgmentSettingsV1::default),
        });
    }
    value.branch_routing = Some(routing);
    let (_, pending) =
        preview_assessment_transition(&value, None, None, &[], &Default::default()).unwrap();
    assert_eq!(
        pending
            .scopes
            .iter()
            .map(|s| s.scope.as_str())
            .collect::<Vec<_>>(),
        vec!["branch/default", "default"]
    );
    let choices = std::collections::BTreeMap::from([(
        "default".into(),
        AssessmentChoiceV1 {
            method: AssessmentAdoptionV1::Stock,
            source: None,
        },
    )]);
    let (_, pending) = preview_assessment_transition(&value, None, None, &[], &choices).unwrap();
    assert!(
        matches!(pending.require_acknowledged(),Err(NativeProtocolError::Review{scope,..}) if scope=="branch/default")
    );
}
