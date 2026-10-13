//! Opt-in real native Windows clients. Missing installed artifacts fail the selected gate.
//! This endpoint issues no provider request and receives only a fresh local challenge.
use super::*;
use hiroute_domain::WorkerDependencySelectionRecordV1;
use hiroute_domain::delegation::WorkerToolV1;

fn selected(name: &str) -> std::path::PathBuf {
    std::env::var_os(name)
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .unwrap_or_else(|| panic!("{name} must select the fixed native artifact"))
}

fn config(harness: WorkerHarnessV1, variable: &str) -> WorkerInstallationConfig {
    let binary = selected(variable);
    // npm declarations resolve to their actual entry; generated cmd scripts are never run.
    let binary = if binary
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"))
    {
        hiroute_integrations::agents::windows_npm_entry(&binary).unwrap()
    } else {
        binary
    };
    WorkerInstallationConfig {
        harness,
        adapter: match harness {
            WorkerHarnessV1::CodexCli => Some(selected("HIROUTE_WORKER_CODEX_ACP_ADAPTER")),
            WorkerHarnessV1::ClaudeCode => Some(selected("HIROUTE_WORKER_CLAUDE_ACP_ADAPTER")),
            _ => None,
        },
        harness_binary: binary,
        node_binary: matches!(
            harness,
            WorkerHarnessV1::Pi | WorkerHarnessV1::CodexCli | WorkerHarnessV1::ClaudeCode
        )
        .then(|| selected("HIROUTE_WORKER_NODE")),
    }
}

#[allow(clippy::too_many_arguments)]
fn profile(
    config: &WorkerInstallationConfig,
    root: &ProbeRoot,
    session: &TaskSessionRoot,
    context: &NativeWorkerContext,
    gateway: std::net::SocketAddr,
    token: ProtectedSecret,
    run: &str,
) -> Result<CandidateWorkerProfile, DelegationErrorV1> {
    let now = now_ms()?;
    let secret = zeroize::Zeroizing::new(token.expose().to_vec());
    let catalog =
        (config.harness == WorkerHarnessV1::CodexCli).then(|| test_codex_catalog(PROBE_MODEL));
    let tools = vec![WorkerToolV1::Read, WorkerToolV1::Edit, WorkerToolV1::Shell];
    let execution = WorkerExecutionIntentV1 {
        root_identity: "root".into(),
        access: WorkspaceAccessV1::TrustedNative,
        tools: tools.clone(),
        network: WorkerNetworkV1::Allowed,
        duration_ms: PROBE_DEADLINE.as_millis() as u64,
        delegation_depth: 1,
    };
    let permit = WorkspaceExecutionPermitV1 {
        permit_id: "native-windows-probe".into(),
        generation: 1,
        root_identity: "root".into(),
        access: execution.access,
        tools,
        network: execution.network,
        expires_at_ms: now + execution.duration_ms,
        max_run_ms: execution.duration_ms,
        max_concurrent: 1,
        revoked: false,
    };
    let profile = CandidateWorkerProfile::build(ProfileInput {
        protocol: hiroute_domain::AgentIngressProtocolV1::Responses,
        context_window_tokens: Some(if config.harness == WorkerHarnessV1::ClaudeCode {
            200_000
        } else {
            32_768
        }),
        max_output_tokens: Some(4_096),
        harness: config.harness,
        adapter: config.adapter.as_deref(),
        harness_binary: &config.harness_binary,
        node_binary: config.node_binary.as_deref(),
        private_root: &root.runs.join(run),
        session_root: session,
        native_context: context,
        workspace: &root.workspace,
        alias: PROBE_MODEL,
        codex_catalog: catalog.as_deref(),
        native_effort: None,
        gateway,
        permit: &permit,
        execution: &execution,
        permission_policy: WorkerPermissionPolicyV1::ApproveAll,
        admitted_at_ms: now,
        token,
    })?;
    assert!(
        profile
            .executable
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    );
    if matches!(
        config.harness,
        WorkerHarnessV1::Pi
            | WorkerHarnessV1::DeepseekHarness
            | WorkerHarnessV1::CodexCli
            | WorkerHarnessV1::ClaudeCode
    ) {
        let node = selected("HIROUTE_WORKER_NODE");
        let node = hiroute_platform::windows::security::open_identity(&node, false).unwrap();
        let actual =
            hiroute_platform::windows::security::open_identity(&profile.executable, false).unwrap();
        assert_eq!(
            hiroute_platform::windows::security::file_identity(&actual).unwrap(),
            hiroute_platform::windows::security::file_identity(&node).unwrap(),
            "the real selected Node executable must run the SDK/ACP entry"
        );
        if config.harness == WorkerHarnessV1::DeepseekHarness {
            assert_eq!(
                std::fs::canonicalize(&profile.args[0]).unwrap(),
                std::fs::canonicalize(&config.harness_binary).unwrap(),
                "the actual DSH JS entry must be the first native Node argument"
            );
        }
    }
    assert_eq!(
        profile.env["HOME"].as_str(),
        context.home().to_str().unwrap()
    );
    assert_eq!(
        profile.env["USERPROFILE"].as_str(),
        context.home().to_str().unwrap()
    );
    assert!(profile.env.keys().all(|key| !matches!(
        key.as_str(),
        "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "NODE_OPTIONS"
    )));
    for file in &profile.materials.files {
        assert!(
            !file
                .contents
                .windows(secret.len())
                .any(|part| part == secret.as_slice()),
            "run credential must remain outside material files"
        );
    }
    if config.harness == WorkerHarnessV1::DeepseekHarness {
        let bytes = profile
            .materials
            .files
            .iter()
            .find(|file| file.relative_path == Path::new("dsh-worker.patch.yml"))
            .unwrap();
        let patch: Value = serde_json::from_slice(&bytes.contents).unwrap();
        assert!(
            patch
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == "tool-pwsh"
                    && entry["config"]["enableRunInBackground"] == false
                    && entry["config"]["promoteOnTimeout"] == false)
        );
        assert!(
            !patch
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == "tool-bash")
        );
    }
    Ok(profile)
}

