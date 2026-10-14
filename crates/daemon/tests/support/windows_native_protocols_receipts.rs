//! Current public safe-fact projection: native model belongs to AttemptStarted,
//! usage belongs to UsageAndCache, joined only by the actual public request ID.
use super::{InstalledProduct, MODEL, UpstreamProtocol, cli, usage};
use hiroute_domain::{ObservationFactsPageV2, ObservationSafeFactV2};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}
fn observation(view: &str, query: Value) -> Value {
    json!({"schema":"hiroute.observation.query/v2","intent":{"view":view,"query":query}})
}
fn query(plan: &str, session: Option<&str>) -> Value {
    json!({"from_ms":now()-600_000,"to_ms":now(),"session_id":session,"request_id":null,
        "plan_id":plan,"limit":50,"cursor":null,"only_model_switch":false})
}
pub fn committed(
    product: &InstalledProduct,
    plan: &str,
    protocol: UpstreamProtocol,
) -> Vec<String> {
    let expected: BTreeSet<_> = [usage(protocol, false), usage(protocol, true)]
        .into_iter()
        .collect();
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut observed = BTreeMap::new();
    while observed.len() < 2 {
        let listed = cli(
            product,
            &["sessions", "list"],
            Some(&observation("sessions", query(plan, None))),
        );
        for session in listed["data"]["sessions"].as_array().unwrap() {
            let session = session["session_id"].as_str().unwrap();
            let timeline = cli(
                product,
                &["sessions", "show"],
                Some(&observation("timeline", query(plan, Some(session)))),
            );
            for row in timeline["data"]["requests"].as_array().unwrap() {
                assert_eq!(row["routing_context"]["state"], "recorded");
                assert_eq!(row["routing_context"]["plan_id"], plan);
                assert_eq!(row["session_id"], session);
                if row["final_native_model"] != MODEL {
                    continue;
                }
                let id = row["request_id"].as_str().unwrap();
                let receipt = cli(
                    product,
                    &["sessions", "receipt"],
                    Some(&observation(
                        "facts",
                        json!({"request_id":id,"limit":100,"cursor":null}),
                    )),
                );
                let facts: ObservationFactsPageV2 = serde_json::from_value(receipt["data"].clone())
                    .expect("real receipt must match its production public DTO");
                if let Some(pair) = admitted(&facts.facts, protocol) {
                    assert!(expected.contains(&pair));
                    if let Some(prior) = observed.insert(pair, id.to_owned()) {
                        assert_eq!(
                            prior, id,
                            "a usage pair must identify one independent actual request"
                        );
                    }
                }
            }
        }
        if observed.len() < 2 {
            assert!(
                Instant::now() < deadline,
                "actual Native protocol usage and success receipts did not become queryable"
            );
            thread::sleep(Duration::from_millis(100));
        }
    }
    assert_eq!(observed.keys().copied().collect::<BTreeSet<_>>(), expected);
    let values = loop {
        let value = cli(product, &["value", "show", "--routing", plan], None)["data"].clone();
        if value["summary"]["pending_requests"] == 0 && sums(&value) == totals(protocol) {
            break value;
        }
        assert!(
            Instant::now() < deadline,
            "actual Native protocol value projection did not settle reported usage"
        );
        thread::sleep(Duration::from_millis(100));
    };
    for dimension in values["summary"]["usage"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|value| matches!(value["metric"].as_str(), Some("input" | "output")))
    {
        assert_eq!(dimension["coverage"], "complete");
    }
    assert!(
        values["summary"]["amounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|amount| amount["known_sum_micros"].is_null()),
        "custom Native models cannot fabricate catalog prices"
    );
    observed.into_values().collect()
}
fn admitted(facts: &[ObservationSafeFactV2], protocol: UpstreamProtocol) -> Option<(u64, u64)> {
    assert!(
        facts
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    let finished = facts
        .iter()
        .find(|fact| fact.event_kind == "request_finished")?;
    assert_eq!(finished.outcome.as_deref(), Some("accepted"));
    let started = facts
        .iter()
        .find(|fact| fact.event_kind == "attempt_started")
        .unwrap();
    assert_eq!(started.native_model.as_deref(), Some(MODEL));
    assert!(started.attempt_ordinal.is_some());
    let usages: Vec<_> = facts
        .iter()
        .filter(|fact| fact.event_kind == "usage_and_cache")
        .collect();
    assert_eq!(
        usages.len(),
        1,
        "fixed Native plan must have one actual attempt usage fact"
    );
    let measured = usages[0];
    assert_eq!(measured.attempt_ordinal, started.attempt_ordinal);
    assert!(
        measured.native_model.is_none(),
        "safe usage fact must not invent a native model field"
    );
    assert_eq!(measured.cache_read_tokens, Some(3));
    match protocol {
        UpstreamProtocol::ChatCompletions => {
            assert_eq!(measured.cache_write_tokens, None);
            assert_eq!(measured.reasoning_tokens, Some(2));
        }
        UpstreamProtocol::Messages => {
            assert_eq!(measured.cache_write_tokens, None);
            assert_eq!(measured.reasoning_tokens, None);
        }
        _ => unreachable!(),
    }
    Some((
        measured.input_tokens.unwrap(),
        measured.output_tokens.unwrap(),
    ))
}
fn sums(value: &Value) -> (u64, u64) {
    let values = value["summary"]["usage"].as_array().unwrap();
    let metric = |name| {
        values.iter().find(|item| item["metric"] == name).unwrap()["known_sum"]
            .as_u64()
            .unwrap_or(0)
    };
    (metric("input"), metric("output"))
}
fn totals(protocol: UpstreamProtocol) -> (u64, u64) {
    let (a, b) = usage(protocol, false);
    let (c, d) = usage(protocol, true);
    (a + c, b + d)
}
