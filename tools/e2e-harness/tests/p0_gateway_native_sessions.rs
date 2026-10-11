//! Native session claims through the production listener and observation producer.
mod runtime_support;

use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use runtime_support::{NativeProvider, ObservationFaults, ProviderReply, RuntimeFixture, request};
use serde_json::{Value, json};

const RESPONSE: &[u8] = br#"{"id":"accepted","model":"runtime-native","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}"#;
const DECISION: &[u8] =
    br#"{"decision":{"kind":"ordinal","probabilities":{"simple":0.9,"complex":0.1}}}"#;

fn provider(body: &'static [u8]) -> NativeProvider {
    NativeProvider::start(vec![
        ProviderReply::Complete {
            status: 200,
            error_kind: None,
            body,
        };
        128
    ])
}

fn body(messages: bool, text: &str, metadata: Option<&str>) -> Value {
    let mut body = if messages {
        json!({"model":"runtime-model","max_tokens":64,"messages":[{"role":"user","content":text}]})
    } else {
        json!({"model":"runtime-model","input":text,"stream":false})
    };
    if let Some(metadata) = metadata {
        body["metadata"] = json!({"user_id":metadata});
    }
    body
}

fn send(
    fixture: &RuntimeFixture,
    messages: bool,
    headers: &[(&str, &str)],
    body: &Value,
    ordinal: usize,
) -> Value {
    let mut headers = headers.to_vec();
    headers.push((
        "X-HiRoute-Token",
        if messages {
            runtime_support::MESSAGES_TOKEN
        } else {
            runtime_support::TOKEN
        },
    ));
    let response = request(
        fixture.address,
        "POST",
        if messages {
            "/v1/messages"
        } else {
            "/v1/responses"
        },
        &headers,
        &serde_json::to_vec(body).unwrap(),
    );
    assert_eq!(
        response.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    let finished = finished(fixture.observation_root.as_deref().unwrap(), ordinal);
    finished.last().unwrap()["correlation"].clone()
}

fn finished(root: &Path, count: usize) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let records: Vec<Value> = fs::read_to_string(root.join("execution-fact.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let finished: Vec<_> = records
            .into_iter()
            .filter(|record| {
                record["fact"]["kind"] == "request_finished"
                    && record["fact"]["outcome"] == "accepted"
            })
            .collect();
        if finished.len() >= count {
            assert_eq!(
                finished.len(),
                count,
                "each request must finish exactly once"
            );
            return finished;
        }
        assert!(
            Instant::now() < deadline,
            "missing executed request {count}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn real_listener_native_sessions_prioritize_ids_and_isolate_identical_content() {
    let simple = provider(RESPONSE);
    let complex = provider(RESPONSE);
    let classifier = provider(DECISION);
    let fixture = RuntimeFixture::launch_rest_classified_with_observation(
        &[&simple, &complex, &classifier],
        1,
        ObservationFaults::healthy(),
    );
    let mut ordinal = 0;
    for messages in [false, true] {
        let mut cases = vec![
            ("x-claude-code-session-id", None),
            ("session-id", None), // Qoder and Codex keep their existing namespace.
            ("x-deepseek-harness-session-id", None),
            ("x-session-id", Some("pi (linux; x64)")),
        ];
        if messages {
            cases.push(("x-session-affinity", Some("pi (linux; x64)")));
            cases.push(("x-session-affinity", Some("deepseek-harness/0.2.0")));
        } else {
            cases.push(("x-client-request-id", Some("pi (linux; x64)")));
            cases.push(("session_id", Some("pi (linux; x64)")));
            cases.push(("x-client-request-id", Some("deepseek-harness/0.2.0")));
        }
        for (index, (name, agent)) in cases.into_iter().enumerate() {
            let native = format!("native-{messages}-{index}");
            let other = format!("other-{messages}-{index}");
            let mut correlations = Vec::new();
            for (id, text) in [
                (&native, "identical first user"),
                (&native, "# AGENTS.md instructions\nrebuilt native context"),
                (&other, "identical first user"),
            ] {
                let mut headers = vec![(name, id.as_str())];
                if let Some(agent) = agent {
                    headers.push(("User-Agent", agent));
                }
                ordinal += 1;
                let correlation = send(
                    &fixture,
                    messages,
                    &headers,
                    &body(messages, text, None),
                    ordinal,
                );
                assert_eq!(correlation["session_scope"], "conversation", "{name}");
                assert_eq!(
                    correlation["correlation_provenance"], "agent_supplied",
                    "{name}"
                );
                assert!(
                    !correlation["conversation_id"]
                        .as_str()
                        .unwrap()
                        .contains(id)
                );
                correlations.push(correlation);
            }
            assert_eq!(
                correlations[0]["conversation_id"], correlations[1]["conversation_id"],
                "{name}: rewritten content must keep native session"
            );
            assert_ne!(
                correlations[0]["conversation_id"], correlations[2]["conversation_id"],
                "{name}: identical content must not join different native sessions"
            );
        }
    }
    assert_eq!(simple.calls() + complex.calls(), ordinal);
}

#[test]
fn real_listener_invalid_native_claims_and_unscoped_hints_remain_request_scoped() {
    let simple = provider(RESPONSE);
    let complex = provider(RESPONSE);
    let classifier = provider(DECISION);
    let fixture = RuntimeFixture::launch_rest_classified_with_observation(
        &[&simple, &complex, &classifier],
        1,
        ObservationFaults::healthy(),
    );
    let mut ordinal = 0;
    let ordinary = "identical ordinary first user";
    // Prove this content is inferable: invalid native claims must suppress the
    // same fallback that otherwise joins these two ordinary requests.
    let mut inferred = Vec::new();
    for _ in 0..2 {
        ordinal += 1;
        let correlation = send(&fixture, false, &[], &body(false, ordinary, None), ordinal);
        assert_eq!(correlation["session_scope"], "conversation");
        assert_eq!(correlation["correlation_provenance"], "gateway_generated");
        inferred.push(correlation);
    }
    assert_eq!(
        inferred[0]["conversation_id"],
        inferred[1]["conversation_id"]
    );
    let cases = [
        (
            false,
            vec![
                ("x-claude-code-session-id", "same"),
                ("x-claude-code-session-id", "same"),
            ],
            None,
        ),
        (
            true,
            vec![("x-claude-code-session-id", "header")],
            Some("{\"session_id\":\"body\"}"),
        ),
        (true, vec![("x-deepseek-harness-session-id", "")], None),
        (
            false,
            vec![
                ("User-Agent", "pi (browser)"),
                ("x-client-request-id", "a"),
                ("session_id", "b"),
            ],
            None,
        ),
        (
            false,
            vec![
                ("User-Agent", "ordinary-client/1"),
                ("x-client-request-id", "per-request"),
                ("session_id", "unknown"),
            ],
            None,
        ),
        (
            true,
            vec![
                ("x-session-affinity", "cache-root"),
                ("User-Agent", "ordinary-client/1"),
            ],
            None,
        ),
    ];
    let mut sessions = std::collections::BTreeSet::new();
    for (index, (messages, headers, metadata)) in cases.into_iter().enumerate() {
        let text = if index < 4 {
            ordinary
        } else {
            "# AGENTS.md instructions\nshared wrapper"
        };
        for _ in 0..2 {
            ordinal += 1;
            let correlation = send(
                &fixture,
                messages,
                &headers,
                &body(messages, text, metadata),
                ordinal,
            );
            assert_eq!(correlation["session_scope"], "request_scoped");
            assert_eq!(correlation["correlation_provenance"], "unproven");
            assert!(
                sessions.insert(correlation["conversation_id"].as_str().unwrap().to_owned()),
                "request-scoped claims must never join"
            );
        }
    }
    // A cache key is not a native session, even for a client carrying Pi branding.
    ordinal += 1;
    let mut cache_only = body(false, "# AGENTS.md instructions", None);
    cache_only["prompt_cache_key"] = json!("cache-root");
    let correlation = send(
        &fixture,
        false,
        &[("User-Agent", "pi (browser)")],
        &cache_only,
        ordinal,
    );
    assert_eq!(correlation["session_scope"], "request_scoped");
    assert_eq!(correlation["correlation_provenance"], "unproven");
    assert_eq!(simple.calls() + complex.calls(), ordinal);
}
