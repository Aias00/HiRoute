//! Standalone profile launch reads the authenticated succeeded Operation. A retained
//! configuration without launch facts needs an explicitly confirmed save, never backfill.
use super::*;
use hiroute_application::agent_connection::{
    CodexModelFileAction, decode_settings_codex_model_file,
};
use hiroute_application::control::CodexLaunchDescriptorRead;
use hiroute_application_api::{AgentLaunchDescriptorRequestV1, ManagedCodexLaunchDescriptorV1};

impl LocalControlAdapter {
    pub(super) fn codex_launch_descriptor_read(
        &self,
        request: &AgentLaunchDescriptorRequestV1,
    ) -> Result<CodexLaunchDescriptorRead, ControlReadError> {
        let context = request
            .connection_id
            .strip_prefix("agent-connection/")
            .ok_or(ControlReadError::NotFound)?;
        if self.settings_agent_for_context(context)
            != Some(settings_facts::SettingsAgentClass::CodexProfile)
        {
            return Err(ControlReadError::NotFound);
        }
        let join = self
            .configured_model_settings_join(context)?
            .ok_or(ControlReadError::Denied)?;
        let payload = decode_settings_codex_model_file(&join.intent).map_err(super::map_port)?;
        if payload.context_id != context {
            return Err(ControlReadError::Corrupt);
        }
        let snapshot = match payload.change {
            CodexModelFileAction::Configure {
                launch_snapshot: Some(snapshot),
                ..
            } => snapshot,
            CodexModelFileAction::Configure {
                launch_snapshot: None,
                ..
            } => {
                return Ok(CodexLaunchDescriptorRead::SnapshotMissing);
            }
            _ => return Err(ControlReadError::Corrupt),
        };
        if !self
            .scanner
            .codex_launch_snapshot_is_current(&snapshot)
            .map_err(|_| ControlReadError::SnapshotChanged)?
        {
            return Err(ControlReadError::SnapshotChanged);
        }
        ManagedCodexLaunchDescriptorV1::trusted(
            request.connection_id.clone(),
            snapshot,
            join.grant.generation(),
            join.publication_digest,
        )
        .map(|descriptor| CodexLaunchDescriptorRead::Ready(Box::new(descriptor)))
        .map_err(|_| ControlReadError::Corrupt)
    }
}
