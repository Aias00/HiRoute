//! Public Native-source admission and actual Gateway Responses conversion to
//! Chat Completions / Messages upstreams. The independent HTTP oracle observes
//! saved credentials, real paths and protocol shapes; public receipts prove use.
use super::windows_business_http as http;
use super::windows_headless_business::{Business, cli};
use super::windows_routing_business::{MODEL, Plan, grant};
use super::{InstalledProduct, SECRET};
#[path = "windows_native_protocols_receipts.rs"]
mod receipts;

use hiroute_domain::UpstreamProtocol;
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub struct NativeProtocols {
    pub chat: Source,
    pub messages: Source,
    context: String,
    restore: String,
}
pub struct Source {
    pub plan: Plan,
    source: Business,
    server: http::Server,
    counts: Arc<Mutex<(usize, usize, usize)>>,
    protocol: UpstreamProtocol,
    observed: Vec<String>,
}
impl NativeProtocols {
    pub fn setup(product: &InstalledProduct) -> Self {
        let chat = Source::setup(product, UpstreamProtocol::ChatCompletions);
        let messages = Source::setup(product, UpstreamProtocol::Messages);
        // Codex requires developer-role fidelity, so a Messages upstream must
        // not enter its catalog. Pi supports these bounded Responses requests;
        // obtain its real public grant instead of bypassing client admission.
        let scan = cli(product, &["agents", "scan"], None);
        let agent = scan["data"]["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["agent_id"] == "agent_pi_default")
            .unwrap();
        assert_eq!(agent["supported"], true);
        let context = agent["context_id"].as_str().unwrap().to_owned();
        super::windows_user_models::checked(product, "agent_pi_default");
        let spec = json!({"schema_version":{"major":2,"minor":0},"context_id":context,
            "model":{"intent":"configure","settings":{"mode":"pi_additional",
                "allowed_plan_ids":[chat.plan.plan_id,messages.plan.plan_id],
                "plan_protocols":{&chat.plan.plan_id:"responses",&messages.plan.plan_id:"responses"}}}});
        super::windows_user_models::apply(
            product,
            "connect",
            &spec,
            "windows-native-protocol-connect",
        );
        let status = cli(product, &["agents", "connect", "status", &context], None);
        assert_eq!(status["data"]["state"], "configured");
        let restore = status["data"]["restore_point_ref"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            chat,
            messages,
            context,
            restore,
        }
    }
    pub fn traffic(&mut self, product: &InstalledProduct, gateway: SocketAddr) {
        let token = grant(product, &format!("agent-connection/{}", self.context));
        for source in [&mut self.chat, &mut self.messages] {
            source.traffic(product, gateway, &token);
        }
    }
    pub fn recovered(&self, product: &InstalledProduct) {
        let status = cli(
            product,
            &["agents", "connect", "status", &self.context],
            None,
        );
        assert_eq!(status["data"]["state"], "configured");
        assert_eq!(status["data"]["restore_point_ref"], self.restore);
        for source in [&self.chat, &self.messages] {
            let shown = cli(
                product,
                &["compute", "show", source.source.source_id()],
                None,
            );
            assert_eq!(
                shown["data"]["sources"][0]["models"][0]["binding_id"],
                source.source.binding_id()
            );
            let plan = cli(product, &["routing", "show", &source.plan.plan_id], None);
            assert_eq!(plan["data"]["head"]["model_alias"], source.plan.alias);
            let observed = receipts::committed(product, &source.plan.plan_id, source.protocol);
            assert_eq!(
                observed, source.observed,
                "native protocol receipts changed across actual restart"
            );
            assert_eq!(
                *source.counts.lock().unwrap(),
                (1, 2, 0),
                "restart must not send unsolicited native inference"
            );
            let _ = source.server.address;
        }
    }
    pub fn restore(&self, product: &InstalledProduct) {
        let spec = json!({"schema_version":{"major":2,"minor":0},"context_id":self.context,
            "model":{"intent":"restore","restore_point_ref":self.restore}});
        super::windows_user_models::apply(
            product,
            "restore",
            &spec,
            "windows-native-protocol-restore",
        );
        let status = cli(
            product,
            &["agents", "connect", "status", &self.context],
            None,
        );
        assert!(matches!(
            status["data"]["state"].as_str(),
            Some("restored" | "not_configured")
        ));
    }
}
impl Source {
    fn setup(product: &InstalledProduct, protocol: UpstreamProtocol) -> Self {
        let counts = Arc::new(Mutex::new((0, 0, 0)));
        let requests = counts.clone();
        let server = http::Server::start(move |request| {
            assert!(
                request.authenticated(&format!("Bearer {SECRET}")),
                "actual Native transport must use the saved credential"
            );
            if request.method == "GET" {
                assert_eq!(request.path, "/v1/models");
                requests.lock().unwrap().2 += 1;
                return http::Reply::json(
                    200,
                    json!({"object":"list","data":[{"id":MODEL,"object":"model","created":1,"owned_by":"windows-native-oracle"}]}),
                );
            }
            assert_eq!(request.method, "POST");
            assert_eq!(
                request.path,
                match protocol {
                    UpstreamProtocol::ChatCompletions => "/v1/chat/completions",
                    UpstreamProtocol::Messages => "/v1/messages",
                    UpstreamProtocol::Responses => unreachable!(),
                }
            );
            assert_eq!(request.body["model"], MODEL);
            if protocol == UpstreamProtocol::Messages {
                assert_eq!(request.header("anthropic-version"), Some("2023-06-01"));
                assert!(
                    request.body["max_tokens"]
                        .as_u64()
                        .is_some_and(|value| value > 0)
                );
            }
            let tools = request.body["tools"].as_array().unwrap();
            let name = |tool: &Value| {
                if protocol == UpstreamProtocol::ChatCompletions {
                    tool["function"]["name"].as_str() == Some("hiroute_probe")
                } else {
                    tool["name"] == "hiroute_probe"
                }
            };
            if tools.iter().any(name) {
                assert_eq!(request.body["stream"], false);
                requests.lock().unwrap().0 += 1;
                return probe(protocol);
            }
            let encoded = serde_json::to_string(&request.body).unwrap();
            assert!(
                encoded.contains("windows-native-protocol-request"),
                "unexpected inference escaped the native protocol fixture"
            );
            assert!(
                tools.iter().any(|tool| {
                    if protocol == UpstreamProtocol::ChatCompletions {
                        tool["type"] == "function"
                            && tool["function"]["name"] == "native_roundtrip"
                            && tool["function"]["parameters"]["type"] == "object"
                    } else {
                        tool["name"] == "native_roundtrip"
                            && tool["input_schema"]["type"] == "object"
                    }
                }),
                "Responses tool declaration must reach the correct native upstream shape"
            );
            let streaming = request.body["stream"] == true;
            requests.lock().unwrap().1 += 1;
            reply(protocol, streaming)
        });
        let key = label(protocol);
        let source = Business::save_on_with_protocol(
            product,
            &format!("candidate/native/windows-{key}"),
            &format!("windows-native-{key}-source"),
            server.address,
            131072,
            protocol,
        );
        assert_eq!(*counts.lock().unwrap(), (1, 0, 0));
        let plan = Plan::create(
            product,
            source.binding_id(),
            &format!("windows-native-{key}-plan"),
        );
        Self {
            plan,
            source,
            server,
            counts,
            protocol,
            observed: Vec::new(),
        }
    }
    fn traffic(&mut self, product: &InstalledProduct, gateway: SocketAddr, token: &str) {
        for streaming in [false, true] {
            let body = json!({"model":self.plan.alias,"input":[{"role":"user","content":[{"type":"input_text","text":"windows-native-protocol-request"}]}],
                "max_output_tokens":128,"stream":streaming,
                "tools":[{"type":"function","name":"native_roundtrip","description":"Bounded native schema conversion","parameters":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false}}]});
            let response = http::post(gateway, "/v1/responses", Some(token), &body);
            assert_eq!(
                response.status, 200,
                "actual Native protocol route must succeed"
            );
            let (input, output) = usage(self.protocol, streaming);
            let document = if streaming {
                let events = response.events();
                assert!(
                    events
                        .iter()
                        .any(|(name, event)| name == "response.output_text.delta"
                            && event["delta"] == answer(self.protocol))
                );
                let terminals: Vec<_> = events
                    .iter()
                    .filter(|(name, _)| name == "response.completed")
                    .collect();
                assert_eq!(
                    terminals.len(),
                    1,
                    "conversion must publish exactly one completed terminal"
                );
                terminals[0].1["response"].clone()
            } else {
                response.json()
            };
            // The current streaming terminal publishes a response snapshot;
            // the non-streaming producer also publishes its object tag.
            if !streaming {
                assert_eq!(document["object"], "response");
            }
            assert_eq!(document["status"], "completed");
            assert_eq!(document["model"], self.plan.alias);
            assert!(
                document["output"]
                    .to_string()
                    .contains(answer(self.protocol))
            );
            assert_eq!(document["usage"]["input_tokens"], input);
            assert_eq!(document["usage"]["output_tokens"], output);
            assert_eq!(
                document["usage"]["input_tokens_details"]["cached_tokens"],
                3
            );
        }
        assert_eq!(*self.counts.lock().unwrap(), (1, 2, 0));
        self.observed = receipts::committed(product, &self.plan.plan_id, self.protocol);
    }
}
fn label(protocol: UpstreamProtocol) -> &'static str {
    match protocol {
        UpstreamProtocol::ChatCompletions => "chat",
        UpstreamProtocol::Messages => "messages",
        _ => unreachable!(),
    }
}
fn answer(protocol: UpstreamProtocol) -> &'static str {
    match protocol {
        UpstreamProtocol::ChatCompletions => "windows native Chat answer",
        UpstreamProtocol::Messages => "windows native Messages answer",
        _ => unreachable!(),
    }
}
pub(super) fn usage(protocol: UpstreamProtocol, stream: bool) -> (u64, u64) {
    match (protocol, stream) {
        (UpstreamProtocol::ChatCompletions, false) => (31, 7),
        (UpstreamProtocol::ChatCompletions, true) => (37, 11),
        // Messages' reported uncached input excludes cache reads. This oracle
        // intentionally has no cache-write usage: Responses cannot represent it.
        (UpstreamProtocol::Messages, false) => (44, 13),
        (UpstreamProtocol::Messages, true) => (46, 17),
        _ => unreachable!(),
    }
}
fn probe(protocol: UpstreamProtocol) -> http::Reply {
    http::Reply::json(
        200,
        match protocol {
            UpstreamProtocol::ChatCompletions => {
                json!({"id":"native-chat-check","object":"chat.completion","created":1,"model":MODEL,"choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"type":"function","id":"probe-call","function":{"name":"hiroute_probe","arguments":"{\"value\":\"ok\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}})
            }
            UpstreamProtocol::Messages => {
                json!({"id":"native-messages-check","type":"message","role":"assistant","model":MODEL,"content":[{"type":"tool_use","id":"probe-call","name":"hiroute_probe","input":{"value":"ok"}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":2,"output_tokens":1}})
            }
            _ => unreachable!(),
        },
    )
}
fn reply(protocol: UpstreamProtocol, streaming: bool) -> http::Reply {
    let (input, output) = usage(protocol, streaming);
    let id = format!("native-{}-{input}", label(protocol));
    let text = answer(protocol);
    if protocol == UpstreamProtocol::ChatCompletions {
        let usage = json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output,
            "prompt_tokens_details":{"cached_tokens":3},"completion_tokens_details":{"reasoning_tokens":2}});
        if !streaming {
            return http::Reply::json(
                200,
                json!({"id":id,"object":"chat.completion","created":1,"model":MODEL,
                "choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":usage}),
            );
        }
        let events = [
            json!({"id":id,"object":"chat.completion.chunk","created":1,"model":MODEL,"choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
            json!({"id":id,"object":"chat.completion.chunk","created":1,"model":MODEL,"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
            json!({"id":id,"object":"chat.completion.chunk","created":1,"model":MODEL,"choices":[],"usage":usage}),
        ];
        return http::Reply::sse(
            events
                .iter()
                .map(|value| format!("data: {value}\n\n"))
                .collect::<String>()
                + "data: [DONE]\n\n",
        );
    }
    let usage = json!({"input_tokens":input-3,"output_tokens":output,"cache_read_input_tokens":3});
    if !streaming {
        return http::Reply::json(
            200,
            json!({"id":id,"type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":text}],"stop_reason":"end_turn","stop_sequence":null,"usage":usage}),
        );
    }
    let events = [
        (
            "message_start",
            json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":MODEL,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":input-3,"output_tokens":0,"cache_read_input_tokens":3}}}),
        ),
        (
            "content_block_start",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        ),
        (
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        ),
        (
            "content_block_stop",
            json!({"type":"content_block_stop","index":0}),
        ),
        (
            "message_delta",
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":output}}),
        ),
        ("message_stop", json!({"type":"message_stop"})),
    ];
    http::Reply::sse(
        events
            .iter()
            .map(|(name, value)| format!("event: {name}\ndata: {value}\n\n"))
            .collect(),
    )
}
