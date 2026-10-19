//! Independent Claude settings live in the accepted Operation, with no native file effect.
use super::{ClaudeLaunchSnapshotIntent, codex_model_restore_point_ref};
use hiroute_domain::{
    AgentActivationPreferenceV1, AgentFacetIntent, AgentModelGrantV2, AgentOperationRead,
    AgentSettingsSpecV2, OperationId, PortError, PortErrorCode, PortResult,
};
use serde::{Deserialize, Serialize};

pub const CLAUDE_INDEPENDENT_LAUNCH_STATE_SCHEMA_V1: &str =
    "hiroute.settings-claude-launch-state/v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeIndependentLaunchStateV1 {
    pub schema: String,
    pub context_id: String,
    pub action: ClaudeIndependentLaunchActionV1,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClaudeIndependentLaunchActionV1 {
    Configure {
        previous_operation: Option<OperationId>,
        snapshot: Box<ClaudeLaunchSnapshotIntent>,
        gateway_base_url: String,
        trusted_hiroute_executable: String,
    },
    Restore {
        original_operation: OperationId,
    },
}

impl ClaudeIndependentLaunchStateV1 {
    pub fn new(context_id: String, action: ClaudeIndependentLaunchActionV1) -> PortResult<Self> {
        let state = Self {
            schema: CLAUDE_INDEPENDENT_LAUNCH_STATE_SCHEMA_V1.into(),
            context_id,
            action,
        };
        state.validate()?;
        Ok(state)
    }

    fn validate(&self) -> PortResult<()> {
        if self.schema != CLAUDE_INDEPENDENT_LAUNCH_STATE_SCHEMA_V1
            || !super::settings::identity(&self.context_id)
        {
            return Err(invalid());
        }
        match &self.action {
            ClaudeIndependentLaunchActionV1::Configure {
                snapshot,
                gateway_base_url,
                trusted_hiroute_executable,
                ..
            } => {
                snapshot.validate()?;
                if !super::claude_model_journal::local_gateway_base_url(gateway_base_url)
                    || !std::path::Path::new(trusted_hiroute_executable).is_absolute()
                    || trusted_hiroute_executable.contains(['\0', '\r', '\n'])
                {
                    return Err(invalid());
                }
            }
            ClaudeIndependentLaunchActionV1::Restore { original_operation } => {
                OperationId::parse(original_operation.as_str().to_owned())
                    .map_err(|_| invalid())?;
            }
        }
        Ok(())
    }
}

/// Consume only the original accepted state. Absence is the released ordinary mode;
/// malformed or detached metadata is corruption, never an invitation to rebuild from disk.
pub fn claude_independent_launch_for_operation(
    operation: &(impl AgentOperationRead + ?Sized),
) -> PortResult<Option<ClaudeIndependentLaunchStateV1>> {
    let input = operation.agent_input();
    let value = &input.control["payload"]["state"]["mutations"]["claude_launch"];
    if value.is_null() {
        if input.spec.desired_state["activation_preference"] == "managed_launch" {
            return Err(invalid());
        }
        return Ok(None);
    }
    let state: ClaudeIndependentLaunchStateV1 =
        serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    state.validate()?;
    let spec: AgentSettingsSpecV2 =
        serde_json::from_value(input.spec.desired_state.clone()).map_err(|_| invalid())?;
    if input.spec.command_id != "agents.settings.apply"
        || spec.context_id != state.context_id
        || spec.activation_preference != AgentActivationPreferenceV1::ManagedLaunch
        || input.control["payload"]["state"]["accept_digest"]
            != serde_json::to_value(operation.accepted_digest()).map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    match (&spec.model, &state.action) {
        (
            AgentFacetIntent::Configure { settings },
            ClaudeIndependentLaunchActionV1::Configure {
                snapshot,
                previous_operation,
                ..
            },
        ) => {
            if previous_operation.as_ref() == Some(operation.operation_id()) {
                return Err(invalid());
            }
            let grant: AgentModelGrantV2 =
                serde_json::from_value(input.control["payload"]["state"]["model_grant"].clone())
                    .map_err(|_| invalid())?;
            let [mutation] = input.grants else {
                return Err(invalid());
            };
            let scope = mutation.desired_scope().ok_or_else(invalid)?;
            if scope.model_grant() != &grant
                || scope.connection_id() != format!("agent-connection/{}", state.context_id)
                || grant
                    .claude_preset_values(settings, &snapshot.native_presets)
                    .map_err(|_| invalid())?
                    != snapshot.presets
            {
                return Err(invalid());
            }
        }
        (
            AgentFacetIntent::Restore { restore_point_ref },
            ClaudeIndependentLaunchActionV1::Restore { original_operation },
        ) if original_operation != operation.operation_id()
            && *restore_point_ref == codex_model_restore_point_ref(original_operation) => {}
        _ => return Err(invalid()),
    }
    Ok(Some(state))
}

fn invalid() -> PortError {
    PortError::new(PortErrorCode::InvalidData, "claude.independent.launch")
}
