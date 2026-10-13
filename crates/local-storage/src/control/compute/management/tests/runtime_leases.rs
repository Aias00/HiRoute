use super::*;
use hiroute_domain::{
    ComputeManagementRepositoryPort, ComputeManagementSourceV2, ConnectorRuntimeKind,
    GatewayOperationalTargetV1, NATIVE_CREDENTIAL_LEASE_REQUEST_SCHEMA_V1,
    NativeCredentialAccessDeniedV1, NativeCredentialLeaseErrorV1, NativeCredentialLeaseRequestV1,
};

fn save(
    stores: &LocalStorageSet,
    registry: &TrustedComputeCandidateRegistry,
    subject: ComputeManagementSubjectV2,
    intent: ComputeManagementIntentV2,
    key_edits: Vec<ComputeKeyEditV2>,
    key: &str,
) -> ComputeManagementSourceV2 {
    let workspace = WorkspaceId::default();
    let snapshot = stores
        .control()
        .compute_management_snapshot(&workspace)
        .unwrap();
    let input = ProtectedInput;
    let planner =
        ComputeManagementPlanner::new(registry, stores.control(), stores.secrets(), &input);
    let preview = planner
        .preview(ComputeManagementChangeV2 {
            edit: None,
            schema: "hiroute.compute-management-change/v2".into(),
            subject,
            expected_revisions: snapshot.revisions,
            selected_model_refs: vec!["model/one".into(), "model/two".into()],
            intent,
            key_edits,
            validation: None,
        })
        .unwrap();
    let runtime = TransactionRuntime::default();
    let external = NoExternal;
    let coordinator = TransactionCoordinator::new(
        stores.control(),
        stores.secrets(),
        stores.runtime(),
        &external,
        &input,
        &runtime,
    );
    coordinator.reconcile_startup_and_open().unwrap();
    apply_preview(stores, &planner, &coordinator, &workspace, preview, key);
    stores
        .control()
        .compute_management_snapshot(&workspace)
        .unwrap()
        .sources
        .remove(0)
}

pub(super) fn frozen_request(
    source: &ComputeManagementSourceV2,
    model: usize,
    credential: usize,
) -> NativeCredentialLeaseRequestV1 {
    let target = GatewayOperationalTargetV1::UserConfiguredNative {
        uri: "https://api.example.test/v1/responses".into(),
    };
    NativeCredentialLeaseRequestV1 {
        schema_version: NATIVE_CREDENTIAL_LEASE_REQUEST_SCHEMA_V1.into(),
        stable_binding_id: source.models[model].binding_id.clone(),
        credential_id: source.credentials[credential].key_id.clone(),
        credential_destination_ref: source.target.credential_destination().unwrap(),
        excluded_key_ids: Vec::new(),
        connector_runtime: ConnectorRuntimeKind::BuiltinNative,
        connector_id: "builtin-openai".into(),
        upstream_protocol: UpstreamProtocol::Responses,
        upstream_model_id: source.models[model].upstream_model_id.clone(),
        native_transport_model: source.models[model].upstream_model_id.clone(),
        logical_endpoint: target.uri().into(),
        operational_target_digest: CanonicalDigest::of(&target).unwrap(),
        operational_target: target,
        request_path: "/v1/responses".into(),
        runtime_epoch: None,
        target_epoch: None,
        protocol_profile_digest: CanonicalDigest::of_bytes(b"frozen-managed-profile"),
        authentication: source.authentication.clone(),
    }
}

