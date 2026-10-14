//! Actual public route publication and Gateway requests against a loopback Native API.
//! Agent connections are configured through the public Agent entry by the caller.
use super::windows_business_http as http;
#[path = "windows_routing_observation.rs"]
mod observation;

use super::windows_headless_business::{self as business, Business, cli};
use super::{InstalledProduct, SECRET, assert_no_secret, track_secret};
use hiroute_platform::windows::job::{Child, Launch, Stream};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub const MODEL: &str = "windows-native-api-model";
pub struct Plan {
    pub plan_id: String,
    pub alias: String,
    pub editor: Value,
}
impl Plan {
    /// Also used by the real upstream Main/Worker journey, with its own source/oracle.
    pub fn create(product: &InstalledProduct, binding_id: &str, key: &str) -> Self {
        let editor = json!({
            "schema":"hiroute.plan-editor/v2", "display_name":"Windows public route", "purpose":"Actual Windows Gateway observation",
            "mode":"fixed_model", "candidates":[{"binding_id":binding_id}],
            "smart":{"economy":[],"primary":[],"judgment":judgment(),"reselect_on_user_message":false,
                "classifier":{"kind":"local_rules"},"complex_keywords":[]},
            "free":{"candidates":[],"primary":[],"primary_fallback":false},"delegation_enabled":false,"requirements":{},
            "limits":{"maximum_attempts":1,"request_timeout_ms":30000,"attempt_timeout_ms":30000}
        });
        Self::create_with_editor(product, editor, key)
    }
    pub fn create_with_editor(product: &InstalledProduct, editor: Value, key: &str) -> Self {
        let change = json!({"schema":"hiroute.plan-content-change/v2",
            "target":{"intent":"create","creation_key":key},"editor":editor,"consumed_draft":null});
        let preview = preview(product, &change);
        apply(product, &change, &preview, key);
        let plan_id = preview["plan_head"]["reference"]["plan_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let alias = preview["plan_head"]["model_alias"]
            .as_str()
            .unwrap()
            .to_owned();
        let listed = cli(product, &["routing", "list"], None);
        assert!(
            listed["data"]["plans"]
                .as_array()
                .unwrap()
                .iter()
                .any(|plan| plan["head"]["reference"]["plan_id"] == plan_id)
        );
        let shown = cli(product, &["routing", "show", &plan_id], None);
        assert_eq!(shown["data"]["head"]["model_alias"], alias);
        assert_eq!(shown["data"]["head"]["status"], "enabled");
        Self {
            plan_id,
            alias,
            editor,
        }
    }
    pub fn update_and_reject_stale(&mut self, product: &InstalledProduct) {
        let shown = cli(product, &["routing", "show", &self.plan_id], None);
        let revision = shown["data"]["head"]["head_revision"].clone();
        let change = |editor: Value| {
            json!({"schema":"hiroute.plan-content-change/v2",
            "target":{"intent":"update","plan_id":self.plan_id,"expected_head_revision":revision},"editor":editor,"consumed_draft":null})
        };
        let mut stale_editor = self.editor.clone();
        stale_editor["purpose"] = json!("Stale Windows edit");
        let stale = change(stale_editor);
        let stale_preview = preview(product, &stale);
        let mut editor = self.editor.clone();
        editor["purpose"] = json!("Published Windows route update");
        let current = change(editor.clone());
        let current_preview = preview(product, &current);
        apply(product, &current, &current_preview, "windows-route-update");
        let rejected = business::command(
            product,
            &["routing", "apply"],
            Some(&apply_request(
                &stale,
                &stale_preview,
                "windows-route-stale",
            )),
        );
        assert_ne!(rejected.0, 0);
        assert!(matches!(
            rejected.1["error"]["code"].as_str(),
            Some("CHANGE_PREVIEW_STALE" | "REVISION_CONFLICT")
        ));
        let shown = cli(product, &["routing", "show", &self.plan_id], None);
        assert_eq!(shown["data"]["head"]["model_alias"], self.alias);
        assert!(
            shown["data"]["head"]["head_revision"].as_u64().unwrap() > revision.as_u64().unwrap()
        );
        self.editor = editor;
    }
}
pub fn judgment() -> Value {
    json!({"degree":{"simple_threshold_millis":800,"instructions":"Assess task complexity.","simple":"Routine work","complex":"Uncertain work"},
        "competence":{"floor_millis":500,"instructions":"Assess task competence.","criteria":["Failed","Partly competent","Competent"]}})
}
fn preview(product: &InstalledProduct, change: &Value) -> Value {
    cli(
        product,
        &["routing", "preview"],
        Some(&json!({"change":change})),
    )["data"]
        .clone()
}
fn apply_request(change: &Value, preview: &Value, key: &str) -> Value {
    json!({"change":change,"accept_digest":preview["change_digest"],"expected_revisions":preview["expected_revisions"],"idempotency_key":key})
}
fn apply(product: &InstalledProduct, change: &Value, preview: &Value, key: &str) {
    let request = apply_request(change, preview, key);
    let applied = cli(product, &["routing", "apply"], Some(&request));
    let id = applied["operation"]["operation_id"].as_str().unwrap();
    business::assert_operation(product, id);
    let replay = cli(product, &["routing", "apply"], Some(&request));
    assert_eq!(replay["operation"]["operation_id"], id);
}

pub struct Routing {
    pub plan: Plan,
    _source: Business,
    upstream: http::Server,
    counts: Arc<Mutex<(usize, usize)>>,
}
impl Routing {
    pub fn setup(product: &InstalledProduct) -> Self {
        let counts = Arc::new(Mutex::new((0, 0)));
        let requests = counts.clone();
        let upstream = http::Server::start(move |request| {
            assert!(
                request.authenticated(&format!("Bearer {SECRET}")),
                "Gateway must read its saved Native credential"
            );
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/v1/responses");
            assert_eq!(request.body["model"], MODEL);
            // Initial production Compute check probes real tool use before source save.
            if request.body["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == "hiroute_probe"))
            {
                requests.lock().unwrap().0 += 1;
                return http::Reply::json(
                    200,
                    json!({"id":"windows-route-compute-check","object":"response","model":MODEL,"status":"completed",
                    "output":[{"type":"function_call","id":"check-call","call_id":"check-call","name":"hiroute_probe","arguments":"{\"value\":\"ok\"}"}],
                    "usage":{"input_tokens":2,"output_tokens":1,"total_tokens":3}}),
                );
            }
            requests.lock().unwrap().1 += 1;
            let body = serde_json::to_string(&request.body).unwrap();
            let managed = body.contains("windows-routing-managed-launch");
            let messages = body.contains("windows-routing-messages");
            assert!(
                managed || messages || body.contains("windows-routing-responses"),
                "unknown loopback route request"
            );
            let streaming = request.body["stream"] == true;
            let (input, output) = if managed {
                (23, 8)
            } else {
                match (messages, streaming) {
                    (false, false) => (11, 7),
                    (false, true) => (13, 5),
                    (true, false) => (17, 9),
                    (true, true) => (19, 6),
                }
            };
            let response = json!({"id":format!("windows-route-{input}"),"object":"response","model":MODEL,"status":"completed",
                "output":[{"type":"message","id":format!("message-{input}"),"role":"assistant","status":"completed", "content":[{"type":"output_text","text":"windows native route answer","annotations":[]}]}],
                "usage":{"input_tokens":input,"output_tokens":output,"total_tokens":input+output,"input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":2}}});
            if !streaming {
                return http::Reply::json(200, response);
            }
            let events = [
                (
                    "response.created",
                    json!({"type":"response.created","response":{"id":format!("windows-route-{input}"),"object":"response","model":MODEL,"status":"in_progress","output":[]}}),
                ),
                (
                    "response.output_item.added",
                    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":format!("message-{input}"),"role":"assistant","status":"in_progress","content":[]}}),
                ),
                (
                    "response.content_part.added",
                    json!({"type":"response.content_part.added","item_id":format!("message-{input}"),"output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
                ),
                (
                    "response.output_text.delta",
                    json!({"type":"response.output_text.delta","item_id":format!("message-{input}"),"output_index":0,"content_index":0,"delta":"windows native route answer"}),
                ),
                (
                    "response.output_text.done",
                    json!({"type":"response.output_text.done","item_id":format!("message-{input}"),"output_index":0,"content_index":0,"text":"windows native route answer"}),
                ),
                (
                    "response.completed",
                    json!({"type":"response.completed","response":response}),
                ),
            ];
            http::Reply::sse(
                events
                    .into_iter()
                    .map(|(name, value)| format!("event: {name}\ndata: {value}\n\n"))
                    .collect(),
            )
        });
        let source = Business::save_on_with_context(
            product,
            "candidate/native/windows-routing-business",
            "windows-routing-source",
            upstream.address,
            131072,
        );
        assert_eq!(*counts.lock().unwrap(), (1, 0));
        let mut plan = Plan::create(product, source.binding_id(), "windows-public-route");
        plan.update_and_reject_stale(product);
        let options = cli(product, &["compute", "connection", "options"], None);
        let model_id = options["data"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|option| option["model_configuration_ids"].as_array().unwrap())
            .find_map(Value::as_str)
            .expect("published current model catalog must offer reference models");
        let model = cli(
            product,
            &["models", "show"],
            Some(&json!({"kind":"reference","model_configuration_id":model_id})),
        );
        assert_eq!(model["data"]["kind"], "reference");
        Self {
            plan,
            _source: source,
            upstream,
            counts,
        }
    }
    pub fn assert_native_launch_reached_upstream(&self) {
        assert!(
            self.counts.lock().unwrap().1 >= 5,
            "released managed launch must make an actual authenticated upstream request"
        );
    }

    /// Both connections must be public-configured and allow this plan. Their protocols
    /// remain distinct: Codex Responses cannot authorize Claude Messages.
    pub fn traffic_and_observe(
        &self,
        product: &InstalledProduct,
        gateway: SocketAddr,
        responses_connection: &str,
        messages_connection: &str,
    ) {
        let response_token = grant(product, responses_connection);
        let messages_token = grant(product, messages_connection);
        let response_body = |stream: bool| json!({"model":self.plan.alias,"input":[{"role":"user","content":[{"type":"input_text","text":"windows-routing-responses"}]}],"stream":stream,"instructions":"bounded Windows route prompt".repeat(4000)});
        for token in [None, Some("invalid-windows-gateway-token")] {
            let denied = http::post(gateway, "/v1/responses", token, &response_body(false));
            assert!(
                matches!(denied.status, 401 | 403),
                "unauthenticated Gateway must reject before upstream"
            );
        }
        assert_eq!(*self.counts.lock().unwrap(), (1, 0));
        for streaming in [false, true] {
            let response = http::post(
                gateway,
                "/v1/responses",
                Some(&response_token),
                &response_body(streaming),
            );
            assert_eq!(response.status, 200);
            if streaming {
                let events = response.events();
                assert!(
                    events
                        .iter()
                        .any(|(name, event)| name == "response.output_text.delta"
                            && event["delta"] == "windows native route answer")
                );
                assert!(
                    events
                        .iter()
                        .any(|(name, event)| name == "response.completed"
                            && event["response"]["status"] == "completed")
                );
            } else {
                assert!(
                    response.json()["output"]
                        .to_string()
                        .contains("windows native route answer")
                );
            }
        }
        let messages_body = |stream: bool| json!({"model":self.plan.alias,"messages":[{"role":"user","content":[{"type":"text","text":"windows-routing-messages"}]}],"max_tokens":128,"stream":stream});
        let wrong_protocol = http::post(
            gateway,
            "/v1/messages",
            Some(&response_token),
            &messages_body(false),
        );
        assert!(
            matches!(wrong_protocol.status, 401 | 403),
            "Responses grant must not authorize Messages"
        );
        assert_eq!(*self.counts.lock().unwrap(), (1, 2));
        for streaming in [false, true] {
            let response = http::post(
                gateway,
                "/v1/messages",
                Some(&messages_token),
                &messages_body(streaming),
            );
            assert_eq!(response.status, 200);
            if streaming {
                let events = response.events();
                assert!(
                    events
                        .iter()
                        .any(|(name, event)| name == "content_block_delta"
                            && event["delta"]["text"] == "windows native route answer")
                );
                assert!(events.iter().any(|(name, _)| name == "message_stop"));
            } else {
                assert_eq!(response.json()["type"], "message");
                assert!(
                    response.json()["content"]
                        .to_string()
                        .contains("windows native route answer")
                );
            }
        }
        assert_eq!(*self.counts.lock().unwrap(), (1, 4));
        observation::assert_committed(product, &self.plan.plan_id);
        let _ = self.upstream.address; // The oracle stays alive through public ingestion/value reads.
    }
}

/// The same native hidden helper used by managed Agents; raw material never enters
/// command JSON, argv, environment, fixtures on disk, assertions or diagnostics.
pub(crate) fn grant(product: &InstalledProduct, connection: &str) -> Zeroizing<String> {
    use hiroute_application_api::HIDDEN_AGENT_GRANT_HELPER_VERB_V1;
    let args = [
        OsString::from(HIDDEN_AGENT_GRANT_HELPER_VERB_V1),
        OsString::from(connection),
    ];
    let environment = std::env::vars_os().collect::<Vec<_>>();
    let mut child = Child::spawn(Launch {
        program: &product.stable,
        args: &args,
        env: &environment,
        cwd: product.stable.parent(),
        stdin: Stream::Null,
        stdout: Stream::Piped,
        stderr: Stream::Piped,
    })
    .unwrap();
    let capture = |file: std::fs::File| {
        thread::spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            file.take(65537).read_to_end(&mut bytes).unwrap();
            assert!(bytes.len() <= 65536);
            bytes
        })
    };
    let output = capture(child.stdout.take().unwrap());
    let error = capture(child.stderr.take().unwrap());
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(exit) = child.exit_code().unwrap() {
            assert_eq!(exit, 0, "native Agent grant helper must succeed");
            break;
        }
        if Instant::now() >= deadline {
            child.terminate().unwrap();
            panic!("native grant helper exceeded bound");
        }
        thread::sleep(Duration::from_millis(10));
    }
    while !child.empty().unwrap() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let bytes = output.join().unwrap();
    let error = error.join().unwrap();
    assert_no_secret(&error);
    assert!(error.is_empty(), "native grant helper emitted diagnostics");
    let token = std::str::from_utf8(&bytes)
        .unwrap()
        .strip_suffix('\n')
        .expect("native grant helper must emit one terminated token");
    assert!(
        !token.is_empty()
            && token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "native grant helper must emit exactly one base64url token"
    );
    track_secret(token.as_bytes());
    Zeroizing::new(token.to_owned())
}
