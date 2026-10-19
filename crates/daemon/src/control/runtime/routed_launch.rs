//! Read-only default-scope and installed-publication joins. Application owns admission order.
use super::*;
use hiroute_application::control::RoutedLaunchContextFacts;
use hiroute_application_api::{
    AgentModelSettingsStateV2, AgentSettingsStatusRequestV2, RoutedLaunchAgentV1,
    RoutedLaunchRequestV1,
};
use hiroute_domain::PublicationRepositoryPort;

impl LocalControlAdapter {
    pub(super) fn routed_launch_context_facts(
        &self,
        request: &RoutedLaunchRequestV1,
    ) -> Result<RoutedLaunchContextFacts, ControlReadError> {
        use settings_facts::SettingsAgentClass;
        let class = match request.agent {
            RoutedLaunchAgentV1::Codex => SettingsAgentClass::CodexProfile,
            RoutedLaunchAgentV1::ClaudeCode => SettingsAgentClass::Claude,
        };
        let context = request
            .context
            .clone()
            .unwrap_or_else(|| self.settings_context(class));
        let mut facts = RoutedLaunchContextFacts {
            activation_mode: None,
            context: context.clone(),
            candidate_contexts: Vec::new(),
            connection_current: false,
            selection: None,
        };
        if self.settings_agent_for_context(&context) != Some(class) {
            return Ok(facts);
        }
        let (status, _, _) = self.evaluate_model_settings(&AgentSettingsStatusRequestV2 {
            schema_version: hiroute_application_api::AGENT_SETTINGS_SCHEMA_V2,
            context_id: context.clone(),
            native_target: None,
            effective: None,
            include_skills: false,
        })?;
        if status.operation_id.is_some()
            && !matches!(
                status.state,
                AgentModelSettingsStateV2::NotConfigured | AgentModelSettingsStateV2::Restored
            )
        {
            facts.candidate_contexts.push(context);
        }
        facts.connection_current = status.state == AgentModelSettingsStateV2::Configured;
        facts.activation_mode = status.activation_mode;
        facts.selection = status.current_selection;
        Ok(facts)
    }

    pub(super) fn routed_launch_installed_publication(
        &self,
    ) -> Result<Option<hiroute_domain::GatewayPublicationV1>, ControlReadError> {
        let record = self
            .stores_lock()
            .map_err(super::map_port)?
            .control()
            .active_publication(&WorkspaceId::default())
            .map_err(super::map_port)?;
        match record {
            Some(record)
                if self
                    .publication_is_installed(&record)
                    .map_err(super::map_port)? =>
            {
                record
                    .verify()
                    .map(Some)
                    .map_err(|_| ControlReadError::Corrupt)
            }
            _ => Ok(None),
        }
    }
}