#[test]
fn frozen_native_requests_obey_current_source_and_key_switches_after_restart() {
    let root = tempdir().unwrap();
    let registry = TrustedComputeCandidateRegistry::new();
    let primary = candidate("candidate/lease-primary", "slot/primary");
    let secondary = candidate("candidate/lease-secondary", "slot/secondary");
    registry
        .register_compute_candidate(primary.clone())
        .unwrap();
    registry
        .register_compute_candidate(secondary.clone())
        .unwrap();
    let stores = LocalStorageSet::open_for_daemon_startup(root.path()).unwrap();
    let source = save(
        &stores,
        &registry,
        ComputeManagementSubjectV2::Candidate {
            candidate: primary.candidate,
        },
        ComputeManagementIntentV2::SaveReady,
        Vec::new(),
        "lease-create",
    );
    let subject = ComputeManagementSubjectV2::SavedSource {
        source_id: source.source_id.clone(),
    };
    let source = save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveReady,
        vec![ComputeKeyEditV2::Add {
            input_candidate: secondary.candidate,
        }],
        "lease-add-key",
    );
    let requests = [
        frozen_request(&source, 0, 0),
        frozen_request(&source, 1, 0),
        frozen_request(&source, 0, 1),
    ];
    for request in &requests {
        assert!(
            stores
                .lease_native_credential_exact(request)
                .unwrap()
                .is_some()
        );
    }
    let edit = |index: usize, enabled| ComputeKeyEditV2::SetEnabled {
        key_id: source.credentials[index].key_id.clone(),
        expected_generation: 1,
        enabled,
    };
    save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveReady,
        vec![edit(0, false)],
        "lease-disable-first-key",
    );
    for request in &requests[..2] {
        assert!(
            stores.lease_native_credential_exact(request).err()
                == Some(NativeCredentialLeaseErrorV1::Denied(
                    NativeCredentialAccessDeniedV1::CredentialDisabled
                )),
            "the same disabled key must be unavailable to both frozen model bindings"
        );
        assert!(
            stores
                .secrets()
                .lease_native_credential_exact(request)
                .unwrap()
                .is_some(),
            "the old Secret-only production path still leases it, proving this regression"
        );
    }
    assert!(
        stores
            .lease_native_credential_exact(&requests[2])
            .unwrap()
            .is_some()
    );
    save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveDisabled,
        Vec::new(),
        "lease-disable-source",
    );
    for request in &requests {
        assert!(
            stores.lease_native_credential_exact(request).err()
                == Some(NativeCredentialLeaseErrorV1::Denied(
                    NativeCredentialAccessDeniedV1::SourceDisabled
                ))
        );
    }
    drop(stores);
    let stores = LocalStorageSet::open_for_daemon_startup(root.path()).unwrap();
    for request in &requests {
        assert!(
            stores.lease_native_credential_exact(request).err()
                == Some(NativeCredentialLeaseErrorV1::Denied(
                    NativeCredentialAccessDeniedV1::SourceDisabled
                ))
        );
    }
    save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveReady,
        Vec::new(),
        "lease-reenable-source",
    );
    assert!(
        stores.lease_native_credential_exact(&requests[0]).err()
            == Some(NativeCredentialLeaseErrorV1::Denied(
                NativeCredentialAccessDeniedV1::CredentialDisabled
            ))
    );
    assert!(
        stores
            .lease_native_credential_exact(&requests[2])
            .unwrap()
            .is_some()
    );
    save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveDisabled,
        vec![edit(1, false)],
        "lease-disable-last-key",
    );
    for request in &requests {
        assert!(
            stores.lease_native_credential_exact(request).err()
                == Some(NativeCredentialLeaseErrorV1::Denied(
                    NativeCredentialAccessDeniedV1::SourceDisabled
                ))
        );
    }
    save(
        &stores,
        &registry,
        subject,
        ComputeManagementIntentV2::SaveReady,
        vec![edit(0, true), edit(1, true)],
        "lease-reenable-keys",
    );
    for request in &requests {
        let lease = stores
            .lease_native_credential_exact(request)
            .unwrap()
            .unwrap();
        assert_eq!(lease.generation(), 1);
        assert_eq!(lease.credential_id(), request.credential_id);
    }
}

