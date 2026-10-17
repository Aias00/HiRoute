use super::*;
use hiroute_gateway::server::publication::token_sha256;

#[test]
fn standard_model_bearer_preserves_current_grants_and_legacy_carrier_precedence() {
    let upstream = NativeProvider::start(vec![
        ProviderReply::Complete {
            status: 200,
            error_kind: None,
            body: RESPONSES_OK,
        };
        3
    ]);
    let fixture = RuntimeFixture::launch_with_runtime_state_control(&[&upstream], 1);
    let body = br#"{"model":"runtime-model","input":"bearer admission"}"#;
    let bearer = format!("Bearer {TOKEN}");
    let send = |headers: &[(&str, &str)], body: &[u8]| {
        request(fixture.address, "POST", "/v1/responses", headers, body)
    };
    assert_eq!(send(&[("Authorization", &bearer)], body).status, 200);
    assert_eq!(
        request(
            fixture.address,
            "GET",
            "/v1/models",
            &[("Authorization", &bearer)],
            b""
        )
        .status,
        200
    );

    for secret in [
        "native-oauth",
        "provider-secret-1",
        "unissued-connection-token",
        "hr_run_control_fixture",
        "hr_run_model_unissued",
    ] {
        let invalid = format!("Bearer {secret}");
        let response = send(&[("Authorization", &invalid)], body);
        assert_eq!(response.status, 401, "{secret}");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["code"],
            "GATEWAY_GRANT_UNAUTHORIZED"
        );
    }
    for headers in [
        vec![
            ("Authorization", bearer.as_str()),
            ("Authorization", bearer.as_str()),
        ],
        vec![("Authorization", "Bearer first,second")],
        vec![("Authorization", "Basic runtime-token")],
        vec![("Authorization", bearer.as_str()), ("X-HiRoute-Token", "")],
        vec![
            ("Authorization", bearer.as_str()),
            ("X-HiRoute-Token", "unissued"),
        ],
    ] {
        assert_eq!(send(&headers, body).status, 401);
    }
    assert_eq!(
        request(
            fixture.address,
            "POST",
            "/v1/messages",
            &[("Authorization", &bearer)],
            br#"{"model":"runtime-model","messages":[]}"#,
        )
        .status,
        422,
        "a valid Responses grant cannot authorize Messages"
    );
    assert_eq!(
        send(
            &[("Authorization", &bearer)],
            br#"{"model":"not-in-grant","input":"denied"}"#
        )
        .status,
        404
    );
    assert_eq!(
        send(
            &[
                ("X-HiRoute-Token", TOKEN),
                ("Authorization", "Bearer native-oauth")
            ],
            body
        )
        .status,
        200,
        "the retained explicit carrier still overrides native SDK authorization"
    );
    assert_eq!(
        upstream.calls(),
        2,
        "denied requests never reach an upstream"
    );

    let mut publication = fixture.publication();
    publication
        .grants
        .iter_mut()
        .find(|grant| {
            grant.protocol == hiroute_gateway::server::request_plan::IngressProtocol::Responses
        })
        .unwrap()
        .bearer_token_sha256 = token_sha256("rotated-runtime-token");
    publication.publication_revision += 1;
    fixture.publish(&mut publication);
    assert_eq!(send(&[("Authorization", &bearer)], body).status, 401);
    assert_eq!(send(&[("X-HiRoute-Token", TOKEN)], body).status, 401);
    assert_eq!(
        send(&[("Authorization", "Bearer rotated-runtime-token")], body).status,
        200
    );
    assert_eq!(upstream.calls(), 3);
    for wire in upstream.requests() {
        assert_eq!(
            wire_header(&wire, "authorization").as_deref(),
            Some("Bearer provider-secret-1")
        );
        assert!(wire_header(&wire, "x-hiroute-token").is_none());
        let wire = String::from_utf8_lossy(&wire);
        assert!(!wire.contains("runtime-token") && !wire.contains("native-oauth"));
    }
}
