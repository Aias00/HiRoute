//! Actual public authorization rejection boundaries, without an account or a
//! fabricated subscription validation. Both installed callers use the same
//! public journal producer; all dynamic process output remains with the caller.
use hiroute_application_api::ErrorCode;
use serde_json::{Value, json};
use std::net::TcpListener;

type PublicCall<'a> = dyn FnMut(&[&str], Option<&Value>) -> (i64, Value) + 'a;

fn accepted(call: &mut PublicCall<'_>, args: &[&str], request: &Value) -> Value {
    let (exit, envelope) = call(args, Some(request));
    assert!(
        exit == 0
            && matches!(envelope["status"].as_str(), Some("succeeded" | "accepted"))
            && envelope["data"].is_object(),
        "public authorization boundary journal operation failed"
    );
    envelope
}

fn rejected(call: &mut PublicCall<'_>, request: &Value, expected: ErrorCode) {
    let (exit, envelope) = call(&["compute", "connection", "authorize"], Some(request));
    assert_ne!(
        exit, 0,
        "public authorization rejection must fail its process"
    );
    let actual = serde_json::from_value::<ErrorCode>(envelope["error"]["code"].clone()).ok();
    assert_eq!(actual, Some(expected));
}

pub fn exercise(call: &mut PublicCall<'_>) {
    let absent = json!({"operation_id":format!("op_{}", "0".repeat(32)),
        "state":"succeeded", "sequence":1, "cancellable":false});
    rejected(
        call,
        &json!({"action":"result", "operation":absent}),
        ErrorCode::ResourceNotFound,
    );
    rejected(
        call,
        &json!({"action":"release", "validation":{
            "approval_operation":absent,"validation_ref":"validation/foreground/missing","validation_revision":1}}),
        ErrorCode::ResourceNotFound,
    );
    // A real public operation establishes wrong-kind and stale-reference
    // failures without writing a database or manufacturing an account/check.
    let endpoint = TcpListener::bind("127.0.0.1:0").unwrap();
    endpoint.set_nonblocking(true).unwrap();
    let id = "windows-foreground-authorization-boundary";
    let service = json!({"id":id,"revision":1,"name":"Foreground authorization boundary",
        "connection":{"kind":"custom","endpoint":format!("http://{}/decision",endpoint.local_addr().unwrap()),
            "timeout_ms":1000,"auth_header":null}});
    let desired = json!({"id":id,"expected_revision":0,"service":service});
    let version = json!({"major":1,"minor":0});
    let spec = json!({"schema_version":version,"command_id":"decision.services.apply",
        "resource_id":id,"desired_state":desired});
    let preview = accepted(
        call,
        &["decision", "services", "apply"],
        &json!({"schema_version":version,"spec":spec}),
    )["data"]
        .clone();
    let request = json!({"schema_version":version,"spec":preview["normalized_spec"],
        "accept_digest":preview["change_digest"],"expected_revisions":preview["expected_revisions"],
        "idempotency_key":"windows-foreground-authorization-boundary"});
    let applied = accepted(call, &["decision", "services", "apply"], &request);
    let operation = applied["operation"].clone();
    assert!(operation["operation_id"].is_string());
    assert!(applied["data"]["state"] == "succeeded");
    rejected(
        call,
        &json!({"action":"result", "operation":operation}),
        ErrorCode::InvalidArguments,
    );
    let mut stale = operation.clone();
    stale["sequence"] = json!(
        operation["sequence"]
            .as_u64()
            .unwrap()
            .checked_add(1)
            .unwrap()
    );
    rejected(
        call,
        &json!({"action":"result", "operation":stale}),
        ErrorCode::RevisionConflict,
    );
    rejected(
        call,
        &json!({"action":"release", "validation":{
            "approval_operation":stale,"validation_ref":"validation/foreground/stale","validation_revision":1}}),
        ErrorCode::RevisionConflict,
    );
    match endpoint.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        _ => panic!("declaration-only custom decision generated an unapproved endpoint hop"),
    }
}
