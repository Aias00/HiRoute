//! Real role-all / Local Control / HTTP composition; installed acceptance is a separate gate.
use super::*;
use hiroute_application_api::*;
use hiroute_client_core::{Client, LocalEndpoint};
use hiroute_host_runtime::StandaloneLayout;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    time::Duration,
};
fn request(operation: &str, payload: Value) -> LocalControlWireRequestV2 {
    LocalControlWireRequestV2 {
        schema_version: LOCAL_CONTROL_SCHEMA_V2,
        request_id: format!("console-{operation}"),
        operation_id: operation.into(),
        payload,
        protected_grant: None,
    }
}
fn http(
    address: SocketAddr,
    path: &str,
    body: Option<Value>,
    bearer: Option<&str>,
    extra: &str,
) -> (u16, Value, String) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = body.map(|v| v.to_string()).unwrap_or_default();
    let authorization = bearer
        .map(|b| format!("Authorization: Bearer {b}\r\n"))
        .unwrap_or_default();
    write!(stream,"{} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nX-HiRoute-Context: 1234567890abcdef\r\n{authorization}{extra}Content-Length: {}\r\n\r\n{body}",if body.is_empty(){"GET"}else{"POST"},body.len()).unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let (status, content) = raw.split_once("\r\n\r\n").unwrap();
    let code = status.split_whitespace().nth(1).unwrap().parse().unwrap();
    (
        code,
        serde_json::from_str(content).unwrap_or(Value::Null),
        status.into(),
    )
}
#[test]
fn real_role_all_keeps_external_gate_and_console_auth_authority_separate() {
    #[cfg(unix)]
    if crate::test_support::isolated_agent_home(
        "console::tests::real_role_all_keeps_external_gate_and_console_auth_authority_separate",
    ) {
        return;
    }
    let directory = crate::test_support::private_tempdir();
    let layout = StandaloneLayout::from_values(
        false,
        Some(directory.path().to_path_buf()),
        None,
        None,
        None,
    )
    .unwrap();
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let gateway = held.local_addr().unwrap();
    drop(held);
    let mut role = crate::start_role_all(
        crate::RoleAllConfig::new(
            layout.storage_root(),
            layout.runtime_root.clone(),
            gateway,
            layout.gateway_lkg(),
        )
        .with_released_commands_only(),
    )
    .unwrap();
    let assets = directory.path().join("assets");
    std::fs::create_dir(&assets).unwrap();
    std::fs::write(
        assets.join("index.html"),
        b"<!doctype html><title>HiRoute</title>",
    )
    .unwrap();
    let mut console = ConsoleService::start(&role, layout.clone(), assets, None).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let external = Client::new(
        "cli",
        LocalEndpoint::from_runtime_root(&layout.runtime_root),
    );
    let denied = runtime
        .block_on(external.call_wire(request("GetClientServiceStatus", json!({}))))
        .unwrap();
    assert_eq!(denied.error.unwrap().code, ErrorCode::UnknownCommand);
    let internal = runtime
        .block_on(
            role.console_client()
                .call_wire(request("GetClientServiceStatus", json!({}))),
        )
        .unwrap();
    assert!(internal.error.is_none(), "{:?}", internal.error);
    let mut agent = request("CreateConsoleLaunch", json!({}));
    agent.protected_grant = Some(ProtectedClientGrantV2 {
        principal_kind: PrincipalKind::SealedCollaboration,
        capability: "not-an-owner".into(),
    });
    assert_eq!(
        runtime
            .block_on(external.call_wire(agent))
            .unwrap()
            .error
            .unwrap()
            .code,
        ErrorCode::CapabilityDenied
    );
    let launch = runtime
        .block_on(external.call_wire(request("CreateConsoleLaunch", json!({}))))
        .unwrap()
        .data
        .unwrap();
    let url = launch["url"].as_str().unwrap();
    let (address, code) = url
        .strip_prefix("http://")
        .unwrap()
        .split_once("/launch?code=")
        .unwrap();
    let address: SocketAddr = address.parse().unwrap();
    assert_eq!(
        http(
            address,
            "/api/console/v1/session/launch",
            Some(json!({"code":code})),
            None,
            "Origin: http://evil.example\r\n"
        )
        .0,
        403
    );
    let (status, exchange, headers) = http(
        address,
        "/api/console/v1/session/launch",
        Some(json!({"code":code})),
        None,
        "",
    );
    assert_eq!(status, 200);
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-security-policy:")
    );
    assert!(!headers.to_ascii_lowercase().contains("set-cookie"));
    let bearer = exchange["bearer"].as_str().unwrap();
    assert_eq!(
        http(
            address,
            "/api/console/v1/session/launch",
            Some(json!({"code":code})),
            None,
            ""
        )
        .0,
        401
    );
    assert_eq!(
        http(address, "/api/console/v1/session", None, None, "").0,
        401
    );
    assert_eq!(
        http(
            address,
            "/api/console/v1/command/quit_desktop",
            Some(json!({})),
            Some(bearer),
            ""
        )
        .0,
        404
    );
    let (status, snapshot, _) = http(
        address,
        "/api/console/v1/command/desktop_snapshot",
        Some(json!({})),
        Some(bearer),
        "",
    );
    assert_eq!(status, 200);
    assert_eq!(snapshot["trusted_authority"], true);
    let secret = "console-fixture-secret-must-not-be-logged";
    let (status, input, _) = http(
        address,
        "/api/console/v1/command/register_protected_model_input",
        Some(json!({"secret":secret,"sourceId":null})),
        Some(bearer),
        "",
    );
    assert_eq!(status, 200, "{input}");
    assert!(!input.to_string().contains(secret));
    role.upgrade_status(hiroute_host_runtime::UpgradeAction::Prepare, "test".into())
        .unwrap();
    assert!(
        runtime
            .block_on(
                role.console_client()
                    .call_wire(request("GetClientServiceStatus", json!({})))
            )
            .unwrap()
            .error
            .is_some()
    );
    role.upgrade_status(hiroute_host_runtime::UpgradeAction::Cancel, "test".into())
        .unwrap();
    let audit = std::fs::read_to_string(layout.data_root.join("console/audit.jsonl")).unwrap();
    assert!(audit.contains("register_protected_model_input"));
    assert!(!audit.contains(secret));
    assert!(!audit.contains(bearer));
    assert!(!audit.contains(code));
    console.shutdown();
    role.shutdown();
    role.join(Duration::from_secs(30)).unwrap();
}
