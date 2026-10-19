//! Launch-time configuration facts exclude user settings and stored credentials entirely.
use super::*;

pub(super) fn sources(layout: &AgentFilesystemLayoutV1, include_user: bool) -> Vec<ClaudeSource> {
    let mut sources = vec![ClaudeSource::Process];
    if let Some(path) = &layout.claude_launch_settings {
        sources.push(ClaudeSource::File(ConfigLayerV1::Launch, path.clone()));
    }
    sources.extend(
        layout
            .claude_project_settings
            .iter()
            .filter(|path| !same_claude_settings_file(path, &layout.claude_user_settings))
            .cloned()
            .map(|path| ClaudeSource::File(ConfigLayerV1::Project, path)),
    );
    if include_user {
        sources.push(ClaudeSource::File(
            ConfigLayerV1::User,
            layout.claude_user_settings.clone(),
        ));
    }
    sources.extend(
        layout
            .claude_managed_settings
            .iter()
            .cloned()
            .map(|path| ClaudeSource::File(ConfigLayerV1::Managed, path)),
    );
    sources
}

impl AgentFilesystemLayoutV1 {
    /// Capture only the caller's project/process/policy layers. The frozen launch does not
    /// need the native user's settings, OAuth file, API token or credential helper.
    pub fn claude_independent_launch_preflight(
        &self,
        executable: String,
        oauth_override_present: bool,
    ) -> Result<ManagedClaudeLaunchPreflightV1, AgentFilesystemScanError> {
        let observations = sources(self, false)
            .iter()
            .map(|source| {
                source.read(
                    &self.process_environment,
                    &self.process_environment_presence,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut preflight = ManagedClaudeLaunchPreflightV1::from_independent_observations(
            executable,
            &hiroute_domain::ManagedLaunchProfileV1::claude_code(),
            &observations,
        );
        // This native OAuth override is outside the retained environment-removal contract.
        // Retain and expose only presence, never the credential value.
        if oauth_override_present {
            preflight.conflicts.push(
                super::super::managed_launch::AgentAuthPrecedenceConflictV1 {
                    code: super::super::managed_launch::AGENT_AUTH_PRECEDENCE_CONFLICT_V1.into(),
                    layer: ConfigLayerV1::Process,
                    source_ref: "process/CLAUDE_CODE_OAUTH_TOKEN".into(),
                    fields: BTreeSet::from([
                        super::super::managed_launch::AgentAuthRoutingFieldV1::ClaudeOAuthToken,
                    ]),
                },
            );
        }
        Ok(preflight)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn layout(root: &Path) -> AgentFilesystemLayoutV1 {
        let home = root.join("user");
        let project = root.join("project");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        let mut layout = AgentFilesystemLayoutV1::from_process(&home, &project);
        layout.claude_user_settings = home.join(".claude/settings.json");
        layout.claude_managed_settings.clear();
        layout.process_environment.clear();
        layout.process_environment_presence.clear();
        layout
    }

    #[test]
    fn independent_preflight_never_opens_user_settings_or_stored_credentials() {
        let root = tempfile::tempdir().unwrap();
        let layout = layout(root.path());
        // Invalid user settings would make a full discovery fail. Launch only needs the
        // frozen state and the caller's higher-priority routing/auth sources.
        std::fs::write(&layout.claude_user_settings, b"not a JSON document").unwrap();
        let credentials = layout
            .claude_user_settings
            .with_file_name(".credentials.json");
        let bytes = b"private-native-credential-fixture";
        std::fs::write(&credentials, bytes).unwrap();
        let preflight = layout
            .claude_independent_launch_preflight("/fixture/claude".into(), false)
            .unwrap();
        assert!(preflight.is_launchable());
        assert_eq!(
            std::fs::read(&layout.claude_user_settings).unwrap(),
            b"not a JSON document"
        );
        assert_eq!(std::fs::read(credentials).unwrap(), bytes);
    }

    #[test]
    fn project_auth_is_blocked_without_reporting_values_and_without_changing_files() {
        let root = tempfile::tempdir().unwrap();
        let layout = layout(root.path());
        let project = &layout.claude_project_settings[0];
        let bytes = br#"{"env":{"ANTHROPIC_BASE_URL":"https://example.test/native","ANTHROPIC_AUTH_TOKEN":"opaque-project-secret"}}"#;
        std::fs::write(project, bytes).unwrap();
        std::fs::set_permissions(project, std::fs::Permissions::from_mode(0o600)).unwrap();
        let preflight = layout
            .claude_independent_launch_preflight("/fixture/claude".into(), false)
            .unwrap();
        assert!(!preflight.is_launchable());
        assert_eq!(preflight.conflicts[0].layer, ConfigLayerV1::Project);
        assert!(
            !serde_json::to_string(&preflight)
                .unwrap()
                .contains("opaque-project-secret")
        );
        assert_eq!(std::fs::read(project).unwrap(), bytes);
    }

    #[test]
    fn unsanitized_oauth_override_is_a_presence_only_process_blocker() {
        let root = tempfile::tempdir().unwrap();
        let layout = layout(root.path());
        let preflight = layout
            .claude_independent_launch_preflight("/fixture/claude".into(), true)
            .unwrap();
        assert!(!preflight.is_launchable());
        assert_eq!(
            preflight.conflicts[0].source_ref,
            "process/CLAUDE_CODE_OAUTH_TOKEN"
        );
        assert_eq!(preflight.conflicts[0].layer, ConfigLayerV1::Process);
    }
}
