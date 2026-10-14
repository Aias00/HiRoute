//! Pi's effective user targets and bounded settings facts. No native process or helper is run.
use super::*;
use hiroute_domain::{AdditionalAgentModelV1, AgentCapability, CapabilityState};
use serde::Serialize;

fn pi_executable_candidate(path: &Path) -> Option<PathBuf> {
    let executable = super::super::executable::resolve(path).ok().flatten()?;
    #[cfg(windows)]
    {
        windows_pi_entry(&executable).ok()
    }
    #[cfg(not(windows))]
    {
        Some(executable)
    }
}

/// Borrowed package files retain their existing owner and ACL. Native checks
/// reject reparse aliases; file leases deny data writes/deletion and directory
/// leases deny namespace replacement only while this locator observes them.
#[cfg(windows)]
fn windows_pi_file(
    path: &Path,
    ancestors: &mut Vec<std::fs::File>,
) -> std::io::Result<std::fs::File> {
    use hiroute_platform::windows::security;
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x02000000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x00200000;
    // npm accepts a harmless leading './' in bin declarations. Remove only
    // that lexical component, before any native read; never follow an alias
    // by canonicalizing or normalize a parent-directory escape.
    let mut lexical = PathBuf::new();
    for component in path.components() {
        use std::path::{Component, Prefix};
        match component {
            Component::Prefix(prefix)
                if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) =>
            {
                lexical.push(component.as_os_str());
            }
            Component::RootDir | Component::Normal(_) => lexical.push(component.as_os_str()),
            Component::CurDir => {}
            _ => return Err(std::io::ErrorKind::InvalidInput.into()),
        }
    }
    let path = lexical.as_path();
    for directory in path
        .ancestors()
        .skip(1)
        .filter(|path| path.file_name().is_some())
    {
        let original = security::open_identity(directory, true)?;
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(directory)?;
        if security::file_identity(&original)? != security::file_identity(&held)? {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        ancestors.push(held);
    }
    let original = security::open_identity(path, false)?;
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if security::file_identity(&original)? != security::file_identity(&held)? {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    Ok(held)
}

/// A generated npm shim is a locator, never a process or package identity.
/// The shared npm producer inspects each original spelling before reading or
/// canonicalizing it. This verifies this observation, not a future launch.
#[cfg(windows)]
fn windows_pi_entry(selected: &Path) -> std::io::Result<PathBuf> {
    use hiroute_platform::windows::security;
    use std::io::Read;
    let mut ancestors = Vec::new();
    let (normalized, entry, mut manifests) = if selected
        .extension()
        .is_some_and(|extension| extension.as_encoded_bytes().eq_ignore_ascii_case(b"cmd"))
    {
        if !selected
            .file_stem()
            .is_some_and(|stem| stem.as_encoded_bytes().eq_ignore_ascii_case(b"pi"))
        {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let mut inspected = Vec::new();
        let normalized =
            super::super::windows_runtime::windows_npm_entry_with_inspection(selected, |path| {
                inspected.push(windows_pi_file(path, &mut ancestors)?);
                Ok(())
            })?;
        // The existing shared callback contract supplies manifest, then entry.
        if inspected.len() != 2 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        let entry = inspected.pop().expect("two inspected native files");
        (normalized, entry, inspected)
    } else {
        let entry = windows_pi_file(selected, &mut ancestors)?;
        (selected.to_owned(), entry, Vec::new())
    };
    let executable = super::super::managed_launch::windows_executable_path(&normalized)?;
    let current = security::open_identity(&executable, false)?;
    if security::file_identity(&entry)? != security::file_identity(&current)? {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    // The sole Pi producer stops at the nearest matching declaration. Retain
    // only paths it actually reads; also inspect the raw bin spelling before
    // canonicalization could hide a junction unrelated to the selected path.
    let identity =
        super::super::pi_runtime::pi_cli_installation_with_inspection(&executable, |path| {
            manifests.push(windows_pi_file(path, &mut ancestors)?);
            Ok(())
        })
        .map_err(|_| std::io::ErrorKind::InvalidData)?;
    let manifest = security::open_identity(&identity.package_root.join("package.json"), false)?;
    let expected = security::file_identity(&manifest)?;
    let mut matching = None;
    for (index, held) in manifests.iter().enumerate() {
        if security::file_identity(held)? == expected {
            matching = Some(index);
            break;
        }
    }
    let held = &mut manifests[matching.ok_or(std::io::ErrorKind::InvalidData)?];
    let mut bytes = Vec::new();
    held.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 || identity.manifest_digest != CanonicalDigest::of_bytes(&bytes) {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    Ok(executable)
}

fn settings_value(bytes: &[u8]) -> Result<serde_json::Value, AgentFilesystemScanError> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let value = if bytes.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(bytes).map_err(|_| AgentFilesystemScanError::InvalidConfig)?
    };
    if !value.is_object() {
        return Err(AgentFilesystemScanError::InvalidConfig);
    }
    Ok(value)
}

#[derive(Serialize)]
pub struct PiDefaultModel {
    pub content_digest: CanonicalDigest,
    provider: Option<String>,
    model: Option<String>,
}
impl PiDefaultModel {
    pub fn removes_default(&self, provider: &str, models: &[AdditionalAgentModelV1]) -> bool {
        self.provider.as_deref().is_some_and(|selected_provider| {
            let owned_namespace = selected_provider == provider
                || selected_provider
                    .strip_prefix(&format!("{provider}-"))
                    .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()));
            owned_namespace
                && self.model.as_ref().is_none_or(|selected| {
                    !models.iter().any(|m| {
                        &m.alias == selected
                            && hiroute_domain::additional_model_provider_for(provider, models, m)
                                == selected_provider
                    })
                })
        })
    }
}

