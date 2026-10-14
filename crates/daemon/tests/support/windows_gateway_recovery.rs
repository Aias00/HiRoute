//! Actual public listener recovery on an installed native manager. The failed
//! desired port remains occupied throughout recovery, so an automatic fallback
//! or an accidental retry of desired B cannot satisfy the assertion.
use super::fixture;
use hiroute_host_runtime::{GatewayListenerConfigV1, GatewayListenerOperationStateV1};
use serde_json::Value;
use std::fs::File;
use std::net::TcpListener;
use std::path::Path;

pub fn exercise(token: &File, cli: &Path, previous: &Value, sid: &str, session: u32) {
    let shown = fixture::cli(token, cli, &["gateway", "show"]);
    assert_eq!(shown["ready"], true);
    let original: GatewayListenerConfigV1 =
        serde_json::from_value(shown["config"].clone()).unwrap();
    original.validate().unwrap();
    let applied = original.applied_address().unwrap().unwrap();
    assert!(applied.ip().is_loopback());
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let blocked = occupied.local_addr().unwrap();
    assert_ne!(blocked.port(), applied.port());

    let rejected = fixture::command(
        token,
        cli,
        vec![
            "gateway".into(),
            "set".into(),
            "--address".into(),
            "127.0.0.1".into(),
            "--port".into(),
            blocked.port().to_string(),
            "--timeout".into(),
            "15s".into(),
            "--output".into(),
            "json".into(),
        ],
    );
    assert_ne!(
        rejected["code"], 0,
        "occupied fixed desired port must reject actual restart"
    );
    assert_eq!(rejected["result"]["error"]["code"], "DAEMON_UNAVAILABLE");
    let failed = fixture::cli(token, cli, &["gateway", "show"]);
    assert_eq!(
        failed["ready"], false,
        "failed restart must not advertise the old listener as ready"
    );
    let config: GatewayListenerConfigV1 = serde_json::from_value(failed["config"].clone()).unwrap();
    config.validate().unwrap();
    assert_eq!(config.desired_address().unwrap(), Some(blocked));
    assert_eq!(config.applied_address().unwrap(), Some(applied));
    let operation = config.operation.unwrap();
    assert_eq!(operation.state, GatewayListenerOperationStateV1::Failed);
    assert_eq!(
        operation.error_code.as_deref(),
        Some("SERVICE_RESTART_FAILED")
    );

    let recovered = fixture::cli(token, cli, &["gateway", "recover"]);
    assert_eq!(recovered["ready"], true);
    let config: GatewayListenerConfigV1 =
        serde_json::from_value(recovered["config"].clone()).unwrap();
    config.validate().unwrap();
    assert_eq!(config.desired_address().unwrap(), Some(applied));
    assert_eq!(config.applied_address().unwrap(), Some(applied));
    let recovered_operation = config.operation.unwrap();
    assert_eq!(
        recovered_operation.state,
        GatewayListenerOperationStateV1::Succeeded
    );
    assert_ne!(recovered_operation.operation_id, operation.operation_id);
    assert!(recovered_operation.error_code.is_none());
    // This authenticates the held real daemon, medium owner token, control
    // response and Gateway HTTP ready/v2; Task EnginePID is never an oracle.
    let actual = fixture::inspect(token, false);
    assert_eq!(actual["sid"], sid);
    assert_eq!(actual["session"], session);
    assert!(
        actual["pid"] != previous["pid"] || actual["creation_time"] != previous["creation_time"]
    );
    assert_eq!(occupied.local_addr().unwrap(), blocked);
    drop(occupied);
    println!(
        "Windows Gateway public recovery: occupied desired B, retained applied A, real manager failure, exact A recovery and authenticated owner runtime verified"
    );
}
