//! The management Skill reports its own file ownership, never model or Worker authority.
use super::*;
use hiroute_application::agent_connection::codex_model_restore_point_ref;
use hiroute_application_api::{
    AgentManagementSkillStatusV1, AgentModelSettingsStateV2 as State, AgentSettingsStatusRequestV2,
};
use hiroute_domain::{
    AgentFacetIntent, AgentOperationRead, AgentSettingsSpecV2, NativeAgentArtifactPort,
    OperationState,
};

impl LocalControlAdapter {
    pub(super) fn hermes_management_settings_status(
        &self,
        request: &AgentSettingsStatusRequestV2,
    ) -> Result<AgentManagementSkillStatusV1, ControlReadError> {
        let mut status = AgentManagementSkillStatusV1 {
            schema: "hiroute.agent-management-skill-status/v1".into(),
            state: State::NotConfigured,
            operation_id: None,
            operation_state: None,
            restore_point_ref: None,
        };
        let stores = self.stores_lock().map_err(super::map_port)?;
        let belongs = |operation: &dyn AgentOperationRead| -> Result<bool, ControlReadError> {
            let input = operation.agent_input();
            if input.spec.command_id != "agents.settings.apply"
                || input.spec.resource_id.as_deref() != Some(&request.context_id)
            {
                return Ok(false);
            }
            let spec: AgentSettingsSpecV2 =
                serde_json::from_value(input.spec.desired_state.clone())
                    .map_err(|_| ControlReadError::Corrupt)?;
            Ok(spec
                .management_skill
                .as_ref()
                .is_some_and(|i| !matches!(i, AgentFacetIntent::Keep)))
        };
        for pending in stores
            .control()
            .recoverable_operations()
            .map_err(super::map_port)?
        {
            if belongs(&pending)? {
                status.state = if pending.state == OperationState::NeedsAttention {
                    State::NeedsAttention
                } else {
                    State::Pending
                };
                status.operation_id = Some(pending.operation_id.to_string());
                status.operation_state = Some(pending.state.as_str().into());
                return Ok(status);
            }
        }
        let mut selected = None;
        for operation in stores
            .control()
            .succeeded_agent_operations_for_kinds(
                &WorkspaceId::default(),
                &["ApplyAgentConnectionChange", "ApplyAgentConnectionRestore"],
            )
            .map_err(super::map_port)?
        {
            if belongs(&operation)? {
                selected = Some(operation);
                break;
            }
        }
        let Some(operation) = selected else {
            return Ok(status);
        };
        let spec: AgentSettingsSpecV2 =
            serde_json::from_value(operation.plan.spec().desired_state.clone())
                .map_err(|_| ControlReadError::Corrupt)?;
        let record = stores
            .control()
            .skill_installation(
                &WorkspaceId::default(),
                &format!("skill-root/{}", request.context_id),
            )
            .map_err(super::map_port)?;
        drop(stores);
        status.operation_id = Some(operation.operation_id.to_string());
        status.operation_state = Some(operation.state.as_str().into());
        match spec.management_skill {
            Some(AgentFacetIntent::Restore { .. }) => {
                status.state = if record.is_none_or(|r| !r.contexts.contains(&request.context_id)) {
                    State::Restored
                } else {
                    State::Drift
                }
            }
            Some(AgentFacetIntent::Configure { .. }) => {
                let target = self.hermes_target_for_context(
                    &request.context_id,
                    request.native_target.as_ref(),
                )?;
                let subject = hiroute_integrations::hermes_subject(target.config_root())
                    .map_err(|_| ControlReadError::Corrupt)?;
                let file_target = hiroute_domain::AgentConnectionEffectRoleV1::RoutingSkill
                    .settings_target_for(&subject)
                    .map_err(|_| ControlReadError::Corrupt)?;
                let bytes = self
                    .artifacts
                    .read_native_target(&file_target)
                    .map_err(super::map_port)?;
                let current = record.is_some_and(|r| {
                    r.contexts.contains(&request.context_id)
                        && bytes
                            .as_ref()
                            .is_some_and(|b| CanonicalDigest::of_bytes(b) == r.content_digest)
                });
                status.state = if current {
                    State::Configured
                } else {
                    State::Drift
                };
                status.restore_point_ref =
                    Some(codex_model_restore_point_ref(&operation.operation_id));
            }
            _ => return Err(ControlReadError::Corrupt),
        }
        Ok(status)
    }
}
