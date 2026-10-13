//! Native main-client preflight through the actual production CollaborationProbe.
//! This proves native Skill + public CLI execution, not worker delegation or account OAuth.
use super::*;
use std::path::PathBuf;

fn selected(variable: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .unwrap_or_else(|| panic!("{variable} must select the fixed actual native executable"))
}

#[test]
#[ignore = "fixed actual Codex and this candidate hiroute.exe; required CI selects exactly one case"]
fn actual_windows_codex_native_skill_executes_the_public_cli() {
    CodexNativeIngressProbe::bind()
        .unwrap()
        .run_collaboration(
            &selected("HIROUTE_WORKER_CODEX_BINARY"),
            &selected("HIROUTE_WORKER_CLI_BINARY"),
        )
        .unwrap();
}

#[test]
#[ignore = "fixed actual Claude and this candidate hiroute.exe; required CI selects exactly one case"]
fn actual_windows_claude_native_skill_executes_the_public_cli() {
    ClaudeNativeIngressProbe::run_collaboration(
        &selected("HIROUTE_WORKER_CLAUDE_BINARY"),
        &selected("HIROUTE_WORKER_CLI_BINARY"),
    )
    .unwrap();
}

#[test]
#[ignore = "fixed actual Qoder and this candidate hiroute.exe; absent native login fails rather than synthesizing an account"]
fn actual_windows_qoder_native_skill_executes_the_public_cli_without_synthesized_account() {
    let root = probe_files::temporary("hiroute-qoder-native-main-").unwrap();
    let home = root.path().join("home");
    let config = home.join(".qoder");
    probe_files::directory(&home).unwrap();
    probe_files::directory(&config).unwrap();
    let context = QoderNativeContext::from_selected(&home, Some(&config)).unwrap();
    QoderCollaborationProbe::run(
        &selected("HIROUTE_WORKER_QODER_BINARY"),
        &context,
        &selected("HIROUTE_WORKER_CLI_BINARY"),
        QoderCollaborationProbeTarget::PreinstallCapability,
    )
    .unwrap();
    // No borrowed credentials or user Skill/settings exist in this owned context.
    assert!(!context.skill_target().exists());
    assert!(!config.join("settings.json").exists());
}
