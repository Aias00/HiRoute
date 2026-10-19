//! Read-only launch composition. Settings status remains the authority for native files,
//! grant generation and installed publication; no configuration is repaired here.
use super::*;
use hiroute_application::control::{AgentConnectionControlPort, CodexLaunchDescriptorRead};
use hiroute_application_api::{
    AgentLaunchCheckV1, AgentLaunchDescriptorRequestV1, AgentModelSettingsStateV2,
    AgentSettingsStatusRequestV2, RoutedLaunchAgentV1, RoutedLaunchDescriptorV1,
    RoutedLaunchReadV1, RoutedLaunchRequestV1,
};

impl LocalControlAdapter {
    pub(super) fn routed_launch_read(
        &self,
        request: &RoutedLaunchRequestV1,
    ) -> Result<RoutedLaunchReadV1, ControlReadError> {
        use settings_facts::SettingsAgentClass;
        let class = match request.agent {
            RoutedLaunchAgentV1::Codex => SettingsAgentClass::CodexProfile,
            RoutedLaunchAgentV1::ClaudeCode => SettingsAgentClass::Claude,
        };
        let context = request
            .context
            .clone()
            .unwrap_or_else(|| self.settings_context(class));
        let mut read = RoutedLaunchReadV1 {
            check: AgentLaunchCheckV1::new(request.agent, Some(context.clone())),
            descriptor: None,
        };
        let repair = "hiroute agents connect preview --help";
        if self.settings_agent_for_context(&context) != Some(class) {
            read.check
                .record("connection", false, "connection_missing", repair);
            return Ok(read);
        }
        let status = self.model_settings_status(&AgentSettingsStatusRequestV2 {
            schema_version: hiroute_application_api::AGENT_SETTINGS_SCHEMA_V2,
            context_id: context.clone(),
            native_target: None,
            effective: None,
            include_skills: false,
        })?;
        if status.state != AgentModelSettingsStateV2::Configured {
            read.check
                .record("connection", false, "connection_not_current", repair);
            return Ok(read);
        }
        read.check.candidate_contexts.push(context.clone());
        let descriptor_request = AgentLaunchDescriptorRequestV1 {
            connection_id: format!("agent-connection/{context}"),
        };
        let descriptor = match request.agent {
            RoutedLaunchAgentV1::Codex => {
                match self.codex_launch_descriptor_read(&descriptor_request) {
                    Ok(CodexLaunchDescriptorRead::Ready(descriptor)) => {
                        RoutedLaunchDescriptorV1::Codex(descriptor)
                    }
                    Ok(CodexLaunchDescriptorRead::SnapshotMissing) => {
                        read.check
                            .record("connection", false, "launch_snapshot_missing", repair);
                        return Ok(read);
                    }
                    Err(ControlReadError::SnapshotChanged) => {
                        read.check
                            .record("connection", false, "installation_changed", repair);
                        return Ok(read);
                    }
                    Err(error) => return Err(error),
                }
            }
            RoutedLaunchAgentV1::ClaudeCode => {
                // The existing managed path rechecks native auth precedence, the private
                // overlay and helper. Activation-choice admission is owned by settings.
                match self.managed_launch_descriptor(&descriptor_request) {
                    Ok(descriptor) => RoutedLaunchDescriptorV1::ClaudeCode(Box::new(descriptor)),
                    Err(ControlReadError::Denied | ControlReadError::SnapshotChanged) => {
                        read.check
                            .record("connection", false, "connection_not_current", repair);
                        return Ok(read);
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        read.check.record("connection", true, "", "");
        let selection = status.current_selection.ok_or(ControlReadError::Corrupt)?;
        let publication = self
            .stores_lock()
            .map_err(super::map_port)?
            .control()
            .active_publication(&WorkspaceId::default())
            .map_err(super::map_port)?;
        let Some(publication) = publication else {
            read.check
                .record("plans", false, "plan_unpublished", "hiroute routing list");
            return Ok(read);
        };
        let digest = match &descriptor {
            RoutedLaunchDescriptorV1::Codex(d) => &d.publication_digest,
            RoutedLaunchDescriptorV1::ClaudeCode(d) => &d.publication_digest,
        };
        if &publication.digest != digest {
            read.check.record(
                "plans",
                false,
                "publication_changed",
                "hiroute routing list",
            );
            return Ok(read);
        }
        let published = publication
            .verify()
            .map_err(|_| ControlReadError::Corrupt)?
            .published_agent_plans()
            .map_err(|_| ControlReadError::Corrupt)?;
        if !read.check.record(
            "plans",
            selection.allowed_plan_ids().iter().all(|plan| {
                published
                    .iter()
                    .any(|p| &p.agent_plan_id == plan && p.active)
            }),
            "plan_unpublished",
            "hiroute routing list",
        ) {
            return Ok(read);
        }
        if request.agent == RoutedLaunchAgentV1::ClaudeCode {
            read.check.record("managed_launch", true, "", "");
        }
        read.descriptor = Some(descriptor);
        Ok(read)
    }
}
