use super::*;

const RESPONSES_INCOMPLETE: &[u8] = br#"{"id":"partial-response","model":"runtime-native","status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"message","id":"partial-message","role":"assistant","status":"incomplete","content":[{"type":"output_text","text":"PRIVATE_PARTIAL_BODY"}]}],"usage":{"input_tokens":7,"output_tokens":2,"total_tokens":9}}"#;
const CHAT_INCOMPLETE: &[u8] = br#"{"id":"partial-chat","object":"chat.completion","created":1,"model":"runtime-native","choices":[{"index":0,"message":{"role":"assistant","content":"PRIVATE_PARTIAL_BODY"},"finish_reason":"length"}],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}}"#;
const MESSAGES_INCOMPLETE: &[u8] = br#"{"id":"partial-message","type":"message","role":"assistant","model":"runtime-native","content":[{"type":"text","text":"PRIVATE_PARTIAL_BODY"}],"stop_reason":"max_tokens","stop_sequence":null,"usage":{"input_tokens":7,"output_tokens":2}}"#;

#[test]
fn successful_nonstream_usage_is_recorded_before_request_finish_without_a_decision_stage() {
    for (protocol, body) in [
        ("responses", br#"{"id":"usage","model":"runtime-native","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}],"usage":{"input_tokens":32,"output_tokens":45,"total_tokens":77,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":36}}}"#.as_slice()),
        ("chat_completions", br#"{"id":"usage","object":"chat.completion","created":1,"model":"runtime-native","choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":32,"completion_tokens":45,"total_tokens":77,"prompt_tokens_details":{"cached_tokens":0},"completion_tokens_details":{"reasoning_tokens":36}}}"#.as_slice()),
        ("messages", br#"{"id":"usage","type":"message","role":"assistant","model":"runtime-native","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":32,"output_tokens":45,"cache_read_input_tokens":0}}"#.as_slice()),
    ] {
        serde_json::from_slice::<serde_json::Value>(body)
            .expect("valid nonstream provider JSON fixture");
        let provider = NativeProvider::start(vec![
            ProviderReply::Complete { status: 200, error_kind: None, body },
            ProviderReply::Complete { status: 200, error_kind: None, body },
        ]);
        let fixture = RuntimeFixture::launch_with_observation_and_publication_candidates(
            &[&provider], 1, ObservationFaults::healthy(),
            Some(&[PublicationCandidate {
                provider_index: 0, upstream_protocol: protocol, statically_enabled: true,
            }]),
        );
        let response = fixture.request_body(br#"{"model":"runtime-model","input":"hello","stream":false}"#);
        let facts = wait_execution_facts(&fixture, 1);
        assert_eq!(provider.calls(), 1);
        let requests = provider.requests();
        let offset = requests[0].windows(4).position(|part| part == b"\r\n\r\n").unwrap() + 4;
        let upstream: serde_json::Value = serde_json::from_slice(&requests[0][offset..]).unwrap();
        let observed: Vec<_> = facts.iter().map(|row| &row["fact"]).collect();
        assert_eq!(upstream["stream"], false, "{protocol}: {observed:?}");
        assert_eq!(response.status, 200, "{protocol}: {observed:?}");
        let document: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(document["status"], "completed");
        assert_eq!(document["usage"]["input_tokens"], 32);
        assert_eq!(document["usage"]["output_tokens"], 45);

        let terminal = facts.iter().find(|row| row["fact"]["kind"] == "request_finished").unwrap();
        assert_eq!(terminal["fact"]["outcome"], "accepted");
        assert_eq!(terminal["fact"]["facts_completeness"], "complete");
        let request_id = terminal["correlation"]["request_id"].as_str().unwrap();
        // The content terminal proves this capture worker has enqueued any
        // usage. Content and execution have independent persistence workers.
        let path = fixture.observation_root.as_ref().unwrap().join("conversation-content.jsonl");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let complete = std::fs::read_to_string(&path).ok().is_some_and(|text| {
                text.lines().filter(|line| !line.is_empty()).any(|line| {
                    let row: serde_json::Value = serde_json::from_str(line).unwrap();
                    row["correlation"]["request_id"] == request_id
                        && row["direction"] == "response_delivered" && row["phase"] == "finish"
                })
            });
            if complete { break; }
            assert!(Instant::now() < deadline, "response capture did not complete: {protocol}");
            std::thread::sleep(Duration::from_millis(10));
        }
        // A subsequent execution terminal on the same FIFO producer proves
        // the first capture's enqueued facts have also been persisted.
        let marker = fixture.request_body(br#"{"model":"runtime-model","input":"usage flush marker","stream":false}"#);
        assert_eq!(marker.status, 200, "{protocol}");
        assert_eq!(provider.calls(), 2);
        let facts = wait_execution_facts(&fixture, 2);
        let usage: Vec<_> = facts.iter().filter(|row|
            row["correlation"]["request_id"] == request_id
                && row["fact"]["kind"] == "usage_and_cache"
        ).collect();
        assert_eq!(usage.len(), 1, "{protocol}: {facts:?}");
        let usage = usage[0];
        assert_eq!(usage["fact"]["source"], "provider_completion");
        assert_eq!(usage["fact"]["ordinal"], 1);
        assert_eq!(usage["correlation"]["request_id"], request_id);
        assert!(usage["sequence"].as_u64().unwrap() < terminal["sequence"].as_u64().unwrap());
        for (field, expected) in [("input_tokens", 32), ("output_tokens", 45), ("cache_read_tokens", 0)] {
            assert_eq!(usage["fact"][field], expected, "{protocol}: {field}");
        }
        assert_eq!(usage["fact"]["reasoning_tokens"], if protocol == "messages" { serde_json::Value::Null } else { serde_json::json!(36) });
        assert!(usage["fact"]["cache_write_tokens"].is_null());
        assert!(usage["fact"]["billable_tokens"].is_null());
        assert_eq!(usage["fact"]["input_provenance"], "reported");
        assert!(!facts.iter().any(|row| row["fact"]["kind"] == "agent_turn_finished"));
    }
}

#[test]
fn known_nonstream_incomplete_relays_before_body_and_retains_usage_within_budget() {
    for (protocol, body) in [
        ("responses", RESPONSES_INCOMPLETE),
        ("chat_completions", CHAT_INCOMPLETE),
        ("messages", MESSAGES_INCOMPLETE),
    ] {
        for max_attempts in [1, 2] {
            let first = NativeProvider::start(vec![ProviderReply::Complete {
                status: 200,
                error_kind: None,
                body,
            }]);
            let second = NativeProvider::start(vec![ProviderReply::Complete {
                status: 200,
                error_kind: None,
                body: FALLBACK_OK,
            }]);
            let candidates = [
                PublicationCandidate {
                    provider_index: 0,
                    upstream_protocol: protocol,
                    statically_enabled: true,
                },
                PublicationCandidate {
                    provider_index: 1,
                    upstream_protocol: "responses",
                    statically_enabled: true,
                },
            ];
            let fixture = RuntimeFixture::launch_with_observation_and_publication_candidates(
                &[&first, &second],
                max_attempts,
                ObservationFaults::healthy(),
                Some(&candidates),
            );
            let response = fixture
                .request_body(br#"{"model":"runtime-model","input":"hello","stream":false}"#);
            let label = format!("{protocol}, max_attempts={max_attempts}");
            let wire = String::from_utf8_lossy(&response.body);
            assert_eq!(
                response.status,
                if max_attempts == 2 { 200 } else { 502 },
                "{label}: {wire}"
            );
            assert_eq!(first.calls(), 1, "{label}");
            assert_eq!(second.calls(), usize::from(max_attempts == 2), "{label}");
            assert!(!wire.contains("PRIVATE_PARTIAL_BODY"), "{label}: {wire}");
            if max_attempts == 2 {
                assert!(wire.contains("fallback"), "{label}: {wire}");
            }
            let facts = wait_execution_facts(&fixture, 1);
            assert!(
                facts
                    .iter()
                    .any(|row| row.pointer("/fact/kind").and_then(|v| v.as_str())
                        == Some("attempt_finished")
                        && row.pointer("/fact/retryable").and_then(|v| v.as_bool()) == Some(true)
                        && row.pointer("/fact/disposition").and_then(|v| v.as_str())
                            == Some("continue")),
                "prebody failure must remain relayable: {label}: {facts:?}"
            );
            assert!(
                facts
                    .iter()
                    .any(|row| row.pointer("/fact/kind").and_then(|v| v.as_str())
                        == Some("usage_and_cache")
                        && row.pointer("/fact/input_tokens").and_then(|v| v.as_u64()) == Some(7)
                        && row.pointer("/fact/output_tokens").and_then(|v| v.as_u64()) == Some(2)),
                "incomplete attempt usage must survive: {label}: {facts:?}"
            );
        }
    }
}