impl FilesystemAgentScannerV1 {
    pub fn pi_user_models_target(&self) -> PathBuf {
        self.layout.pi_config_root.join("models.json")
    }
    pub fn pi_user_skill_target(&self) -> PathBuf {
        self.layout
            .pi_config_root
            .join("skills/hiroute-collaboration/SKILL.md")
    }
    pub fn pi_default_model(&self) -> Result<PiDefaultModel, AgentFilesystemScanError> {
        let observed = super::super::filesystem_config::read_validated_config_bytes(
            &self.layout.pi_config_root.join("settings.json"),
        )?;
        let bytes = observed
            .as_ref()
            .map_or(&[][..], |(bytes, _)| bytes.as_slice());
        let value = settings_value(bytes)?;
        let field = |key: &str| -> Result<Option<String>, AgentFilesystemScanError> {
            value
                .get(key)
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or(AgentFilesystemScanError::InvalidConfig)
                })
                .transpose()
        };
        Ok(PiDefaultModel {
            content_digest: CanonicalDigest::of_bytes(bytes),
            provider: field("defaultProvider")?,
            model: field("defaultModel")?,
        })
    }
    pub fn pi_executable_target(&self) -> Option<PathBuf> {
        let selected = &self.layout.pi_executable;
        if selected.is_absolute() || selected.components().count() > 1 {
            return pi_executable_candidate(selected);
        }
        let mut paths = std::env::var_os("PATH")
            .map(|v| {
                std::env::split_paths(&v)
                    .filter(|p| p.is_absolute())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        paths.extend([PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin")]);
        #[cfg(target_os = "macos")]
        paths.push(PathBuf::from("/opt/homebrew/bin"));
        paths
            .into_iter()
            .take(128)
            .find_map(|p| pi_executable_candidate(&p.join(selected)))
    }
    pub fn pi_settings_discovery(&self) -> FilesystemAgentDiscoveryV1 {
        let Some(executable) = self.pi_executable_target() else {
            return probe_report(
                "agent_pi_default",
                AgentKindV1::Pi,
                ExecutableProbe::NotFound,
            );
        };
        let installation = super::super::pi_cli_installation(&executable);
        if installation.is_err() || !self.layout.pi_config_root.is_absolute() {
            return report_only_agent(
                "agent_pi_default",
                AgentKindV1::Pi,
                "not-probed".into(),
                AgentReportOnlyReasonV1::ConfigUnavailable,
            );
        }
        let identity = installation.expect("checked CLI identity");
        let mut outcome = resolve_agent_observation(base_observation(
            "agent_pi_default",
            AgentKindV1::Pi,
            identity.version,
        ));
        let resource_settings = super::super::filesystem_config::read_validated_config_bytes(
            &self.layout.pi_config_root.join("settings.json"),
        )
        .and_then(|observed| {
            let bytes = observed.map(|(bytes, _)| bytes).unwrap_or_default();
            let settings = settings_value(&bytes)?;
            let rules = settings
                .get("skills")
                .map(|v| v.as_array().ok_or(AgentFilesystemScanError::InvalidConfig))
                .transpose()?;
            let excluded = rules.is_some_and(|rules| {
                rules.iter().any(|rule| {
                    let Some(rule) = rule.as_str() else {
                        return true;
                    };
                    let negative = rule.starts_with('!') || rule.starts_with('-');
                    // Exact exclusions of unrelated Skills remain usable. Complex exclusion
                    // patterns cannot be statically proven here and fail closed at Save.
                    let pattern = &rule[usize::from(negative)..];
                    negative
                        && (!pattern
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-/ ".contains(&b))
                            || pattern.contains("hiroute-collaboration")
                            || pattern.ends_with("SKILL.md")
                            || pattern.trim_end_matches('/').ends_with("skills"))
                })
            });
            Ok((CanonicalDigest::of_bytes(&bytes), !excluded))
        });
        if let AgentDiscoveryOutcomeV1::Supported { installation } = &mut outcome {
            super::super::observed_capabilities::attach_file_capabilities(
                installation,
                &executable,
                &self.pi_user_skill_target(),
            );
            let resources_allowed = resource_settings
                .as_ref()
                .is_ok_and(|(_, allowed)| *allowed);
            if let Ok((settings_digest, _)) = &resource_settings {
                installation.observation_digest =
                    CanonicalDigest::of(&(&installation.observation_digest, settings_digest))
                        .expect("serializable native observation");
                for proof in &mut installation.capability_evidence {
                    proof.dependency_digest = installation.observation_digest.clone();
                }
            }
            // Only a successful local resource/tool interface check authorizes Skill loading.
            // Model configuration does not depend on the Worker or history SDK contracts.
            let resource_contract = self.pi_collaboration_cli.lock().ok().is_some_and(|v| {
                v.as_ref()
                    .is_some_and(|(_, digest)| *digest == identity.manifest_digest)
            });
            for proof in &mut installation.capability_evidence {
                if proof.capability == AgentCapability::SkillLoading
                    && resources_allowed
                    && resource_contract
                {
                    proof.state = CapabilityState::Proven;
                    proof.reason = None;
                    proof.adapter_contract = "hiroute.pi-native-resources/v1".into();
                }
                if proof.capability == AgentCapability::TrustedCliExecution
                    && self.pi_collaboration_cli.lock().ok().is_some_and(|v| {
                        v.as_ref().is_some_and(|(cli, digest)| {
                            *digest == identity.manifest_digest
                                && super::super::executable::resolve(cli)
                                    .ok()
                                    .flatten()
                                    .as_ref()
                                    == Some(cli)
                        })
                    })
                {
                    proof.state = CapabilityState::Proven;
                    proof.reason = None;
                    proof.adapter_contract = "hiroute.pi-local-cli/v1".into();
                }
            }
        }
        FilesystemAgentDiscoveryV1 {
            outcome,
            configuration_issue: None,
            claude_configuration: None,
            discovered_credential: None,
            permission_hardening: None,
            managed_launch: None,
        }
    }
    #[cfg(any(unix, windows))]
    pub fn check_pi_collaboration(&self, cli: &Path) -> Result<(), AgentFilesystemScanError> {
        *self
            .pi_collaboration_cli
            .lock()
            .map_err(|_| AgentFilesystemScanError::SourceUnavailable)? = None;
        let executable = self
            .pi_executable_target()
            .ok_or(AgentFilesystemScanError::SourceUnavailable)?;
        let identity = super::super::pi_cli_installation(&executable)?;
        self.check_pi_capability(&executable, super::super::PiSdkCapability::Collaboration)?;
        if super::super::pi_cli_installation(&executable)?.manifest_digest
            != identity.manifest_digest
        {
            return Err(AgentFilesystemScanError::SourceChanged);
        }
        let cli = super::super::executable::resolve(cli)
            .map_err(|_| AgentFilesystemScanError::SourceUnavailable)?
            .ok_or(AgentFilesystemScanError::SourceUnavailable)?;
        *self
            .pi_collaboration_cli
            .lock()
            .map_err(|_| AgentFilesystemScanError::SourceUnavailable)? =
            Some((cli, identity.manifest_digest));
        Ok(())
    }

    #[cfg(any(unix, windows))]
    pub fn check_pi_model_configuration(&self) -> Result<(), AgentFilesystemScanError> {
        let executable = self
            .pi_executable_target()
            .ok_or(AgentFilesystemScanError::SourceUnavailable)?;
        self.check_pi_capability(&executable, super::super::PiSdkCapability::Models)
    }

    #[cfg(any(unix, windows))]
    fn check_pi_capability(
        &self,
        executable: &Path,
        capability: super::super::PiSdkCapability,
    ) -> Result<(), AgentFilesystemScanError> {
        let node = super::super::executable::resolve(Path::new("node"))
            .map_err(|_| AgentFilesystemScanError::SourceUnavailable)?
            .ok_or(AgentFilesystemScanError::SourceUnavailable)?;
        super::super::check_pi_sdk_capability(executable, &node, capability)
            .map_err(|_| AgentFilesystemScanError::SourceUnavailable)
    }
}

#[cfg(all(test, windows))]
#[path = "pi_native/windows_tests.rs"]
mod windows_tests;