fn native_flow(config: WorkerInstallationConfig) -> Result<(), DelegationErrorV1> {
    // The selected producer is executed here, rather than a protocol fixture.
    WorkerDependencySelectionRecordV1 {
        harness: config.harness,
        adapter_path: None,
        cli_path: config.harness_binary.to_string_lossy().into_owned(),
        node_path: config
            .node_binary
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
    }
    .validated_launch()
    .map_err(|_| DelegationErrorV1::DependenciesInvalid)?;
    check_installation(&config)?;
    eprintln!(
        "native Windows acceptance checkpoint: entries-checked harness={:?}",
        config.harness
    );
    let challenge = fresh_challenge()?;
    let server = ProbeServer::bind(challenge.clone(), PROBE_MODEL, config.harness)?;
    let root = ProbeRoot::new()?;
    let home = root._root.path().join("borrowed-home");
    let native_config = home.join("native-config");
    create_private(&home)?;
    create_private(&native_config)?;
    let context = NativeWorkerContext::borrowed(&home, &native_config)?;
    let session = TaskSessionRoot::prepare(
        &root.sessions,
        &WorkspaceId::parse("native-windows-probe").unwrap(),
        "root",
        "task",
        config.harness,
        SessionRootUse::New,
    )?;
    session.bind_borrowed_context(context.home(), context.config_root(), &root.workspace)?;
    eprintln!(
        "native Windows acceptance checkpoint: private-context-bound harness={:?}",
        config.harness
    );
    let platform = LocalWorkerPlatform::default();
    let first = run_normal(
        &platform,
        config.harness,
        profile(
            &config,
            &root,
            &session,
            &context,
            server.address(),
            protected_challenge(&challenge)?,
            "new",
        )?,
        AcpSessionStart::New,
        "native-new",
        None,
    )?;
    let native_session_id = first
        .session
        .native_session_id
        .as_deref()
        .ok_or(DelegationErrorV1::ResumeUnavailable)?;
    eprintln!(
        "native Windows acceptance checkpoint: new-proved harness={:?} local-requests={}",
        config.harness,
        server.request_count()
    );
    let history = crate::delegation::profile::native_history(
        session.path(),
        config.harness,
        native_session_id,
    )?;
    let required = history
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let continued = TaskSessionRoot::prepare(
        &root.sessions,
        &WorkspaceId::parse("native-windows-probe").unwrap(),
        "root",
        "task",
        config.harness,
        SessionRootUse::Continue {
            required_history: &required,
        },
    )?;
    continued.verify_native_session(native_session_id)?;
    let loaded = run_normal(
        &platform,
        config.harness,
        profile(
            &config,
            &root,
            &continued,
            &context,
            server.address(),
            protected_challenge(&challenge)?,
            "load",
        )?,
        AcpSessionStart::Load(first.session.clone()),
        "native-load",
        None,
    )?;
    if loaded.session != first.session {
        return Err(DelegationErrorV1::ResumeUnavailable);
    }
    eprintln!(
        "native Windows acceptance checkpoint: exact-load-proved harness={:?} local-requests={}",
        config.harness,
        server.request_count()
    );
    run_cancel_session(
        &platform,
        profile(
            &config,
            &root,
            &continued,
            &context,
            server.address(),
            protected_challenge(&challenge)?,
            "cancel",
        )?,
        "native-cancel",
        &server,
        AcpSessionStart::Load(loaded.session),
    )?;
    assert!(
        server.request_count() >= 3,
        "new/load/cancel must reach the authenticated local endpoint"
    );
    eprintln!(
        "native Windows acceptance checkpoint: cancel-and-cleanup-proved harness={:?} local-requests={}",
        config.harness,
        server.request_count()
    );
    server.finish()
}

#[test]
#[ignore = "requires fixed official Pi and Node artifacts; selected CI gate must run --ignored --exact"]
fn actual_windows_pi_new_load_and_cancel_use_the_selected_sdk() {
    native_flow(config(WorkerHarnessV1::Pi, "HIROUTE_WORKER_PI_BINARY")).unwrap();
}
#[test]
#[ignore = "requires fixed official DSH artifact; selected CI gate must run --ignored --exact"]
fn actual_windows_dsh_new_load_and_cancel_use_the_selected_native_acp() {
    native_flow(config(
        WorkerHarnessV1::DeepseekHarness,
        "HIROUTE_WORKER_DSH_BINARY",
    ))
    .unwrap();
}
#[test]
#[ignore = "requires fixed native Windows Qoder artifact; no provider credential or prior login is supplied"]
fn actual_windows_qoder_new_load_and_cancel_use_the_selected_native_acp() {
    native_flow(config(
        WorkerHarnessV1::QoderCli,
        "HIROUTE_WORKER_QODER_BINARY",
    ))
    .unwrap();
}

#[path = "windows_tools.rs"]
mod windows_tools;
