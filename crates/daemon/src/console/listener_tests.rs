//! Live role-all composition, including the absence of side effects from launch minting.
use super::{
    ConsoleService,
    tests::{http, request},
};
use hiroute_application_api::ErrorCode;
use hiroute_host_runtime::{
    ConsoleListenerDesiredV1, ConsoleListenerMode, ConsoleListenerStore, ConsolePortMode,
    StandaloneLayout,
};
use serde_json::json;
use std::net::TcpListener;

#[test]
fn launch_uses_only_startup_listeners_and_preserves_pending_configuration() {
    if crate::test_support::isolated_agent_home(
        "console::listener_tests::launch_uses_only_startup_listeners_and_preserves_pending_configuration",
    ) {
        return;
    }
    if super::tests::installed_test_layout(
        "console::listener_tests::launch_uses_only_startup_listeners_and_preserves_pending_configuration",
    ) {
        return;
    }
    for mode in ["off", "occupied", "loopback", "network"] {
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
        let occupied = held.local_addr().unwrap();
        let store = ConsoleListenerStore::new(layout.gateway_config_root());
        let desired = match mode {
            "off" => ConsoleListenerDesiredV1::default(),
            "occupied" => ConsoleListenerDesiredV1 {
                port_mode: ConsolePortMode::Explicit,
                port: Some(occupied.port()),
                ..ConsoleListenerDesiredV1::loopback()
            },
            "network" => ConsoleListenerDesiredV1 {
                mode: ConsoleListenerMode::Network,
                address: Some("0.0.0.0".parse().unwrap()),
                advertise_address: Some("192.0.2.10".parse().unwrap()),
                allow_plaintext_network: true,
                ..Default::default()
            },
            _ => ConsoleListenerDesiredV1::loopback(),
        };
        store.configure(desired).unwrap();
        let gateway = TcpListener::bind("127.0.0.1:0").unwrap();
        let gateway_address = gateway.local_addr().unwrap();
        drop(gateway);
        let role = crate::start_role_all(
            crate::RoleAllConfig::new(
                layout.storage_root(),
                layout.runtime_root.clone(),
                gateway_address,
                layout.gateway_lkg(),
            )
            .with_released_commands_only(),
        )
        .unwrap();
        let mut console = ConsoleService::start(
            &role,
            layout.clone(),
            directory.path().join("missing-assets"),
            None,
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let client = hiroute_client_core::Client::new(
            "cli",
            hiroute_client_core::LocalEndpoint::from_runtime_root(&layout.runtime_root),
        );
        let mint = |payload| {
            runtime
                .block_on(client.call_wire(request("CreateConsoleLaunch", payload)))
                .unwrap()
        };
        if matches!(mode, "off" | "occupied") {
            let before = std::fs::read(store.path()).unwrap();
            for payload in [json!({}), json!({"loopback_only":true}), json!({})] {
                assert_eq!(
                    mint(payload).error.unwrap().code,
                    ErrorCode::ActionRequired,
                    "{mode}"
                );
            }
            assert_eq!(std::fs::read(store.path()).unwrap(), before);
            assert!(store.read().unwrap().applied_address.is_none());
            store
                .configure(ConsoleListenerDesiredV1::loopback())
                .unwrap();
            assert_eq!(
                mint(json!({})).error.unwrap().code,
                ErrorCode::ActionRequired,
                "pending configuration cannot start a listener"
            );
        } else {
            let url = mint(json!({})).data.unwrap()["url"]
                .as_str()
                .unwrap()
                .to_owned();
            let applied = store.read().unwrap().applied_address.unwrap();
            let expected = if mode == "network" {
                format!("http://192.0.2.10:{}", applied.port())
            } else {
                format!("http://{applied}")
            };
            assert!(
                url.starts_with(&format!("{expected}/launch?code=")),
                "{url}"
            );
            let local = mint(json!({"loopback_only":true})).data.unwrap()["url"]
                .as_str()
                .unwrap()
                .to_owned();
            let (address, code) = local
                .strip_prefix("http://")
                .unwrap()
                .split_once("/launch?code=")
                .unwrap();
            let address = address.parse().unwrap();
            assert_eq!(
                http(
                    address,
                    "/api/console/v1/session/launch",
                    Some(json!({"code":code})),
                    None,
                    ""
                )
                .0,
                200
            );
            // Saving a new desired scope does not change the active listener or its port.
            store
                .configure(ConsoleListenerDesiredV1::default())
                .unwrap();
            let still_live = mint(json!({})).data.unwrap()["url"]
                .as_str()
                .unwrap()
                .to_owned();
            assert!(still_live.starts_with(&format!("{expected}/launch?code=")));
        }
        console.shutdown();
        role.shutdown();
    }
}
