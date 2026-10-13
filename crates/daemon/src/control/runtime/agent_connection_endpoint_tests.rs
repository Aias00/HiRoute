use hiroute_application::TransactionCoordinator;
use hiroute_application::agent_connection::AgentConnectionPlanner;
use hiroute_application::control::{AgentConnectionControlPort, ControlReadError};
use hiroute_application_api::{
    AgentConnectSpecV1, AgentConnectionStateV1, AgentConnectionStatusRequestV1,
};
use hiroute_domain::{
    AgentActivationModeV1, AgentConnectionEffectRoleV1, AgentConnectionTransactionSubjectV1,
    BeginOperationOutcome, CanonicalDigest, ControlRepositoryPort, IdempotencyScopeV1, OperationId,
    OperationState, OperationV1, ProtectedApplyCapability, PublicationRepositoryPort,
    SecretStorePort, WorkspaceId,
};
use hiroute_local_storage::{ApplyCapabilityRegistrationV1, LocalStorageSet, ManagedArtifactStore};

use super::LocalControlAdapter;

pub(in super::super) fn open_endpoint_test_artifacts(
    stores: &LocalStorageSet,
    scanner: &hiroute_integrations::FilesystemAgentScannerV1,
    storage: &std::path::Path,
) -> ManagedArtifactStore {
    let native = scanner.claude_user_settings_target();
    hiroute_platform::durable::write_private_atomic(&native, b"{}\n").unwrap();
    #[cfg(windows)]
    hiroute_platform::windows::security::verify_owner_only(&native, false).unwrap();
    let subject = AgentConnectionTransactionSubjectV1::from_registered_profile(
        "agent_claude_default",
        "claude-messages-v1",
        "builtin/claude-messages/v1",
    )
    .unwrap();
    stores
        .open_managed_artifacts_with_external_targets(
            storage.join("artifacts"),
            storage.join("restores"),
            [(
                AgentConnectionEffectRoleV1::ManagedConfiguration
                    .target_for(&subject)
                    .unwrap(),
                native,
            )],
        )
        .unwrap()
}

