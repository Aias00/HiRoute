//! Independent actual upstream peers for ordinary saved Qoder models. A loopback
//! source is local-model evidence, never evidence of provider-account inference.
use super::super::{SECRET, windows_business_http as http, windows_routing_business::MODEL};
#[cfg(windows)]
use super::settings::Snapshot;
use hiroute_domain::UpstreamProtocol;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;
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
    collaboration_step: usize,
}
struct Challenge {
    nonce: String,
    skill: PathBuf,
    command: String,
    expected_cli: Value,
}
impl Oracle {
    pub fn start(
        protocol: UpstreamProtocol,
        skill: PathBuf,
        cli: PathBuf,
        expected_cli: Value,
        nonce: String,
    ) -> Self {
        assert!(matches!(
            protocol,
            UpstreamProtocol::Responses | UpstreamProtocol::Messages
        ));
        let ledger = Arc::new(Mutex::new(Ledger::default()));
        let observed = ledger.clone();
        let challenge = Challenge {
            nonce,
            skill,
            command: format!(
                "& '{}' schema list --output json",
                cli.to_string_lossy().replace('\'', "''")
            ),
            expected_cli,
        };
        let server = http::Server::start(move |request| {
            assert!(
                request.authenticated(&format!("Bearer {SECRET}")),
                "saved Native source credential must reach its real upstream"
            );
            assert!(
                request.method == "POST",
                "ordinary native must use the inference method"
            );
            assert!(
                request.path
                    == if protocol == UpstreamProtocol::Responses {
                        "/v1/responses"
                    } else {
                        "/v1/messages"
                    },
                "ordinary native must use its saved native protocol"
            );
            if protocol == UpstreamProtocol::Messages {
                assert!(
                    request.header("anthropic-version") == Some("2023-06-01"),
                    "ordinary Messages native protocol header is required"
                );
            }
            let mut ledger = observed.lock().unwrap();
            respond(protocol, &request.body, &mut ledger, &challenge)
                .expect("ordinary native request failed a closed protocol/correlation assertion")
        });
        Self { server, ledger }
    }
}

fn respond(
    protocol: UpstreamProtocol,
    body: &Value,
    ledger: &mut Ledger,
    challenge: &Challenge,
) -> Result<http::Reply, &'static str> {
    if body["model"] != MODEL {
        return Err("native-model");
    }
    if protocol == UpstreamProtocol::Messages
        && (!body["messages"]
            .as_array()
            .is_some_and(|messages| !messages.is_empty())
            || !body["max_tokens"].as_u64().is_some_and(|bound| bound > 0))
    {
        return Err("native-messages-shape");
    }
    if tool(body, "hiroute_probe").is_some() {
        ledger.probe += 1;
        if ledger.probe != 1 || body["stream"] != false {
            return Err("probe-count-or-mode");
        }
        return Ok(if protocol == UpstreamProtocol::Messages {
            http::Reply::json(
                200,
                json!({"id":"qoder-native-probe","type":"message","role":"assistant","model":MODEL,"content":[{"type":"tool_use","id":"probe-call","name":"hiroute_probe","input":{"value":"ok"}}],"stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":2,"output_tokens":1}}),
            )
        } else {
            responses(
                body,
                "probe",
                function(body, "probe", "hiroute_probe", json!({"value":"ok"}))?,
                (2, 1),
            )
        });
    }
    if body["stream"] != true {
        return Err("ordinary-stream-mode");
    }
    let label = format!("WINDOWS-QODER-USER-MODEL-{}-", challenge.nonce);
    let encoded = body.to_string();
    let stage = ["configured", "rotated", "collaboration", "restarted"]
        .into_iter()
        .find(|stage| encoded.contains(&format!("{label}{stage}")))
        .ok_or("ordinary-independent-nonce")?;
    let pair = (100 + ledger.requests.len() as u64, 3);
    if stage == "collaboration" {
        if protocol != UpstreamProtocol::Responses {
            return Err("skill-protocol");
        }
        let item = collaboration(body, ledger, challenge)?;
        ledger.requests.push(pair);
        return Ok(responses(body, stage, item, pair));
    }
    if !ledger.completed.insert(stage.into()) {
        return Err("ordinary-repeat");
    }
    ledger.requests.push(pair);
    let receipt = format!("qoder-user-model-{stage}-{}", challenge.nonce);
    Ok(if protocol == UpstreamProtocol::Messages {
        messages(&receipt, pair)
    } else {
        responses(body, stage, message(&receipt), pair)
    })
}

