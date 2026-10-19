use super::*;
use hiroute_domain::{AgentActivationModeV1, AgentActivationPreferenceV1};

fn selection(independent: bool) -> AgentSettingsSpecV2 {
    let mut value = json!({"schema_version":{"major":2,"minor":0}, "context_id":"agent-context/test",
        "model":{"intent":"configure","settings":{"mode":"claude_launcher","surfaces":["claude_cli"],"fixed_models":[],
            "preset_mappings":{"opus":{"kind":"plan","plan_id":"plan/shared"},"sonnet":{"kind":"preserve_native"},"haiku":{"kind":"preserve_native"}}}}});
    if independent {
        value["activation_preference"] = json!("managed_launch");
    }
    serde_json::from_value(value).unwrap()
}

fn conflict() -> hiroute_application_api::AgentSettingsConnectionConflictV1 {
    hiroute_application_api::AgentSettingsConnectionConflictV1 {
        connection_id: "agent-connection/agent-context/test".into(),
        restore_point_ref: Some("model-restore/original".into()),
        operation_id: Some("owned-operation".into()),
        repair: "hiroute agents restore preview --request-stdin".into(),
    }
}

#[test]
fn omitted_activation_preserves_the_released_ordinary_request_field_set() {
    let spec = selection(false);
    assert_eq!(
        spec.activation_preference,
        AgentActivationPreferenceV1::Automatic
    );
    assert!(
        serde_json::to_value(&spec)
            .unwrap()
            .get("activation_preference")
            .is_none()
    );
    let mut explicit = serde_json::to_value(&spec).unwrap();
    explicit["activation_preference"] = json!("automatic");
    let explicit: AgentSettingsSpecV2 = serde_json::from_value(explicit).unwrap();
    assert_eq!(
        serde_json::to_value(spec).unwrap(),
        serde_json::to_value(explicit).unwrap()
    );
}

#[test]
fn explicit_independent_mode_is_reported_and_unsupported_installations_are_blocked() {
    let mut current = facts();
    claude(&mut current).supports_managed_launch = false;
    let preview = preview_agent_settings(selection(true), &current).unwrap();
    assert_eq!(
        preview.activation_mode,
        Some(AgentActivationModeV1::ManagedLaunch)
    );
    assert!(
        preview
            .blockers
            .iter()
            .any(|block| block.reason == SettingsBlockReason::ManagedLaunchUnsupported)
    );
    let ordinary = preview_agent_settings(selection(false), &current).unwrap();
    assert_eq!(
        ordinary.activation_mode, None,
        "ordinary response must keep its old field set"
    );
    assert!(
        !ordinary
            .blockers
            .iter()
            .any(|block| block.reason == SettingsBlockReason::ManagedLaunchUnsupported)
    );
}

#[test]
fn activation_switch_names_the_existing_connection_and_its_restore_action() {
    let mut current = facts();
    claude(&mut current).active_connection = Some(ClaudeModelSlotFacts {
        activation_mode: AgentActivationModeV1::ManagedConfiguration,
        conflict: conflict(),
    });
    let blocked = preview_agent_settings(selection(true), &current).unwrap();
    let block = blocked
        .blockers
        .iter()
        .find(|block| block.reason == SettingsBlockReason::ClaudeConnectionInUse)
        .unwrap();
    assert_eq!(block.connection_conflict.as_ref(), Some(&conflict()));
    let same_mode = preview_agent_settings(selection(false), &current).unwrap();
    assert!(
        !same_mode
            .blockers
            .iter()
            .any(|block| block.reason == SettingsBlockReason::ClaudeConnectionInUse)
    );
}

#[test]
fn pending_model_operation_occupies_the_slot_for_both_activations() {
    let mut current = facts();
    claude(&mut current).pending_connection = Some(conflict());
    for independent in [false, true] {
        let blocked = preview_agent_settings(selection(independent), &current).unwrap();
        assert!(blocked.blockers.iter().any(|block| block.reason
            == SettingsBlockReason::ClaudeConnectionInUse
            && block.connection_conflict.as_ref() == Some(&conflict())));
    }
}
