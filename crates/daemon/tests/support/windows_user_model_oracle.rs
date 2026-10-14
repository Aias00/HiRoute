//! Independent Responses source: a persisted native user route must reach it,
//! and collaboration completion needs actual read + public CLI tool receipts.
use super::super::SECRET;
use super::super::windows_business_http as http;
use super::super::windows_routing_business::MODEL;
use super::native::text;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct Oracle {
    pub server: http::Server,
    pub ledger: Arc<Mutex<Ledger>>,
}
#[derive(Default)]
pub struct Ledger {
    pub probe: usize,
    pub requests: Vec<(u64, u64)>,
    pub completed: BTreeSet<String>,
    pub cli_proven: bool,
}
impl Oracle {
    pub fn start(pi: bool, skill: PathBuf, cli: PathBuf, nonce: String) -> Self {
        let ledger = Arc::new(Mutex::new(Ledger::default()));
        let observed = ledger.clone();
        let server = http::Server::start(move |request| {
            assert!(
                request.authenticated(&format!("Bearer {SECRET}")),
                "saved source credential must reach its real upstream"
            );
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/v1/responses");
            assert_eq!(request.body["model"], MODEL);
            let body = &request.body;
            let mut ledger = observed.lock().unwrap();
            if tool(body, "hiroute_probe").is_some() {
                ledger.probe += 1;
                assert_eq!(ledger.probe, 1);
                assert_eq!(body["stream"], false);
                return reply(
                    body,
                    "probe",
                    function(body, "probe_call", "hiroute_probe", json!({"value":"ok"})),
                    (2, 1),
                );
            }
            let marker = format!("WINDOWS-USER-MODEL-{nonce}-");
            let text = text(body);
            assert!(
                text.contains(&marker),
                "only an independently armed ordinary native invocation may use this source"
            );
            let pair = (100 + ledger.requests.len() as u64, 3);
            ledger.requests.push(pair);
            if text.contains(&format!("{marker}collaboration")) {
                let item = if let Some(output) = correlated(body, "user_cli") {
                    let envelope = envelope(&output);
                    assert_eq!(envelope["status"], "succeeded");
                    assert!(
                        envelope["data"]["descriptor_digest"]
                            .as_str()
                            .is_some_and(|value| value.starts_with("sha256:"))
                    );
                    assert!(
                        envelope["data"]["commands"]
                            .as_array()
                            .is_some_and(|commands| commands
                                .iter()
                                .any(|command| command["command_id"] == "worker.exec"))
                    );
                    ledger.cli_proven = true;
                    ledger.completed.insert("collaboration".into());
                    message(&format!("user-model-collaboration-{nonce}"))
                } else if let Some(output) = correlated(body, "user_skill") {
                    let contents = std::fs::read_to_string(&skill).unwrap();
                    assert!(
                        contents
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .all(|line| output.contains(line)),
                        "ordinary native Main must read the actually installed user Skill"
                    );
                    if pi {
                        let command = format!("{} schema list --output json", bash_path(&cli));
                        function(
                            body,
                            "user_cli",
                            "bash",
                            json!({"command":command,"timeout":10}),
                        )
                    } else {
                        let command = format!("& {} schema list --output json", ps_path(&cli));
                        function(
                            body,
                            "user_cli",
                            "pwsh",
                            json!({"command":command,"description":"Read the public HiRoute schema","timeoutMs":10000,"run_in_background":false}),
                        )
                    }
                } else {
                    assert!(
                        text.contains("hiroute-collaboration"),
                        "ordinary native must discover the installed user Skill"
                    );
                    function(
                        body,
                        "user_skill",
                        "read",
                        if pi {
                            json!({"path":skill})
                        } else {
                            json!({"file_path":skill})
                        },
                    )
                };
                return reply(body, "collaboration", item, pair);
            }
            let stage = ["configured", "rotated", "restarted"]
                .into_iter()
                .find(|stage| text.contains(&format!("{marker}{stage}")))
                .expect("closed ordinary native request stage");
            assert!(
                ledger.completed.insert(stage.into()),
                "ordinary persisted invocation must make exactly one model request"
            );
            reply(
                body,
                stage,
                message(&format!("user-model-{stage}-{nonce}")),
                pair,
            )
        });
        Self { server, ledger }
    }
}
fn tool<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body["tools"]
        .as_array()?
        .iter()
        .find(|tool| tool["name"] == name || tool["function"]["name"] == name)
}
fn function(body: &Value, id: &str, name: &str, args: Value) -> Value {
    let tool = tool(body, name).expect("actual ordinary native must declare the selected tool");
    let schema = tool
        .get("parameters")
        .or_else(|| tool["function"].get("parameters"));
    if let Some(required) = schema.and_then(|schema| schema["required"].as_array()) {
        assert!(
            required
                .iter()
                .filter_map(Value::as_str)
                .all(|field| args.get(field).is_some()),
            "ordinary native function schema differs"
        );
    }
    json!({"type":"function_call","id":format!("item_{id}"),"call_id":id,"name":name,"arguments":args.to_string(),"status":"completed"})
}
fn message(text: &str) -> Value {
    json!({"type":"message","id":"item_done","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]})
}
fn correlated(body: &Value, id: &str) -> Option<String> {
    body["input"]
        .as_array()?
        .iter()
        .find(|item| {
            item["type"] == "function_call_output"
                && item["call_id"] == id
                && item["is_error"] != true
        })
        .map(|item| text(&item["output"]))
}
fn envelope(text: &str) -> Value {
    let values: Vec<Value> = text
        .match_indices('{')
        .filter_map(|(offset, _)| {
            serde_json::Deserializer::from_str(&text[offset..])
                .into_iter::<Value>()
                .next()?
                .ok()
        })
        .filter(|value| value.get("schema_version").is_some() && value.get("status").is_some())
        .collect();
    assert_eq!(
        values.len(),
        1,
        "ordinary native shell must return one genuine public CLI envelope"
    );
    values.into_iter().next().unwrap()
}
fn ps_path(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}
fn bash_path(path: &Path) -> String {
    format!(
        "'{}'",
        path.to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''")
    )
}
fn reply(body: &Value, id: &str, item: Value, pair: (u64, u64)) -> http::Reply {
    let response = json!({"id":format!("resp_user_{id}_{}",pair.0),"object":"response","created_at":1,"model":MODEL,"status":"completed","output":[item],
        "usage":{"input_tokens":pair.0,"output_tokens":pair.1,"total_tokens":pair.0+pair.1}});
    if body["stream"] != true {
        return http::Reply::json(200, response);
    }
    let mut initial = item.clone();
    initial["status"] = json!("in_progress");
    let terminal = item["type"] == "message";
    initial[if terminal { "content" } else { "arguments" }] =
        if terminal { json!([]) } else { json!("") };
    let mut created = response.clone();
    created["status"] = json!("in_progress");
    created["output"] = json!([]);
    let item_id = item["id"].as_str().unwrap();
    let mut events = vec![
        ("response.created", json!({"response":created})),
        (
            "response.output_item.added",
            json!({"output_index":0,"item":initial}),
        ),
    ];
    if terminal {
        let text = &item["content"][0]["text"];
        events.extend([
            ("response.content_part.added",json!({"item_id":item_id,"output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}})),
            ("response.output_text.delta",json!({"item_id":item_id,"output_index":0,"content_index":0,"delta":text})),
            ("response.output_text.done",json!({"item_id":item_id,"output_index":0,"content_index":0,"text":text})),
            ("response.content_part.done",json!({"item_id":item_id,"output_index":0,"content_index":0,"part":item["content"][0]})),
        ]);
    } else {
        events.extend([
            (
                "response.function_call_arguments.delta",
                json!({"item_id":item_id,"output_index":0,"delta":item["arguments"]}),
            ),
            (
                "response.function_call_arguments.done",
                json!({"item_id":item_id,"output_index":0,"arguments":item["arguments"]}),
            ),
        ]);
    }
    events.extend([
        (
            "response.output_item.done",
            json!({"output_index":0,"item":item}),
        ),
        ("response.completed", json!({"response":response})),
    ]);
    let payload = events
        .into_iter()
        .enumerate()
        .map(|(sequence, (name, mut event))| {
            event["type"] = json!(name);
            event["sequence_number"] = json!(sequence);
            format!("event: {name}\ndata: {event}\n\n")
        })
        .collect();
    http::Reply::sse(payload)
}
