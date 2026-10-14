//! Selected-root discovery. Legacy no-target scans never gain an unsolicited Hermes row.
use super::*;
use hiroute_domain::{AgentCapability, CanonicalDigest, CapabilityReason, CapabilityState};
use std::collections::BTreeMap;

pub fn hermes_settings_installation(
    target: &hiroute_domain::AgentNativeTargetV1,
) -> Result<hiroute_domain::SupportedAgentInstallationV1, HermesNativeError> {
    hermes_installation(target, true)
}

pub fn hermes_management_installation(
    target: &hiroute_domain::AgentNativeTargetV1,
) -> Result<hiroute_domain::SupportedAgentInstallationV1, HermesNativeError> {
    hermes_installation(target, false)
}

fn hermes_installation(
    target: &hiroute_domain::AgentNativeTargetV1,
    model: bool,
) -> Result<hiroute_domain::SupportedAgentInstallationV1, HermesNativeError> {
    let target = canonical_hermes_target(target)?;
    let root = target.config_root();
    let config = if model {
        Some(hermes_configuration_digest(root)?)
    } else {
        None
    };
    let support = hermes_support_facts();
    let mut profile = hermes_profile_v1();
    profile.profile_id = hermes_subject(root)?.profile_id().to_owned();
    let digest = CanonicalDigest::of(&(
        "hermes-profile-observation/v1",
        root,
        config,
        &support.dependency_digest,
    ))
    .map_err(|_| hermes_yaml::hermes_error("profile observation"))?;
    let mut installation = hiroute_domain::SupportedAgentInstallationV1 {
        agent_id: "agent_hermes_default".into(),
        version: support.version,
        profile,
        effective_config: BTreeMap::new(),
        observation_digest: digest,
        capability_evidence: Vec::new(),
    };
    observed_capabilities::attach_target_file_capabilities(
        &mut installation,
        &std::path::Path::new(root).join(if model {
            "config.yaml"
        } else {
            "skills/hiroute-management/SKILL.md"
        }),
    );
    for proof in &mut installation.capability_evidence {
        let available = match proof.capability {
            AgentCapability::IngressAuthentication => Some(support.middleware),
            AgentCapability::SkillLoading => Some(support.skills),
            _ => None,
        };
        if let Some(available) = available {
            proof.state = if available {
                CapabilityState::Proven
            } else {
                CapabilityState::Unavailable
            };
            proof.reason = (!available).then_some(CapabilityReason::UnsupportedNativeFeature);
            proof.adapter_contract = "hiroute.hermes-native/v1".into();
        }
    }
    Ok(installation)
}
