//! Caller-default paths are discovery hints; the shared backend establishes actual context.
use super::AgentEntry;
use hiroute_domain::{AgentModelSurfaceV2, AgentNativeTargetV1};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(super) enum ProfileDiscovery {
    Missing,
    Found(AgentNativeTargetV1),
    Unavailable {
        target: Option<AgentNativeTargetV1>,
        reason: &'static str,
    },
}

pub(super) fn default_hermes_profile() -> ProfileDiscovery {
    hermes_target_in(
        std::env::var_os("HERMES_HOME").as_deref(),
        std::env::var_os("HOME").as_deref().map(Path::new),
    )
}

fn hermes_target_in(selected: Option<&OsStr>, home: Option<&Path>) -> ProfileDiscovery {
    let selected = selected.filter(|root| !root.is_empty());
    let root = match selected {
        Some(root) => PathBuf::from(root),
        None => match home {
            Some(home) => home.join(".hermes"),
            None => return ProfileDiscovery::Missing,
        },
    };
    let invalid = || ProfileDiscovery::Unavailable {
        target: None,
        reason: "AGENT_PROFILE_PATH_INVALID",
    };
    let Some(config_root) = root.to_str() else {
        return invalid();
    };
    let target = AgentNativeTargetV1::Hermes {
        config_root: config_root.to_owned(),
    };
    if target.validate().is_err() {
        return invalid();
    }
    let unavailable = |reason| ProfileDiscovery::Unavailable {
        target: Some(target.clone()),
        reason,
    };
    match std::fs::metadata(&root) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return unavailable("AGENT_PROFILE_PATH_INVALID"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return if selected.is_some() {
                unavailable("AGENT_PROFILE_NOT_FOUND")
            } else {
                ProfileDiscovery::Missing
            };
        }
        Err(_) => return unavailable("AGENT_PROFILE_UNREADABLE"),
    }
    match std::fs::metadata(root.join("config.yaml")) {
        Ok(metadata) if metadata.is_file() => ProfileDiscovery::Found(target.clone()),
        Ok(_) => unavailable("AGENT_PROFILE_PATH_INVALID"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if selected.is_some() {
                unavailable("AGENT_PROFILE_CONFIG_MISSING")
            } else {
                ProfileDiscovery::Missing
            }
        }
        Err(_) => unavailable("AGENT_PROFILE_UNREADABLE"),
    }
}

pub(super) fn unreadable_hermes(target: AgentNativeTargetV1) -> AgentEntry {
    unavailable_hermes(Some(target), "AGENT_STATUS_UNAVAILABLE")
}

pub(super) fn unavailable_hermes(target: Option<AgentNativeTargetV1>, reason: &str) -> AgentEntry {
    AgentEntry {
        native_target: target,
        codex_access: None,
        agent_id: "agent_hermes_default".into(),
        version: String::new(),
        configuration_state: "profile_unavailable".into(),
        available_surfaces: vec![AgentModelSurfaceV2::HermesCli],
        native_model_catalog: None,
        context_id: None,
        settings: None,
        status_error: Some(reason.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hermes_default_discovery_is_read_only_and_uses_the_callers_root() {
        let home = tempfile::tempdir().unwrap();
        assert!(matches!(
            hermes_target_in(None, Some(home.path())),
            ProfileDiscovery::Missing
        ));
        assert!(!home.path().join(".hermes").exists());
        let root = home.path().join(".hermes");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("config.yaml"), "model: {}\n").unwrap();
        let ProfileDiscovery::Found(target) = hermes_target_in(None, Some(home.path())) else {
            panic!("default not discovered")
        };
        assert_eq!(target.config_root(), root.to_str().unwrap());
        assert_eq!(
            std::fs::read_to_string(root.join("config.yaml")).unwrap(),
            "model: {}\n"
        );
        assert!(!root.join("plugins").exists());
    }
    #[test]
    fn hermes_explicit_root_never_falls_back_to_another_profile() {
        let home = tempfile::tempdir().unwrap();
        let default = home.path().join(".hermes");
        std::fs::create_dir(&default).unwrap();
        std::fs::write(default.join("config.yaml"), "model: {}\n").unwrap();
        assert!(matches!(
            hermes_target_in(Some(OsStr::new("relative/profile")), Some(home.path())),
            ProfileDiscovery::Unavailable {
                reason: "AGENT_PROFILE_PATH_INVALID",
                ..
            }
        ));
        let root = home.path().join("other");
        let ProfileDiscovery::Unavailable {
            target: Some(target),
            reason: "AGENT_PROFILE_NOT_FOUND",
        } = hermes_target_in(Some(root.as_os_str()), Some(home.path()))
        else {
            panic!("unavailable explicit root was hidden")
        };
        assert_eq!(target.config_root(), root.to_str().unwrap());
        std::fs::create_dir(&root).unwrap();
        assert!(matches!(
            hermes_target_in(Some(root.as_os_str()), Some(home.path())),
            ProfileDiscovery::Unavailable {
                reason: "AGENT_PROFILE_CONFIG_MISSING",
                ..
            }
        ));
        std::fs::write(root.join("config.yaml"), "model: {}\n").unwrap();
        let ProfileDiscovery::Found(target) =
            hermes_target_in(Some(root.as_os_str()), Some(home.path()))
        else {
            panic!("explicit profile unavailable")
        };
        assert_eq!(target.config_root(), root.to_str().unwrap());
        let ProfileDiscovery::Found(target) =
            hermes_target_in(Some(OsStr::new("")), Some(home.path()))
        else {
            panic!("empty override did not use default")
        };
        assert_eq!(target.config_root(), default.to_str().unwrap());
    }
}