fn collaboration(
    body: &Value,
    ledger: &mut Ledger,
    challenge: &Challenge,
) -> Result<Value, &'static str> {
    let input = body["input"].as_array().ok_or("skill-input")?;
    let item = match ledger.collaboration_step {
        0 => {
            if tool(body, "Skill").is_none()
                || tool(body, "Bash").is_none()
                || !body.to_string().contains("hiroute-collaboration")
            {
                return Err("skill-discovery");
            }
            function(
                body,
                "qoder_skill",
                "Skill",
                json!({"skill":"hiroute-collaboration"}),
            )?
        }
        1 => {
            if correlated(body, "call_qoder_skill")
                != Some("Launching skill: hiroute-collaboration")
            {
                return Err("skill-call-result");
            }
            let contents = native_skill_contents(&challenge.skill)?;
            let skill_body = contents
                .strip_prefix("---\n")
                .and_then(|tail| tail.split_once("\n---\n"))
                .map(|(_, body)| body.trim())
                .filter(|body| !body.is_empty())
                .ok_or("skill-front-matter")?;
            let directory = std::fs::canonicalize(challenge.skill.parent().ok_or("skill-target")?)
                .map_err(|_| "skill-target")?;
            let prefix = format!(
                "Base directory for this skill: {}\n\n",
                directory.to_str().ok_or("skill-target-encoding")?
            );
            let loaded = input
                .iter()
                .filter(|item| item["type"] == "message" && item["role"] == "user")
                .filter_map(|item| item["content"].as_array())
                .flatten()
                .filter_map(|part| part["text"].as_str())
                .any(|text| {
                    text.strip_prefix(&prefix)
                        .is_some_and(|body| body.trim() == skill_body)
                });
            if !loaded {
                return Err("skill-actual-path-and-body");
            }
            function(
                body,
                "qoder_cli",
                "Bash",
                json!({"command":challenge.command,"timeout":10000}),
            )?
        }
        2 => {
            let output = correlated(body, "call_qoder_cli").ok_or("skill-cli-correlation")?;
            let envelope = envelope(output)?;
            if envelope != challenge.expected_cli || envelope["status"] != "succeeded" {
                return Err("skill-cli-exact-envelope");
            }
            ledger.cli_proven = true;
            if !ledger.completed.insert("collaboration".into()) {
                return Err("skill-repeat");
            }
            message(&format!(
                "qoder-user-model-collaboration-{}",
                challenge.nonce
            ))
        }
        _ => return Err("skill-request-count"),
    };
    ledger.collaboration_step += 1;
    Ok(item)
}
#[cfg(windows)]
fn native_skill_contents(
    path: &std::path::Path,
) -> Result<zeroize::Zeroizing<String>, &'static str> {
    let snapshot = Snapshot::read(path);
    Ok(zeroize::Zeroizing::new(
        std::str::from_utf8(&snapshot.bytes)
            .map_err(|_| "skill-encoding")?
            .to_owned(),
    ))
}
#[cfg(not(windows))]
fn native_skill_contents(_: &std::path::Path) -> Result<zeroize::Zeroizing<String>, &'static str> {
    // Cross-platform pure oracle tests never substitute a native Skill read.
    Err("native-skill-requires-windows")
}
fn tool<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body["tools"]
        .as_array()?
        .iter()
        .find(|tool| tool["name"] == name)
}
fn function(
    body: &Value,
    suffix: &str,
    name: &str,
    arguments: Value,
) -> Result<Value, &'static str> {
    let declaration = tool(body, name).ok_or("native-tool-declaration")?;
    if let Some(required) = declaration["parameters"]["required"].as_array()
        && required
            .iter()
            .filter_map(Value::as_str)
            .any(|field| arguments.get(field).is_none())
    {
        return Err("native-tool-arguments");
    }
    Ok(
        json!({"id":format!("fc_{suffix}"),"type":"function_call","status":"completed","call_id":format!("call_{suffix}"),"name":name,"arguments":arguments.to_string()}),
    )
}
fn correlated<'a>(body: &'a Value, id: &str) -> Option<&'a str> {
    let mut outputs = body["input"].as_array()?.iter().filter(|item| {
        item["type"] == "function_call_output" && item["call_id"] == id && item["is_error"] != true
    });
    let output = outputs.next()?["output"].as_str()?;
    outputs.next().is_none().then_some(output)
}
fn envelope(output: &str) -> Result<Value, &'static str> {
    let envelopes: Vec<_> = output
        .match_indices('{')
        .filter_map(|(offset, _)| {
            serde_json::Deserializer::from_str(&output[offset..])
                .into_iter::<Value>()
                .next()?
                .ok()
        })
        .filter(|value| value.get("schema_version").is_some() && value.get("status").is_some())
        .collect();
    if envelopes.len() != 1 {
        return Err("skill-cli-envelope-count");
    }
    Ok(envelopes.into_iter().next().unwrap())
}
fn message(text: &str) -> Value {
    json!({"id":"qoder_user_done","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":text,"annotations":[]}]})
}
fn responses(body: &Value, stage: &str, item: Value, pair: (u64, u64)) -> http::Reply {
    let response = json!({"id":format!("resp_qoder_user_{stage}_{}",pair.0),"object":"response","created_at":1,"model":MODEL,"status":"completed","output":[item],"usage":{"input_tokens":pair.0,"output_tokens":pair.1,"total_tokens":pair.0+pair.1}});
    if body["stream"] != true {
        return http::Reply::json(200, response);
    }
    let mut created = response.clone();
    created["status"] = json!("in_progress");
    created["output"] = json!([]);
    let mut initial = item.clone();
    initial["status"] = json!("in_progress");
    let terminal = item["type"] == "message";
    initial[if terminal { "content" } else { "arguments" }] =
        if terminal { json!([]) } else { json!("") };
    let mut events = vec![
        ("response.created", json!({"response":created})),
        (
            "response.output_item.added",
            json!({"output_index":0,"item":initial}),
        ),
    ];
    if terminal {
        events.extend([
            ("response.content_part.added",json!({"item_id":item["id"],"output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}})),
            ("response.output_text.delta",json!({"item_id":item["id"],"output_index":0,"content_index":0,"delta":item["content"][0]["text"]})),
            ("response.output_text.done",json!({"item_id":item["id"],"output_index":0,"content_index":0,"text":item["content"][0]["text"]})),
            ("response.content_part.done",json!({"item_id":item["id"],"output_index":0,"content_index":0,"part":item["content"][0]})),
        ]);
    } else {
        events.extend([
            (
                "response.function_call_arguments.delta",
                json!({"item_id":item["id"],"output_index":0,"delta":item["arguments"]}),
            ),
            (
                "response.function_call_arguments.done",
                json!({"item_id":item["id"],"output_index":0,"arguments":item["arguments"]}),
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
    http::Reply::sse(
        events
            .into_iter()
            .enumerate()
            .map(|(sequence, (name, mut event))| {
                event["type"] = json!(name);
                event["sequence_number"] = json!(sequence);
                format!("event: {name}\ndata: {event}\n\n")
            })
            .collect(),
    )
}
fn messages(text: &str, pair: (u64, u64)) -> http::Reply {
    let events = [
        (
            "message_start",
            json!({"type":"message_start","message":{"id":"qoder_user_messages","type":"message","role":"assistant","model":MODEL,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":pair.0,"output_tokens":0}}}),
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
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":pair.1}}),
        ),
        ("message_stop", json!({"type":"message_stop"})),
    ];
    http::Reply::sse(
        events
            .into_iter()
            .map(|(name, event)| format!("event: {name}\ndata: {event}\n\n"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn challenge() -> Challenge {
        Challenge {
            nonce: "independent".into(),
            skill: "unread-during-negative-oracles".into(),
            command: "trusted-public-command".into(),
            expected_cli: json!({"schema_version":{"major":1,"minor":0},"status":"succeeded","data":{"commands":[]}}),
        }
    }
    #[test]
    fn ordinary_qoder_protocol_oracle_requires_independent_request_and_exact_usage() {
        for protocol in [UpstreamProtocol::Responses, UpstreamProtocol::Messages] {
            let mut ledger = Ledger::default();
            let mut body = json!({"model":MODEL,"stream":true,"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"WINDOWS-QODER-USER-MODEL-independent-configured"}]}]});
            if protocol == UpstreamProtocol::Messages {
                body.as_object_mut().unwrap().remove("input");
                body["messages"] = json!([{"role":"user","content":"WINDOWS-QODER-USER-MODEL-independent-configured"}]);
                body["max_tokens"] = json!(32);
            }
            let reply = respond(protocol, &body, &mut ledger, &challenge()).unwrap();
            let encoded = String::from_utf8(reply.body).unwrap();
            assert!(encoded.contains("qoder-user-model-configured-independent"));
            assert!(
                encoded.contains("input_tokens\":100") && encoded.contains("output_tokens\":3")
            );
            assert!(
                ledger.requests == [(100, 3)]
                    && ledger.completed == BTreeSet::from(["configured".into()])
            );
            assert!(respond(protocol, &body, &mut ledger, &challenge()).is_err());
            let mut wrong = body.clone();
            wrong["model"] = json!("other");
            assert!(respond(protocol, &wrong, &mut Ledger::default(), &challenge()).is_err());
            wrong["model"] = json!(MODEL);
            wrong["input"] = json!("unarmed");
            wrong["messages"] = json!([]);
            assert!(respond(protocol, &wrong, &mut Ledger::default(), &challenge()).is_err());
        }
    }
    #[test]
    fn ordinary_qoder_skill_oracle_rejects_uncorrelated_error_and_fake_cli_success() {
        let proof = challenge();
        let mut ledger = Ledger {
            collaboration_step: 2,
            ..Ledger::default()
        };
        let body = |call: &str, output: &str, error: bool| json!({"input":[{"type":"function_call_output","call_id":call,"output":output,"is_error":error}]});
        let expected = proof.expected_cli.to_string();
        assert!(collaboration(&body("other", &expected, false), &mut ledger, &proof).is_err());
        assert!(
            collaboration(
                &body("call_qoder_cli", &expected, true),
                &mut ledger,
                &proof
            )
            .is_err()
        );
        assert!(collaboration(&body("call_qoder_cli",r#"{"schema_version":{"major":1,"minor":0},"status":"succeeded","data":{"commands":["fabricated"]}}"#,false),&mut ledger,&proof).is_err());
        assert!(
            collaboration(
                &body("call_qoder_cli", &format!("{expected}\n{expected}"), false),
                &mut ledger,
                &proof
            )
            .is_err()
        );
        assert!(!ledger.cli_proven && ledger.completed.is_empty());
        assert!(
            collaboration(
                &body("call_qoder_cli", &expected, false),
                &mut ledger,
                &proof
            )
            .is_ok()
        );
        assert!(ledger.cli_proven && ledger.completed == BTreeSet::from(["collaboration".into()]));
        assert!(
            collaboration(
                &body("call_qoder_cli", &expected, false),
                &mut ledger,
                &proof
            )
            .is_err()
        );
    }
}
