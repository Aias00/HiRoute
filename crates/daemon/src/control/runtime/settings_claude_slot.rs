//! Existing grant/Operation ownership supplies one Claude configuration slot, across activations.
use super::*;
use hiroute_application::agent_connection::{ClaudeModelSlotFacts, codex_model_restore_point_ref};
use hiroute_application_api::AgentSettingsConnectionConflictV1;
use hiroute_domain::{
    AgentActivationModeV1, AgentActivationPreferenceV1, AgentFacetIntent, AgentSettingsSpecV2,
    ControlRepositoryPort, SecretStorePort,
};

impl LocalControlAdapter {
    pub(super) fn claude_model_slot_facts(
        &self,
        context: &str,
    ) -> Result<
        (
            Option<ClaudeModelSlotFacts>,
            Option<AgentSettingsConnectionConflictV1>,
        ),
        ControlReadError,
    > {
        let stores = self.stores_lock().map_err(super::map_port)?;
        let connection_id = format!("agent-connection/{context}");
        let legacy_id = "agent-connection/agent_claude_default/claude-messages-v1";
        let pending = stores
            .control()
            .recoverable_operations()
            .map_err(super::map_port)?
            .into_iter()
            .find(|operation| {
                let spec = operation.plan.spec();
                if spec.command_id == "agents.settings.apply"
                    && spec.resource_id.as_deref() == Some(context)
                {
                    // Decode below when this is the matching model operation, rather than
                    // treating malformed retained input as an empty/available slot.
                    spec.desired_state["model"]["intent"] != "keep"
                        && !spec.desired_state["model"].is_null()
                } else {
                    (spec.command_id == "agents.connect.apply"
                        && spec.desired_state["agent_id"] == "agent_claude_default")
                        || (spec.command_id == "agents.restore.apply"
                            && spec.resource_id.as_deref() == Some(legacy_id))
                }
            });
        let pending = pending.map(|operation| AgentSettingsConnectionConflictV1 {
            connection_id: connection_id.clone(),
            restore_point_ref: None,
            operation_id: Some(operation.operation_id.to_string()),
            repair: format!("hiroute operations get {}", operation.operation_id),
        });
        let current = stores
            .secrets()
            .inspect_agent_access_grant(WorkspaceId::DEFAULT, &connection_id)
            .map_err(super::map_port)?;
        let mut active = None;
        if let Some(current) = current {
            for operation in stores
                .control()
                .succeeded_agent_operations_for_kind(
                    &WorkspaceId::default(),
                    "ApplyAgentConnectionChange",
                )
                .map_err(super::map_port)?
            {
                let spec = operation.plan.spec();
                if spec.command_id != "agents.settings.apply"
                    || spec.resource_id.as_deref() != Some(context)
                {
                    continue;
                }
                let spec: AgentSettingsSpecV2 = serde_json::from_value(spec.desired_state.clone())
                    .map_err(|_| ControlReadError::Corrupt)?;
                if !matches!(spec.model, AgentFacetIntent::Configure { .. }) {
                    continue;
                }
                let [mutation] = operation.plan.agent_access_grants() else {
                    return Err(ControlReadError::Corrupt);
                };
                let effect = operation
                    .step(hiroute_domain::OperationStepKind::ApplySecrets)
                    .effects
                    .iter()
                    .find(|effect| hiroute_domain::is_agent_access_grant_effect(effect))
                    .ok_or(ControlReadError::Corrupt)?;
                let reference =
                    hiroute_domain::AgentAccessGrantRefV1::from_ensure_effect(effect, mutation)
                        .map_err(|_| ControlReadError::Corrupt)?;
                if reference != current {
                    continue;
                }
                active = Some(ClaudeModelSlotFacts {
                    activation_mode: if spec.activation_preference
                        == AgentActivationPreferenceV1::ManagedLaunch
                    {
                        AgentActivationModeV1::ManagedLaunch
                    } else {
                        AgentActivationModeV1::ManagedConfiguration
                    },
                    conflict: AgentSettingsConnectionConflictV1 {
                        connection_id: connection_id.clone(),
                        restore_point_ref: Some(codex_model_restore_point_ref(
                            &operation.operation_id,
                        )),
                        operation_id: Some(operation.operation_id.to_string()),
                        repair: "hiroute agents restore preview --request-stdin".into(),
                    },
                });
                break;
            }
            if active.is_none() {
                return Err(ControlReadError::Corrupt);
            }
        }
        // Released pre-settings connections use the earlier stable connection identity.
        // It shares this daemon's default Claude configuration scope and must be restored first.
        let legacy = stores
            .secrets()
            .inspect_agent_access_grant(WorkspaceId::DEFAULT, legacy_id)
            .map_err(super::map_port)?;
        drop(stores);
        if legacy.is_some() {
            if active.is_some() {
                return Err(ControlReadError::Corrupt);
            }
            let (connection, operation) = self.latest_connection_operation(legacy_id)?;
            active = Some(ClaudeModelSlotFacts {
                activation_mode: connection.connection.activation_mode,
                conflict: AgentSettingsConnectionConflictV1 {
                    connection_id: legacy_id.into(),
                    restore_point_ref: None,
                    operation_id: Some(operation.operation_id.to_string()),
                    repair: "hiroute agents restore preview --request-stdin".into(),
                },
            });
        }
        Ok((active, pending))
    }
}