// Reuse the isolated durable-store fixture and the current V1 planner. This proves the legacy
// endpoint boundary, not native authentication or end-to-end readiness. Ordinary V1 discovery
// still treats the local Gateway as an unregistered upstream; do not broaden raw-grant admission.
pub(in super::super) fn assert_persisted_endpoint_convergence(
    adapter: &LocalControlAdapter,
    spec: &AgentConnectSpecV1,
) {
    let mut facts = adapter.planning_facts(spec).unwrap().facts;
    for iteration in 0..2 {
        facts.activation_mode = AgentActivationModeV1::ManagedConfiguration;
        for proof in &mut facts.installation.capability_evidence {
            if hiroute_domain::AgentAction::ConfigureModel
                .requires()
                .contains(&proof.capability)
            {
                proof.state = hiroute_domain::CapabilityState::Proven;
                proof.reason = None;
            }
        }
        let preview = AgentConnectionPlanner::preview_connect(spec.clone(), facts.clone()).unwrap();
        assert_eq!(
            preview
                .config_change
                .fields
                .iter()
                .any(|field| field.path == "env.ANTHROPIC_BASE_URL"),
            iteration == 0,
            "the second apply must exercise an endpoint omitted from the sealed delta"
        );
        let plan = AgentConnectionPlanner::seal_apply(
            &preview,
            &preview.change_digest,
            &preview.expected_revisions,
            &preview.expected_revisions,
        )
        .unwrap();
        let workspace = WorkspaceId::default();
        let scope = IdempotencyScopeV1::new(
            "interactive-user",
            "ApplyAgentConnectionChange",
            format!("persisted-endpoint-{iteration}"),
        )
        .unwrap();
        let request_digest = CanonicalDigest::of_bytes(scope.key.as_bytes());
        let operation = OperationV1::new(
            OperationId::derive(&workspace, &scope, &request_digest),
            workspace.clone(),
            scope,
            request_digest,
            preview.change_digest.clone(),
            preview.expected_revisions.clone(),
            plan,
        )
        .unwrap();
        let capability = format!("persisted-endpoint-capability-0123456789abcdef-{iteration}");
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 60;
        {
            let stores = adapter.stores.lock().unwrap();
            stores
                .apply_capability_registrar()
                .register(
                    ApplyCapabilityRegistrationV1::from_protected_launcher(
                        capability.clone(),
                        operation.idempotency.principal.clone(),
                        workspace.clone(),
                        operation.idempotency.operation_kind.clone(),
                        operation.accepted_digest.clone(),
                        operation.expected_revisions.clone(),
                        expires,
                    )
                    .unwrap(),
                )
                .unwrap();
            let authorization = stores
                .control()
                .verify_apply_authorization(
                    &ProtectedApplyCapability::new(capability).unwrap(),
                    &workspace,
                    &operation.idempotency.principal,
                    &operation.idempotency.operation_kind,
                    &operation.accepted_digest,
                    &operation.expected_revisions,
                )
                .unwrap();
            assert_eq!(
                stores
                    .control()
                    .begin_operation(&operation, &authorization)
                    .unwrap(),
                BeginOperationOutcome::Created
            );
        }
        let completed = TransactionCoordinator::new(
            adapter,
            adapter,
            adapter,
            adapter,
            adapter,
            &adapter.admission,
        )
        .run(&operation.operation_id)
        .unwrap();
        assert_eq!(completed.state, OperationState::Succeeded, "{completed:#?}");
        let request = AgentConnectionStatusRequestV1 {
            connection_id: preview.change_spec.resource_id.clone().unwrap(),
        };
        let (connection, succeeded) = adapter
            .latest_connection_operation(&request.connection_id)
            .unwrap();
        let (mut installation, _) = adapter
            .scanner
            .claude_installation_for_applied_user_change(&preview.config_change)
            .unwrap();
        assert!(
            adapter
                .verify_connection_endpoint(&connection, &succeeded, &installation)
                .is_ok()
        );
        // The existing public V1 status remains conservative for an unregistered local upstream.
        assert_eq!(
            adapter.connection_status(&request).unwrap().state,
            AgentConnectionStateV1::Disconnected
        );
        adapter
            .managed_agent_runtime
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .gateway_base_url = "http://127.0.0.1:5838/v1".to_owned();
        let stale = adapter.connection_status(&request).unwrap();
        assert_eq!(stale.state, AgentConnectionStateV1::Disconnected);
        assert!(!stale.launch_ready);
        assert!(matches!(
            adapter.verify_connection_endpoint(&connection, &succeeded, &installation),
            Err(ControlReadError::Denied)
        ));
        adapter
            .managed_agent_runtime
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .gateway_base_url = "http://127.0.0.1:5837/v1".to_owned();
        assert!(
            adapter
                .verify_connection_endpoint(&connection, &succeeded, &installation)
                .is_ok()
        );
        // Refresh component facts from the real applied file and durable publication. Public
        // V1 re-planning is outside this fixture because it requires ordinary upstream discovery.
        installation.version = spec.installed_version.clone();
        facts.installation = installation;
        facts.current_config = adapter.scanner.claude_user_config_document().unwrap();
        let subject = AgentConnectionTransactionSubjectV1::from_registered_profile(
            &spec.agent_id,
            &spec.profile_id,
            "builtin/claude-messages/v1",
        )
        .unwrap();
        let stores = adapter.stores.lock().unwrap();
        facts.expected_revisions = stores.control().current_revisions(&workspace).unwrap();
        facts.publication_digest = stores
            .control()
            .active_publication(&workspace)
            .unwrap()
            .unwrap()
            .digest;
        facts.expected_grant_generation = stores
            .secrets()
            .inspect_agent_access_grant(WorkspaceId::DEFAULT, &request.connection_id)
            .unwrap()
            .unwrap()
            .generation();
        facts.before_fingerprints = adapter
            .before_fingerprints(&subject, facts.publication_digest.clone())
            .unwrap();
    }
}
