//! Required opt-in gates execute fixed official clients through the production Worker lifecycle.
//! The local model specifies tools; the native clients must discover resources, run tools,
//! persist exact history and produce independently checked filesystem effects.
use super::super::native_tools_probe::{
    CONTINUE_TOOLS, ToolsFixture, ToolsOracle, observe_claude_history,
};
use super::*;

fn native_tools(config: WorkerInstallationConfig) -> Result<(), DelegationErrorV1> {
    config
        .selection_record()?
        .validated_launch()
        .map_err(|_| DelegationErrorV1::DependenciesInvalid)?;
    check_installation(&config)?;
    eprintln!(
        "native Windows tools checkpoint: entries-checked harness={:?}",
        config.harness
    );
    let root = ProbeRoot::new()?;
    let home = root._root.path().join("borrowed-home");
    let native_config = home.join("native-config");
    create_private(&home)?;
    create_private(&native_config)?;
    let account = (config.harness == WorkerHarnessV1::QoderCli)
        .then(QoderAccount::selected)
        .transpose()?;
    let context = account
        .as_ref()
        .map(|account| Ok(account.context.clone()))
        .unwrap_or_else(|| NativeWorkerContext::borrowed(&home, &native_config))?;
    let fixture = ToolsFixture::prepare(
        config.harness,
        context.home(),
        context.config_root(),
        &root.workspace,
    )?;
    let prompt = fixture.prompt();
    let oracle = Arc::new(Mutex::new(ToolsOracle::new(fixture)));
    let responder = Arc::clone(&oracle);
    let challenge = fresh_challenge()?;
    let server = ProbeServer::bind_with_response(
        challenge.clone(),
        PROBE_MODEL,
        config.harness,
        Arc::new(move |body| {
            responder
                .lock()
                .map_err(|_| DelegationErrorV1::ProtocolFailed)?
                .respond(body)
        }),
    )?;
    let workspace_id = WorkspaceId::parse("native-windows-tools").unwrap();
    let session = TaskSessionRoot::prepare(
        &root.sessions,
        &workspace_id,
        "root",
        "tools",
        config.harness,
        SessionRootUse::New,
    )?;
    session.bind_borrowed_context(context.home(), context.config_root(), &root.workspace)?;
    let platform = LocalWorkerPlatform::default();
    let first = run_normal_prompt(
        &platform,
        config.harness,
        profile(
            &config,
            &root,
            &session,
            &context,
            server.address(),
            protected_challenge(&challenge)?,
            "tools-new",
        )?,
        AcpSessionStart::New,
        "native-tools-new",
        None,
        &prompt,
    )?;
    oracle
        .lock()
        .map_err(|_| DelegationErrorV1::ProtocolFailed)?
        .assert_effects()?;
    eprintln!(
        "native Windows tools checkpoint: native-effects-proved harness={:?} local-requests={}",
        config.harness,
        server.request_count()
    );
    let native_session_id = first
        .session
        .native_session_id
        .as_deref()
        .ok_or(DelegationErrorV1::ResumeUnavailable)?;
    if let Some(account) = &account {
        account.verify()?;
    }
    let history = crate::delegation::profile::native_history(
        session.path(),
        config.harness,
        native_session_id,
    )?;
    if config.harness == WorkerHarnessV1::ClaudeCode {
        let transcript = TaskSessionRoot::native_transcript_path(
            session.path(),
            config.harness,
            native_session_id,
        )?;
        observe_claude_history(&transcript);
    }
    let required = history
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let continued = TaskSessionRoot::prepare(
        &root.sessions,
        &workspace_id,
        "root",
        "tools",
        config.harness,
        SessionRootUse::Continue {
            required_history: &required,
        },
    )?;
    continued.verify_native_session(native_session_id)?;
    let loaded = run_normal_prompt(
        &platform,
        config.harness,
        profile(
            &config,
            &root,
            &continued,
            &context,
            server.address(),
            protected_challenge(&challenge)?,
            "tools-load",
        )?,
        AcpSessionStart::native_continuation(config.harness, first.session.clone()),
        "native-tools-load",
        None,
        CONTINUE_TOOLS,
    )?;
    oracle
        .lock()
        .map_err(|_| DelegationErrorV1::ProtocolFailed)?
        .assert_effects()?;
    if loaded.session != first.session
        || !oracle
            .lock()
            .map_err(|_| DelegationErrorV1::ProtocolFailed)?
            .loaded
    {
        return Err(DelegationErrorV1::ResumeUnavailable);
    }
    eprintln!(
        "native Windows tools checkpoint: exact-tool-history-proved harness={:?} local-requests={}",
        config.harness,
        server.request_count()
    );
    if let Some(account) = &account {
        account.verify()?;
    }
    assert!(
        server.request_count() >= 7,
        "both native Skills, Read, Edit, Shell, terminal and loaded history require nonzero actual requests"
    );
    server.finish()
}

#[test]
#[ignore = "fixed real Codex/ACP/Node/daemon artifacts; required Windows CI selects exact ignored case"]
fn actual_windows_codex_skills_read_edit_shell_and_exact_history() {
    native_tools(config(
        WorkerHarnessV1::CodexCli,
        "HIROUTE_WORKER_CODEX_BINARY",
    ))
    .unwrap();
}
#[test]
#[ignore = "fixed real Claude/ACP/Node artifacts; required Windows CI selects exact ignored case"]
fn actual_windows_claude_skills_read_edit_shell_and_exact_history() {
    native_tools(config(
        WorkerHarnessV1::ClaudeCode,
        "HIROUTE_WORKER_CLAUDE_BINARY",
    ))
    .unwrap();
}
#[test]
#[ignore = "fixed real Pi/Node artifacts; required Windows CI selects exact ignored case"]
fn actual_windows_pi_skills_read_edit_shell_and_exact_history() {
    native_tools(config(WorkerHarnessV1::Pi, "HIROUTE_WORKER_PI_BINARY")).unwrap();
}
#[test]
#[ignore = "fixed real DSH/Node artifacts; required Windows CI selects exact ignored case"]
fn actual_windows_dsh_skills_read_edit_shell_and_exact_history() {
    native_tools(config(
        WorkerHarnessV1::DeepseekHarness,
        "HIROUTE_WORKER_DSH_BINARY",
    ))
    .unwrap();
}
#[test]
#[ignore = "fixed real Qoder and explicit official logged-in private profile; missing account input fails"]
fn actual_windows_qoder_skills_read_edit_shell_and_exact_history() {
    native_tools(config(
        WorkerHarnessV1::QoderCli,
        "HIROUTE_WORKER_QODER_BINARY",
    ))
    .unwrap();
}
