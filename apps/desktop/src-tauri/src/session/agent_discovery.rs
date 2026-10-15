//! Desktop caller scope: resolve an existing default, then use the shared backend scan.
use super::AgentEntry;
use hiroute_domain::{AgentModelSurfaceV2, AgentNativeTargetV1};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(super) fn default_hermes_target() -> Option<AgentNativeTargetV1> {
    hermes_target_in(
        std::env::var_os("HERMES_HOME").as_deref(),
        std::env::var_os("HOME").as_deref().map(Path::new),
    )
}

fn hermes_target_in(selected: Option<&OsStr>, home: Option<&Path>) -> Option<AgentNativeTargetV1> {
    // An explicit Hermes root never falls back to a different profile when unavailable.
    let root = match selected.filter(|root| !root.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => home?.join(".hermes"),
    };
    if !root.is_absolute() || !root.is_dir() || !root.join("config.yaml").is_file() {
        return None;
    }
    Some(AgentNativeTargetV1::Hermes {
        config_root: root.to_str()?.to_owned(),
    })
}

pub(super) fn unreadable_hermes(target: AgentNativeTargetV1) -> AgentEntry {
    AgentEntry {
        native_target: Some(target),
        codex_access: None,
        agent_id: "agent_hermes_default".into(),
        version: String::new(),
        configuration_state: "profile_unavailable".into(),
        available_surfaces: vec![AgentModelSurfaceV2::HermesCli],
        native_model_catalog: None,
        context_id: None,
        settings: None,
        status_error: Some("AGENT_STATUS_UNAVAILABLE".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermes_default_discovery_is_read_only_and_uses_the_callers_root() {
        let home = tempfile::tempdir().unwrap();
        assert!(hermes_target_in(None, Some(home.path())).is_none());
        assert!(!home.path().join(".hermes").exists());
        let root = home.path().join(".hermes");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("config.yaml"), "model: {}\n").unwrap();
        let target = hermes_target_in(None, Some(home.path())).unwrap();
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
        assert!(
            hermes_target_in(Some(OsStr::new("relative/profile")), Some(home.path())).is_none()
        );
        let selected = home.path().join("other");
        assert!(hermes_target_in(Some(selected.as_os_str()), Some(home.path())).is_none());
        std::fs::create_dir(&selected).unwrap();
        std::fs::write(selected.join("config.yaml"), "model: {}\n").unwrap();
        assert_eq!(
            hermes_target_in(Some(selected.as_os_str()), Some(home.path()))
                .unwrap()
                .config_root(),
            selected.to_str().unwrap()
        );
        assert_eq!(
            hermes_target_in(Some(OsStr::new("")), Some(home.path()))
                .unwrap()
                .config_root(),
            default.to_str().unwrap()
        );
    }
}
