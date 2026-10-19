//! Exercise the ambient principal actually reconstructed by Local Control.
use super::*;
use hiroute_application_api::{LOCAL_CONTROL_SCHEMA_V2, LocalControlWireRequestV2};
use serde_json::json;

#[test]
fn local_owner_capacity_roundtrip_reopens_and_does_not_accept_a_delegated_grant() {
    #[cfg(unix)]
    if crate::test_support::isolated_agent_home(
        "control::capacity_tests::local_owner_capacity_roundtrip_reopens_and_does_not_accept_a_delegated_grant",
    ) {
        return;
    }
    let dir = crate::test_support::private_tempdir();
    let open = || {
        runtime::ProductionControlRuntime::open_with_release_catalog(
            dir.path().join("storage"),
            crate::release_catalog::fixture_catalog(),
        )
        .unwrap()
    };
    let runtime = open();
    let call = |runtime: &runtime::ProductionControlRuntime, operation: &str, payload| {
        LocalControlDaemon::new(ApplicationService::new(runtime.application_ports())).dispatch_wire(
            LocalControlWireRequestV2 {
                schema_version: LOCAL_CONTROL_SCHEMA_V2,
                request_id: "capacity-owner".into(),
                operation_id: operation.into(),
                payload,
                protected_grant: None,
            },
        )
    };
    let saved = call(
        &runtime,
        "SetObservationCapacity",
        json!({"max_active_bytes":20_000_000_000u64}),
    );
    assert!(saved.error.is_none(), "{saved:?}");
    assert_eq!(saved.data.unwrap()["max_active_bytes"], 20_000_000_000u64);
    let denied = LocalControlDaemon::new(ApplicationService::new(runtime.application_ports()))
        .dispatch_wire(LocalControlWireRequestV2 {
            schema_version: LOCAL_CONTROL_SCHEMA_V2,
            request_id: "capacity-delegated".into(),
            operation_id: "SetObservationCapacity".into(),
            payload: json!({"max_active_bytes":null}),
            protected_grant: Some(hiroute_application_api::ProtectedClientGrantV2 {
                principal_kind: hiroute_application_api::PrincipalKind::Skill,
                capability: "read-only-grant".into(),
            }),
        });
    assert_eq!(
        denied.error.unwrap().code,
        hiroute_application_api::ErrorCode::CapabilityDenied
    );
    drop(runtime);
    let runtime = open();
    let read = call(&runtime, "GetObservationCapacity", json!({}));
    assert!(read.error.is_none(), "{read:?}");
    assert_eq!(read.data.unwrap()["max_active_bytes"], 20_000_000_000u64);
    let disabled = call(
        &runtime,
        "SetObservationCapacity",
        json!({"max_active_bytes":null}),
    );
    assert!(disabled.error.is_none(), "{disabled:?}");
    assert!(disabled.data.unwrap()["max_active_bytes"].is_null());
}