#[test]
fn current_pool_uses_saved_order_without_changing_frozen_request_or_lease_identity() {
    let root = tempdir().unwrap();
    let registry = TrustedComputeCandidateRegistry::new();
    let primary = candidate("candidate/pool-primary", "slot/primary");
    let secondary = candidate("candidate/pool-secondary", "slot/secondary");
    let replacement = candidate("candidate/pool-replacement", "slot/replacement");
    for facts in [&primary, &secondary, &replacement] {
        registry.register_compute_candidate(facts.clone()).unwrap();
    }
    let stores = LocalStorageSet::open_for_daemon_startup(root.path()).unwrap();
    let source = save(
        &stores,
        &registry,
        ComputeManagementSubjectV2::Candidate {
            candidate: primary.candidate,
        },
        ComputeManagementIntentV2::SaveReady,
        vec![],
        "pool-create",
    );
    let request = frozen_request(&source, 0, 0);
    let first = source.credentials[0].key_id.clone();
    let subject = ComputeManagementSubjectV2::SavedSource {
        source_id: source.source_id.clone(),
    };
    let second =
        hiroute_domain::managed_key_id(&source.source_id, &secondary.candidate.candidate_ref)
            .unwrap();
    save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveReady,
        vec![
            ComputeKeyEditV2::Add {
                input_candidate: secondary.candidate,
            },
            ComputeKeyEditV2::SetOrder {
                key_ids: vec![second.clone(), first.clone()],
            },
        ],
        "pool-add-and-order",
    );
    let bound_members = stores
        .current_native_credential_refs(&request)
        .unwrap()
        .unwrap();
    assert_eq!(bound_members, [second.clone(), first.clone()]);
    assert_eq!(request.credential_id, first);
    assert_eq!(
        stores
            .lease_native_credential_exact(&request)
            .unwrap()
            .unwrap()
            .credential_id(),
        first
    );

    save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveReady,
        vec![
            ComputeKeyEditV2::Replace {
                key_id: second.clone(),
                expected_generation: 1,
                input_candidate: replacement.candidate,
            },
            ComputeKeyEditV2::SetOrder {
                key_ids: vec![first.clone(), second.clone()],
            },
        ],
        "pool-replace-and-order",
    );
    assert_eq!(
        stores
            .current_native_credential_refs(&request)
            .unwrap()
            .unwrap(),
        [first.clone(), second.clone()]
    );
    assert_eq!(
        bound_members,
        [second.clone(), first.clone()],
        "an admitted request's owned list stays fixed"
    );
    let mut bound_second = request.clone();
    bound_second.credential_id = second.clone();
    let lease = stores
        .lease_native_credential_exact(&bound_second)
        .unwrap()
        .unwrap();
    assert_eq!(
        lease.generation(),
        2,
        "a later exact lease sees the current generation of the same key"
    );
    assert_eq!(lease.credential_id(), second);

    save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveReady,
        vec![ComputeKeyEditV2::SetEnabled {
            key_id: second.clone(),
            expected_generation: 2,
            enabled: false,
        }],
        "pool-disable-second",
    );
    assert_eq!(
        stores
            .current_native_credential_refs(&request)
            .unwrap()
            .unwrap(),
        std::slice::from_ref(&first)
    );
    assert!(
        stores
            .lease_native_credential_exact(&bound_second)
            .unwrap()
            .is_none()
    );
    save(
        &stores,
        &registry,
        subject.clone(),
        ComputeManagementIntentV2::SaveDisabled,
        vec![],
        "pool-disable-source",
    );
    assert_eq!(
        stores.current_native_credential_refs(&request),
        Err(NativeCredentialLeaseErrorV1::Denied(
            NativeCredentialAccessDeniedV1::SourceDisabled
        ))
    );
    drop(stores);
    let stores = LocalStorageSet::open_for_daemon_startup(root.path()).unwrap();
    assert_eq!(
        stores.current_native_credential_refs(&request),
        Err(NativeCredentialLeaseErrorV1::Denied(
            NativeCredentialAccessDeniedV1::SourceDisabled
        ))
    );
    save(
        &stores,
        &registry,
        subject,
        ComputeManagementIntentV2::SaveReady,
        vec![],
        "pool-restore-source",
    );
    assert_eq!(
        stores
            .current_native_credential_refs(&request)
            .unwrap()
            .unwrap(),
        [first]
    );
}

#[test]
fn current_pool_rejects_frozen_requests_for_other_models_targets_or_authentication() {
    let root = tempdir().unwrap();
    let registry = TrustedComputeCandidateRegistry::new();
    let primary = candidate("candidate/pool-target", "slot/primary");
    registry
        .register_compute_candidate(primary.clone())
        .unwrap();
    let stores = LocalStorageSet::open_for_daemon_startup(root.path()).unwrap();
    let source = save(
        &stores,
        &registry,
        ComputeManagementSubjectV2::Candidate {
            candidate: primary.candidate,
        },
        ComputeManagementIntentV2::SaveReady,
        vec![],
        "pool-target-create",
    );
    let request = frozen_request(&source, 0, 0);
    let mut other_model = request.clone();
    other_model.upstream_model_id = "other-model".into();
    other_model.native_transport_model = "other-model".into();
    let mut other_destination = request.clone();
    other_destination.credential_destination_ref = "compute-target/other".into();
    let mut other_endpoint = request.clone();
    other_endpoint.logical_endpoint = "https://evil.example.test/v1/responses".into();
    other_endpoint.operational_target = GatewayOperationalTargetV1::UserConfiguredNative {
        uri: other_endpoint.logical_endpoint.clone(),
    };
    other_endpoint.operational_target_digest =
        CanonicalDigest::of(&other_endpoint.operational_target).unwrap();
    let mut other_auth = request.clone();
    other_auth.authentication = GatewayAuthenticationSemanticsV1::Bearer;
    for changed in [other_model, other_destination, other_endpoint, other_auth] {
        assert!(changed.validate().is_ok());
        assert!(stores.current_native_credential_refs(&changed).is_err());
    }
    let mut deleted_source = request;
    deleted_source.stable_binding_id = "binding/managed-deleted".into();
    assert_eq!(
        stores
            .current_native_credential_refs(&deleted_source)
            .unwrap(),
        Some(vec![])
    );
}
