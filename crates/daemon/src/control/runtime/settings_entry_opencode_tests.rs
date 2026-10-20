//! Shared public settings transactions with OpenCode's loaded user layers and real recovery.
use super::models::{model_spec, model_status, restore_spec};
use super::*;

fn configure_models(fixture: &QoderFixture) {
    let mut spec = model_spec(fixture);
    spec["model"]["settings"]["mode"] = json!("opencode_additional");
    apply(&fixture.service, &fixture.runtime, spec, "opencode-models");
}

#[test]
fn opencode_pending_file_tail_retries_the_original_context_bound_operation() {
    if crate::test_support::isolated_agent_home(
        "control::runtime::native_model::tests::settings_entry_tests::qoder::opencode::opencode_pending_file_tail_retries_the_original_context_bound_operation",
    ) {
        return;
    }
    super::models::pending_file_tail_retries_exact_operation(AgentKindV1::OpenCode);
}

#[test]
fn opencode_review_new_jsonc_keeps_managed_json_binding_after_restart_and_restore() {
    if crate::test_support::isolated_agent_home(
        "control::runtime::native_model::tests::settings_entry_tests::qoder::opencode::opencode_review_new_jsonc_keeps_managed_json_binding_after_restart_and_restore",
    ) {
        return;
    }
    let mut fixture = QoderFixture::with_kind(true, AgentKindV1::OpenCode);
    let original = fs::read(&fixture.native_config).unwrap();
    configure_models(&fixture);
    let status = model_status(&fixture);
    let managed = fs::read(&fixture.native_config).unwrap();
    let added = fixture.native_config.with_extension("jsonc");
    let unrelated = br#"{"$schema":"https://opencode.ai/config.json","instructions":["new.md"]}"#;
    private_file(&added, unrelated);
    let storage = fixture._root.path().join("storage");
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let mut layout = AgentFilesystemLayoutV1::from_process(&home, fixture._root.path());
    layout.opencode_context =
        Some(hiroute_integrations::OpenCodeNativeContext::from_lookup(&home, |_| None).unwrap());
    let registry = serde_json::from_slice(include_bytes!(
        "../../../../../assets/connector-registry/current/registry-seed.json"
    ))
    .unwrap();
    let model_data: hiroute_domain::ReleaseModelDataBundleV2 = serde_json::from_slice(
        include_bytes!("../../../../../assets/release-facts/current/bundle/model-data.json"),
    )
    .unwrap();
    let scanner = FilesystemAgentScannerV1::new(
        layout,
        ClaudeRegistrationIndexV1::from_verified_model_data(&registry, &model_data.data).unwrap(),
    );
    drop(fixture.service);
    drop(fixture.runtime);
    fixture.runtime = open_with_scanner(&storage, scanner);
    fixture
        .runtime
        .adapter
        .reconcile_startup_and_open()
        .unwrap();
    fixture.service = LocalControlDaemon::new(ApplicationService::new(
        fixture
            .runtime
            .application_ports()
            .with_agent_connection(Arc::new(FixtureFacts::collaboration(
                fixture.runtime.adapter.clone(),
            ))),
    ));
    assert_eq!(
        fixture
            .runtime
            .adapter
            .scanner
            .opencode_user_models_target(),
        fixture.native_config
    );
    assert_eq!(fs::read(&fixture.native_config).unwrap(), managed);
    assert_eq!(model_status(&fixture)["state"], "configured");
    apply(
        &fixture.service,
        &fixture.runtime,
        restore_spec(&fixture, &status),
        "opencode-restarted-restore",
    );
    assert_eq!(model_status(&fixture)["state"], "not_configured");
    assert_eq!(fs::read(&fixture.native_config).unwrap(), original);
    assert_eq!(fs::read(added).unwrap(), unrelated);
}

#[test]
fn opencode_review_status_reports_relevant_user_layer_drift_only() {
    if crate::test_support::isolated_agent_home(
        "control::runtime::native_model::tests::settings_entry_tests::qoder::opencode::opencode_review_status_reports_relevant_user_layer_drift_only",
    ) {
        return;
    }
    let fixture = QoderFixture::with_kind(true, AgentKindV1::OpenCode);
    configure_models(&fixture);
    let original_status = model_status(&fixture);
    let managed = fs::read(&fixture.native_config).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&managed).unwrap();
    let provider = value["provider"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap();
    let overlay = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".opencode/opencode.jsonc");
    let unrelated = br#"{"$schema":"https://opencode.ai/config.json","instructions":["other.md"],"provider":{"daily":{}}}"#;
    for conflict in [
        json!({"provider":{(provider):{"options":{"baseURL":"https://example.invalid/v1"}}}}),
        json!({"enabled_providers":["daily"]}),
        json!({"disabled_providers":[provider]}),
    ] {
        private_file(&overlay, unrelated);
        assert_eq!(model_status(&fixture)["state"], "configured");
        private_file(&overlay, &serde_json::to_vec(&conflict).unwrap());
        let status = model_status(&fixture);
        assert_eq!(status["state"], "drift", "{conflict}");
        assert_eq!(
            status["restore_point_ref"],
            original_status["restore_point_ref"]
        );
        assert_eq!(fs::read(&fixture.native_config).unwrap(), managed);
    }
    private_file(&overlay, unrelated);
    assert_eq!(model_status(&fixture)["state"], "configured");
}
