//! Real native Session -> protected Application -> daemon/store publication path.
use super::*;

fn editor_input(
    action: &str,
    plan: &AgentPlanStatusV2,
    editor: &hiroute_domain::PlanEditorStateV2,
    draft_revision: Option<u64>,
) -> crate::session::EditorInput {
    serde_json::from_value(serde_json::json!({
        "action": action,
        "plan_id": plan.agent_plan_id,
        "draft_id": "draft/native-publication",
        "expected_head_revision": plan.head.head_revision,
        "expected_draft_revision": draft_revision,
        "editor": editor,
        "language": "en"
    }))
    .unwrap()
}

#[tokio::test]
async fn edited_draft_publish_uses_submitted_content_and_consumes_the_exact_draft() {
    let root = prepare();
    let mut session = open(root.path());
    let original = plan(&mut session).await;
    let mut editor = original
        .desired
        .editor(Some(original.model_alias.as_str().into()))
        .unwrap();
    editor.purpose = "saved draft before the final edit".into();
    let draft = session
        .preview_editor(editor_input("save_draft", &original, &editor, None))
        .await
        .unwrap();
    assert!(!draft.requires_confirmation());
    session
        .finish_native_confirmation(draft, true)
        .await
        .unwrap();
    let snapshot = session.snapshot().await.unwrap();
    let draft_revision = snapshot
        .catalog
        .drafts
        .iter()
        .find(|draft| draft.draft_id == "draft/native-publication")
        .unwrap()
        .revision;

    editor.purpose = "content edited immediately before ordinary Publish".into();
    let publication = session
        .preview_editor(editor_input(
            "publish",
            &original,
            &editor,
            Some(draft_revision),
        ))
        .await
        .unwrap();
    assert!(
        !publication.requires_confirmation(),
        "ordinary Publish has no hidden confirmation dialog"
    );
    let outcome = session
        .finish_native_confirmation(publication, true)
        .await
        .unwrap();
    assert_eq!(outcome.operation.unwrap().state, "succeeded");
    let current = plan(&mut session).await;
    assert_eq!(current.desired.purpose.as_str(), editor.purpose);
    assert_eq!(current.desired.strategy, original.desired.strategy);
    assert_eq!(current.model_alias, original.model_alias);
    assert!(
        session
            .snapshot()
            .await
            .unwrap()
            .catalog
            .drafts
            .iter()
            .all(|draft| draft.draft_id != "draft/native-publication")
    );
    drop(session);
    let mut restarted = open(root.path());
    assert_eq!(
        plan(&mut restarted).await.desired.purpose.as_str(),
        editor.purpose
    );
    assert!(
        restarted
            .snapshot()
            .await
            .unwrap()
            .catalog
            .drafts
            .iter()
            .all(|draft| draft.draft_id != "draft/native-publication")
    );
}

#[tokio::test]
async fn protocol_publication_previews_server_choices_and_same_protocol_edits_without_review() {
    use hiroute_domain::*;
    let root = prepare();
    let mut session = open(root.path());
    let original = plan(&mut session).await;
    let service: DecisionServiceV1 = serde_json::from_value(serde_json::json!({
        "id":"native-transition", "revision":1, "name":"Native transition fixture", "connection": {
            "kind":"system_one", "provider":"compatible", "model":"fixture", "endpoint":"https://decision.example/systemone",
            "timeout_ms":3000, "auth_header":{"name":"Authorization","value_secret_ref":"placeholder"}
        }
    })).unwrap();
    session
        .save_decision_service(
            DecisionServiceChangeV1 {
                id: service.id.clone(),
                expected_revision: 0,
                service: Some(service),
                input_slot: None,
            },
            Some(zeroize::Zeroizing::new("native-fixture".into())),
        )
        .await
        .unwrap();
    let service = session
        .decision_services()
        .await
        .unwrap()
        .services
        .remove(0);
    let mut editor = original
        .desired
        .editor(Some(original.model_alias.as_str().into()))
        .unwrap();
    editor.mode = PlanEditorMode::SmartSaving;
    editor.smart.economy = editor.candidates.clone();
    editor.smart.primary = editor.candidates.clone();
    editor.smart.classifier = ComplexityClassifierModeV1::DecisionService {
        service: Box::new(service),
    };
    editor.smart.native_protocol = Some(NativeProtocolV1::default());
    let mut input = editor_input("publish", &original, &editor, None);
    let pending = session.preview_editor_transition(&input).await.unwrap();
    assert!(
        pending
            .assessment_transition
            .as_ref()
            .unwrap()
            .require_acknowledged()
            .is_err()
    );
    assert_eq!(
        plan(&mut session).await.head,
        original.head,
        "read-only Preview cannot publish"
    );
    input.assessment_choices.insert(
        SMART_SAVING_SCOPE_ID.into(),
        AssessmentChoiceV1 {
            method: AssessmentAdoptionV1::Stock,
            source: None,
        },
    );
    let ready = session.preview_editor_transition(&input).await.unwrap();
    ready
        .assessment_transition
        .as_ref()
        .unwrap()
        .require_acknowledged()
        .unwrap();
    let confirmation = session.preview_editor(input).await.unwrap();
    session
        .finish_native_confirmation(confirmation, true)
        .await
        .unwrap();
    let adopted = plan(&mut session).await;
    assert_eq!(
        adopted.assessment_transition.as_ref().unwrap()[SMART_SAVING_SCOPE_ID].method,
        AssessmentAdoptionV1::Stock
    );
    let mut editor = adopted.desired.editor(None).unwrap();
    assert!(
        serde_json::to_value(&editor).unwrap()["smart"]
            .get("assessment_review")
            .is_none()
    );
    editor
        .smart
        .judgment
        .competence
        .instructions
        .push_str(" Review the requested deliverable.");
    let input = editor_input("publish", &adopted, &editor, None);
    assert!(
        session
            .preview_editor_transition(&input)
            .await
            .unwrap()
            .assessment_transition
            .is_none()
    );
    let confirmation = session.preview_editor(input).await.unwrap();
    session
        .finish_native_confirmation(confirmation, true)
        .await
        .unwrap();
    let edited = plan(&mut session).await;
    assert!(edited.assessment_transition.is_none());
    editor.smart.native_protocol = None;
    let mut input = editor_input("publish", &edited, &editor, None);
    assert!(
        session
            .preview_editor_transition(&input)
            .await
            .unwrap()
            .assessment_transition
            .unwrap()
            .require_acknowledged()
            .is_err()
    );
    input.assessment_choices.insert(
        SMART_SAVING_SCOPE_ID.into(),
        AssessmentChoiceV1 {
            method: AssessmentAdoptionV1::Stock,
            source: None,
        },
    );
    let confirmation = session.preview_editor(input).await.unwrap();
    session
        .finish_native_confirmation(confirmation, true)
        .await
        .unwrap();
    let restored = plan(&mut session).await;
    assert!(restored.desired.native_protocol().is_none());
    assert_eq!(
        restored.desired.assessment_scopes()[SMART_SAVING_SCOPE_ID]
            .competence
            .instructions,
        JudgmentSettingsV1::default().competence.instructions
    );
}
