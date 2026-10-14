//! Public wire settings with real stores/files; native SDK acceptance is a separate gate.
use super::*;
use std::path::{Path, PathBuf};

fn private_file(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

const ORIGINAL: &[u8] = b"# user config\nmodel:\n  default: native/model\n  provider: native\n  context_length: 64000\n  api_key: synthetic-native-key\nproviders:\n  native:\n    api: https://native.invalid/v1\n    transport: chat_completions\nauxiliary:\n  vision:\n    model: native/vision\nchannels:\n  discord: unchanged\n";

fn isolate(test: &str) -> bool {
    let source = crate::test_support::private_tempdir();
    private_file(&source.path().join("hermes"), b"#!/bin/sh\nexit 97\n");
    fs::set_permissions(
        source.path().join("hermes"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    private_file(
        &source.path().join("hermes_cli/plugins.py"),
        b"def register_middleware(): pass\n# plugins\n",
    );
    private_file(
        &source.path().join("hermes_cli/middleware.py"),
        b"def apply_llm_request_middleware(): pass\n",
    );
    private_file(&source.path().join("agent/conversation_loop.py"), b"apply_llm_request_middleware(request, session_id=agent.session_id, model=agent.model, provider=agent.provider, base_url=agent.base_url, api_mode=agent.api_mode)\n");
    private_file(
        &source.path().join("tools/skills_tool.py"),
        b"# structural fixture, never executed\n",
    );
    private_file(
        &source.path().join("pyproject.toml"),
        b"[project]\nversion = \"0.19.1\"\n",
    );
    let path =
        std::env::join_paths([source.path(), Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    crate::test_support::isolated_agent_home_with_path(test, &path)
}

struct Fixture {
    root: tempfile::TempDir,
    profile: PathBuf,
    runtime: ProductionControlRuntime,
    service: LocalControlDaemon,
    scanner: FilesystemAgentScannerV1,
    context: String,
}
impl Fixture {
    fn new(models: bool) -> Self {
        let root = crate::test_support::private_tempdir();
        let profile = root.path().join("chosen-profile");
        private_file(&profile.join("config.yaml"), ORIGINAL);
        private_file(
            &profile.join("sessions/preserved"),
            b"unrelated native session",
        );
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let mut layout = AgentFilesystemLayoutV1::from_process(&home, root.path());
        layout.codex_executable = root.path().join("missing-codex");
        layout.claude_executable = root.path().join("missing-claude");
        layout.qoder_executable = root.path().join("missing-qoder");
        layout.pi_executable = root.path().join("missing-pi");
        layout.dsh_executable = root.path().join("missing-dsh");
        let registry = serde_json::from_slice(include_bytes!(
            "../../../../../assets/connector-registry/current/registry-seed.json"
        ))
        .unwrap();
        let models_data: hiroute_domain::ReleaseModelDataBundleV2 = serde_json::from_slice(
            include_bytes!("../../../../../assets/release-facts/current/bundle/model-data.json"),
        )
        .unwrap();
        let scanner = FilesystemAgentScannerV1::new(
            layout,
            ClaudeRegistrationIndexV1::from_verified_model_data(&registry, &models_data.data)
                .unwrap(),
        );
        let storage = root.path().join("storage");
        let runtime = ProductionControlRuntime::prepare_for_role_all_with_scanner(
            &storage,
            crate::release_catalog::fixture_catalog(),
            None,
            scanner.clone(),
        )
        .unwrap();
        let runtime = if models {
            configure_runtime_with_initial(&storage, runtime, golden_publication_plans_only())
        } else {
            runtime
        };
        runtime.adapter.reconcile_startup_and_open().unwrap();
        let service = LocalControlDaemon::new(ApplicationService::new(runtime.application_ports()));
        let context = hiroute_integrations::hermes_context_id(profile.to_str().unwrap());
        Self {
            root,
            profile,
            runtime,
            service,
            scanner,
            context,
        }
    }
    fn base(&self) -> serde_json::Value {
        json!({"schema_version":{"major":2,"minor":0}, "context_id":self.context,
            "native_target":{"kind":"hermes","config_root":self.profile}, "model":{"intent":"keep"}})
    }
    fn model_spec(&self, index: usize) -> serde_json::Value {
        let publication = self
            .runtime
            .adapter
            .stores_lock()
            .unwrap()
            .control()
            .active_publication(&WorkspaceId::default())
            .unwrap()
            .unwrap()
            .verify()
            .unwrap();
        let plans: Vec<_> = publication
            .published_agent_plans()
            .unwrap()
            .into_iter()
            .filter(|p| {
                p.active
                    && p.supported_ingress
                        .contains(&AgentIngressProtocolV1::Responses)
            })
            .collect();
        let mut spec = self.base();
        spec["model"] = json!({"intent":"configure","settings":{"mode":"hermes_default","plan_id":plans[index].agent_plan_id,"protocol":"responses"}});
        spec
    }
    fn status(&self) -> serde_json::Value {
        let result = self.service.dispatch_wire(request(
            "GetAgentConnectionStatus",
            json!({"schema_version":{"major":2,"minor":0},"context_id":self.context}),
            None,
        ));
        assert!(result.error.is_none(), "{result:?}");
        result.data.unwrap()
    }
    fn restore(&self) -> serde_json::Value {
        json!({"schema_version":{"major":2,"minor":0},"context_id":self.context,"model":{"intent":"restore","restore_point_ref":self.status()["restore_point_ref"]}})
    }
    fn reopen(self) -> Self {
        let Self {
            root,
            profile,
            runtime,
            service,
            scanner,
            context,
        } = self;
        drop(service);
        drop(runtime);
        let storage = root.path().join("storage");
        let runtime = ProductionControlRuntime::prepare_for_role_all_with_scanner(
            &storage,
            crate::release_catalog::fixture_catalog(),
            None,
            scanner.clone(),
        )
        .unwrap();
        let runtime =
            configure_runtime_with_initial(&storage, runtime, golden_publication_plans_only());
        runtime.adapter.reconcile_startup_and_open().unwrap();
        let service = LocalControlDaemon::new(ApplicationService::new(runtime.application_ports()));
        Self {
            root,
            profile,
            runtime,
            service,
            scanner,
            context,
        }
    }
}

#[test]
fn management_only_is_independent_and_recoverable_without_model_publication() {
    if isolate(
        "control::runtime::native_model::tests::settings_entry_tests::hermes::management_only_is_independent_and_recoverable_without_model_publication",
    ) {
        return;
    }
    let fixture = Fixture::new(false);
    let mut spec = fixture.base();
    spec["management_skill"] = json!({"intent":"configure","settings":{}});
    // Malformed unrelated model config does not become a management-Skill dependency.
    private_file(
        &fixture.profile.join("config.yaml"),
        b"model: [invalid-for-model]\n",
    );
    apply(
        &fixture.service,
        &fixture.runtime,
        spec,
        "hermes-skill-only",
    );
    assert_eq!(fixture.status()["state"], "not_configured");
    assert_eq!(fixture.status()["management_skill"]["state"], "configured");
    assert!(fixture.status().get("collaboration").is_none());
    assert_eq!(
        fs::read(fixture.profile.join("skills/hiroute-management/SKILL.md")).unwrap(),
        include_bytes!("../../../../../assets/skills/hiroute-management/SKILL.md")
    );
    let stores = fixture.runtime.adapter.stores_lock().unwrap();
    assert!(
        stores
            .control()
            .active_publication(&WorkspaceId::default())
            .unwrap()
            .is_none()
    );
    assert!(
        stores
            .secrets()
            .inspect_agent_access_grant(
                WorkspaceId::DEFAULT,
                &format!("agent-connection/{}", fixture.context)
            )
            .unwrap()
            .is_none()
    );
    drop(stores);
    let spec = json!({"schema_version":{"major":2,"minor":0},"context_id":fixture.context,
        "management_skill":{"intent":"restore","restore_point_ref":fixture.status()["management_skill"]["restore_point_ref"]}});
    let preview = fixture.service.dispatch_wire(request(
        "PreviewAgentConnectionRestore",
        json!({"spec":spec}),
        None,
    ));
    assert!(preview.error.is_none(), "{preview:?}");
    let preview = preview.data.unwrap();
    let result = fixture.service.dispatch_wire(request("ApplyAgentConnectionRestore", json!({"spec":spec,"accept_digest":preview["accept_digest"],"dependency_digest":preview["dependency_digest"],"expected_revisions":preview["expected_revisions"],"idempotency_key":"hermes-skill-remove"}), None));
    assert!(result.error.is_none(), "{result:?}");
    assert_eq!(result.data.unwrap()["state"], "succeeded");
    assert!(!fixture.profile.join("skills/hiroute-management").exists());
    assert_eq!(
        fs::read(fixture.profile.join("config.yaml")).unwrap(),
        b"model: [invalid-for-model]\n"
    );
}

#[test]
fn model_resources_rehydrate_after_restart_and_restore_without_touching_skill() {
    if isolate(
        "control::runtime::native_model::tests::settings_entry_tests::hermes::model_resources_rehydrate_after_restart_and_restore_without_touching_skill",
    ) {
        return;
    }
    let fixture = Fixture::new(true);
    let mut spec = fixture.model_spec(0);
    spec["management_skill"] = json!({"intent":"configure","settings":{}});
    apply(&fixture.service, &fixture.runtime, spec, "hermes-main");
    assert_eq!(
        fs::read(fixture.profile.join("plugins/hiroute/__init__.py")).unwrap(),
        hiroute_integrations::HERMES_PLUGIN_MODULE.as_bytes()
    );
    assert_eq!(fixture.status()["model_verified"], false);
    let fixture = fixture.reopen();
    assert_eq!(fixture.status()["state"], "configured");
    let spec = fixture.restore();
    apply(
        &fixture.service,
        &fixture.runtime,
        spec,
        "hermes-restore-after-restart",
    );
    assert_eq!(
        fs::read(fixture.profile.join("config.yaml")).unwrap(),
        ORIGINAL
    );
    assert!(!fixture.profile.join("plugins/hiroute").exists());
    assert_eq!(fixture.status()["management_skill"]["state"], "configured");
    assert_eq!(
        fs::read(fixture.profile.join("sessions/preserved")).unwrap(),
        b"unrelated native session"
    );
}

#[test]
fn borrowed_plugin_files_survive_restore_and_foreign_collision_blocks_before_write() {
    if isolate(
        "control::runtime::native_model::tests::settings_entry_tests::hermes::borrowed_plugin_files_survive_restore_and_foreign_collision_blocks_before_write",
    ) {
        return;
    }
    let fixture = Fixture::new(true);
    let module = fixture.profile.join("plugins/hiroute/__init__.py");
    let manifest = fixture.profile.join("plugins/hiroute/plugin.yaml");
    private_file(&module, b"foreign plugin");
    let preview = fixture.service.dispatch_wire(request(
        "PreviewAgentConnectionChange",
        json!({"spec":fixture.model_spec(0)}),
        None,
    ));
    assert_eq!(preview.data.unwrap()["applicable"], false);
    assert_eq!(
        fs::read(fixture.profile.join("config.yaml")).unwrap(),
        ORIGINAL
    );
    private_file(
        &module,
        hiroute_integrations::HERMES_PLUGIN_MODULE.as_bytes(),
    );
    private_file(
        &manifest,
        hiroute_integrations::HERMES_PLUGIN_MANIFEST.as_bytes(),
    );
    apply(
        &fixture.service,
        &fixture.runtime,
        fixture.model_spec(0),
        "hermes-borrow",
    );
    apply(
        &fixture.service,
        &fixture.runtime,
        fixture.restore(),
        "hermes-borrow-restore",
    );
    assert!(module.exists() && manifest.exists());
    assert_eq!(
        fs::read(&module).unwrap(),
        hiroute_integrations::HERMES_PLUGIN_MODULE.as_bytes()
    );
}

#[test]
fn reconfigure_then_conditional_restore_preserves_foreign_edits_and_rejects_target_rebind() {
    if isolate(
        "control::runtime::native_model::tests::settings_entry_tests::hermes::reconfigure_then_conditional_restore_preserves_foreign_edits_and_rejects_target_rebind",
    ) {
        return;
    }
    let fixture = Fixture::new(true);
    apply(
        &fixture.service,
        &fixture.runtime,
        fixture.model_spec(0),
        "hermes-plan-0",
    );
    apply(
        &fixture.service,
        &fixture.runtime,
        fixture.model_spec(1),
        "hermes-plan-1",
    );
    let mut changed = fs::read(fixture.profile.join("config.yaml")).unwrap();
    changed.extend_from_slice(b"user_note: retained\n");
    private_file(&fixture.profile.join("config.yaml"), &changed);
    let different = fixture.root.path().join("another-profile");
    fs::create_dir(&different).unwrap();
    let mut spec = fixture.model_spec(0);
    spec["native_target"]["config_root"] = json!(different);
    let rejected = fixture.service.dispatch_wire(request(
        "PreviewAgentConnectionChange",
        json!({"spec":spec}),
        None,
    ));
    assert!(rejected.error.is_some());
    apply(
        &fixture.service,
        &fixture.runtime,
        fixture.restore(),
        "hermes-conditional-restore",
    );
    let restored =
        String::from_utf8(fs::read(fixture.profile.join("config.yaml")).unwrap()).unwrap();
    assert!(
        restored.contains("\"default\": \"native/model\""),
        "{restored}"
    );
    assert!(restored.contains("user_note: retained"), "{restored}");
    assert!(restored.contains("  discord: unchanged"), "{restored}");
}
