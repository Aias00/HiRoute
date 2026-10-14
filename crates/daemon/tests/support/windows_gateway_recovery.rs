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
    exercise_with(
        &mut |arguments| {
            let mut args = arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>();
            if !arguments.contains(&"--output") {
                args.extend([
                    "--output".into(),
                    "json".into(),
                    "--timeout".into(),
                    "45s".into(),
                ]);
            }
            let result = fixture::command(token, cli, args);
            (result["code"].as_i64().unwrap(), result["result"].clone())
        },
        &mut || fixture::inspect(token, false),
        previous,
        sid,
        session,
    );
}

/// Standard-owner callers retain their actual token and use normal process
/// creation. The existing LUA/SCM wrapper supplies the same real command oracle.
pub fn exercise_with(
    call: &mut dyn FnMut(&[&str]) -> (i64, Value),
    inspect: &mut dyn FnMut() -> Value,
    previous: &Value,
    sid: &str,
    session: u32,
) -> Value {
    fn data(call: &mut dyn FnMut(&[&str]) -> (i64, Value), arguments: &[&str]) -> Value {
        let (exit, envelope) = call(arguments);
        assert!(
            exit == 0 && envelope["status"] == "succeeded" && envelope["data"].is_object(),
            "public Gateway recovery command failed"
        );
        envelope["data"].clone()
    }
    let shown = data(call, &["gateway", "show"]);
    assert_eq!(shown["ready"], true);
    let original: GatewayListenerConfigV1 =
        serde_json::from_value(shown["config"].clone()).unwrap();
    original.validate().unwrap();
    let applied = original.applied_address().unwrap().unwrap();
    assert!(applied.ip().is_loopback());
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let blocked = occupied.local_addr().unwrap();
    assert_ne!(blocked.port(), applied.port());

    let (exit, rejected) = call(&[
        "gateway",
        "set",
        "--address",
        "127.0.0.1",
        "--port",
        &blocked.port().to_string(),
        "--timeout",
        "15s",
        "--output",
        "json",
    ]);
    assert_ne!(
        exit, 0,
        "occupied fixed desired port must reject actual restart"
    );
    let error = serde_json::from_value::<hiroute_application_api::ErrorCode>(
        rejected["error"]["code"].clone(),
    )
    .ok();
    assert_eq!(
        error,
        Some(hiroute_application_api::ErrorCode::DaemonUnavailable)
    );
    let failed = data(call, &["gateway", "show"]);
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

    let recovered = data(call, &["gateway", "recover"]);
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
    let actual = inspect();
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
    actual
}
