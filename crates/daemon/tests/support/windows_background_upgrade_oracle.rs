//! Independent HTTP endpoint for the real upgrade consumer, never a HiRoute response fixture.
use super::super::windows_business_http as http;

use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub const MODEL: &str = "windows-upgrade-native-model";
pub const SECRET: &str = "windows-upgrade-private-native-key-2026";
pub const PHASES: [&str; 3] = ["before", "rollback", "candidate"];
pub const USAGE: [(u64, u64); 3] = [(31, 11), (37, 13), (41, 17)];

pub(super) fn assert_no_secret(bytes: &[u8]) {
    assert!(
        !bytes
            .windows(SECRET.len())
            .any(|part| part == SECRET.as_bytes()),
        "upgrade credential reached a public body"
    );
}

#[derive(Default, Debug, PartialEq)]
struct Counts {
    probe: usize,
    models: usize,
    traffic: [usize; 3],
    rejected: usize,
    invalid: usize,
}

pub struct Oracle {
    server: http::Server,
    counts: Arc<Mutex<Counts>>,
}
impl Oracle {
    pub fn start() -> Self {
        let counts = Arc::new(Mutex::new(Counts::default()));
        let actual = counts.clone();
        let server = http::Server::start(move |request| {
            let mut counts = actual.lock().unwrap();
            if !request.authenticated(&format!("Bearer {SECRET}")) {
                counts.rejected += 1;
                return http::Reply::json(401, json!({"error":"fixture credential rejected"}));
            }
            match (request.method.as_str(), request.path.as_str()) {
                ("GET", "/v1/models") => {
                    counts.models += 1;
                    http::Reply::json(
                        200,
                        json!({"object":"list","data":[{"id":MODEL,"object":"model"}]}),
                    )
                }
                ("POST", "/v1/responses")
                    if request.body["model"] == MODEL && request.body["stream"] == false =>
                {
                    if request.body["tools"].as_array().is_some_and(|tools| {
                        tools.iter().any(|tool| tool["name"] == "hiroute_probe")
                    }) {
                        counts.probe += 1;
                        return http::Reply::json(
                            200,
                            json!({"id":"upgrade-native-check","object":"response","model":MODEL,"status":"completed",
                            "output":[{"type":"function_call","id":"check-call","call_id":"check-call","name":"hiroute_probe","arguments":"{\"value\":\"ok\"}"}],
                            "usage":{"input_tokens":2,"output_tokens":1,"total_tokens":3}}),
                        );
                    }
                    let phase = request
                        .body
                        .pointer("/input/0/content/0/text")
                        .and_then(Value::as_str)
                        .and_then(|text| {
                            PHASES
                                .iter()
                                .position(|phase| text == format!("windows-upgrade-{phase}"))
                        });
                    if let Some(phase) = phase {
                        counts.traffic[phase] += 1;
                        let (input, output) = USAGE[phase];
                        return http::Reply::json(
                            200,
                            json!({"id":format!("upgrade-response-{input}"),"object":"response","model":MODEL,"status":"completed",
                            "output":[{"type":"message","id":format!("upgrade-message-{input}"),"role":"assistant","status":"completed",
                                "content":[{"type":"output_text","text":format!("windows-upgrade-{}-answer",PHASES[phase]),"annotations":[]}]}],
                            "usage":{"input_tokens":input,"output_tokens":output,"total_tokens":input+output,
                                "input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":2}}}),
                        );
                    }
                    counts.invalid += 1;
                    http::Reply::json(400, json!({"error":"unexpected fixture request"}))
                }
                _ => {
                    counts.invalid += 1;
                    http::Reply::json(400, json!({"error":"unexpected fixture protocol"}))
                }
            }
        });
        Self { server, counts }
    }
    pub fn address(&self) -> SocketAddr {
        self.server.address
    }
    pub fn assert_counts(&self, phases: &[usize], models: usize) {
        let mut traffic = [0; 3];
        for phase in phases {
            traffic[*phase] += 1;
        }
        assert_eq!(
            *self.counts.lock().unwrap(),
            Counts {
                probe: 1,
                models,
                traffic,
                rejected: 0,
                invalid: 0
            },
            "real saved-key and Gateway requests must reach the independent endpoint exactly once"
        );
    }
}

#[test]
fn upgrade_oracle_authenticates_and_distinguishes_each_real_request() {
    let oracle = Oracle::start();
    let body = |phase: usize| json!({"model":MODEL,"input":[{"role":"user","content":[{"type":"input_text","text":format!("windows-upgrade-{}",PHASES[phase])}]}],"stream":false});
    assert_eq!(
        http::post(
            oracle.address(),
            "/v1/responses",
            Some("foreign-key"),
            &body(0)
        )
        .status,
        401
    );
    let mut wrong_model = body(0);
    wrong_model["model"] = json!("different-model");
    assert_eq!(
        http::post(
            oracle.address(),
            "/v1/responses",
            Some(SECRET),
            &wrong_model
        )
        .status,
        400
    );
    // http::post adds Bearer itself: its token argument is the key, not a header.
    let probe = json!({"model":MODEL,"stream":false,"tools":[{"name":"hiroute_probe"}]});
    let checked = http::post(oracle.address(), "/v1/responses", Some(SECRET), &probe);
    assert_eq!(checked.status, 200);
    assert_eq!(checked.json()["output"][0]["name"], "hiroute_probe");
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(oracle.address()).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    write!(stream,"GET /v1/models HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {SECRET}\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = Vec::new();
    stream.take(65537).read_to_end(&mut response).unwrap();
    assert!(response.len() <= 65536 && response.starts_with(b"HTTP/1.1 200 "));
    assert_no_secret(&response);
    let body_offset = response
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    let models: Value = serde_json::from_slice(&response[body_offset..]).unwrap();
    assert_eq!(models["data"][0]["id"], MODEL);
    for phase in 0..3 {
        let response = http::post(
            oracle.address(),
            "/v1/responses",
            Some(SECRET),
            &body(phase),
        );
        assert_eq!(response.status, 200);
        let value = response.json();
        assert_eq!(value["model"], MODEL);
        assert_eq!(value["usage"]["input_tokens"], USAGE[phase].0);
        assert_eq!(value["usage"]["output_tokens"], USAGE[phase].1);
        assert_eq!(
            value["output"][0]["content"][0]["text"],
            format!("windows-upgrade-{}-answer", PHASES[phase])
        );
    }
    assert_eq!(
        *oracle.counts.lock().unwrap(),
        Counts {
            probe: 1,
            models: 1,
            traffic: [1, 1, 1],
            rejected: 1,
            invalid: 1
        }
    );
}
