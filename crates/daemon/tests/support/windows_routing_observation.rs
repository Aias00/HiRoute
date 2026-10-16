//! Public current observation queries over the actual Gateway ingestion and value worker.
use super::{InstalledProduct, MODEL, cli};
use hiroute_domain::{ObservationCompletenessScope, ObservationStatusV1};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USAGE: [(u64, u64); 4] = [(11, 7), (13, 5), (17, 9), (19, 6)];
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}
fn query(plan: &str, session: Option<&str>) -> Value {
    json!({"from_ms":now()-600_000,"to_ms":now(),"session_id":session,"request_id":null,
        "plan_id":plan,"limit":50,"cursor":null,"only_model_switch":false})
}
fn request(view: &str, query: Value) -> Value {
    json!({"schema":"hiroute.observation.query/v2","intent":{"view":view,"query":query}})
}

pub fn assert_committed(product: &InstalledProduct, plan: &str) {
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut observed = BTreeSet::new();
    let mut sessions = BTreeSet::new();
    while observed.len() < USAGE.len() {
        let listed = cli(
            product,
            &["sessions", "list"],
            Some(&request("sessions", query(plan, None))),
        );
        for session in listed["data"]["sessions"].as_array().unwrap() {
            let session_id = session["session_id"].as_str().unwrap();
            let timeline = cli(
                product,
                &["sessions", "show"],
                Some(&request("timeline", query(plan, Some(session_id)))),
            );
            for row in timeline["data"]["requests"].as_array().unwrap() {
                assert_eq!(row["session_id"], session_id);
                assert_eq!(row["routing_context"]["state"], "recorded");
                assert_eq!(row["routing_context"]["plan_id"], plan);
                if row["final_native_model"] != MODEL {
                    continue;
                }
                let request_id = row["request_id"].as_str().unwrap();
                let facts = cli(
                    product,
                    &["sessions", "receipt"],
                    Some(&request(
                        "facts",
                        json!({"request_id":request_id,"limit":100,"cursor":null}),
                    )),
                );
                let values = facts["data"]["facts"].as_array().unwrap();
                if let Some(usage) = values.iter().find(|fact| {
                    fact["input_tokens"]
                        .as_u64()
                        .zip(fact["output_tokens"].as_u64())
                        .is_some_and(|pair| USAGE.contains(&pair))
                }) {
                    let pair = (
                        usage["input_tokens"].as_u64().unwrap(),
                        usage["output_tokens"].as_u64().unwrap(),
                    );
                    let ordinal = usage["attempt_ordinal"].as_u64().unwrap();
                    let started = values
                        .iter()
                        .find(|fact| {
                            fact["event_kind"] == "attempt_started"
                                && fact["attempt_ordinal"].as_u64() == Some(ordinal)
                        })
                        .expect("the actual request must contain its attempt model identity");
                    assert_eq!(started["native_model"], MODEL);
                    assert_eq!(started["attempt_ordinal"], usage["attempt_ordinal"]);
                    assert_eq!(usage["cache_read_tokens"], 3);
                    assert_eq!(usage["reasoning_tokens"], 2);
                    assert!(!values.is_empty());
                    assert!(
                        values
                            .windows(2)
                            .all(|pair| pair[0]["sequence"].as_u64().unwrap()
                                < pair[1]["sequence"].as_u64().unwrap()),
                        "safe facts must preserve actual receipt sequence"
                    );
                    if observed.insert(request_id.to_owned()) {
                        sessions.insert(session_id.to_owned());
                    }
                    assert!(USAGE.contains(&pair));
                }
            }
        }
        if observed.len() < USAGE.len() {
            assert!(
                Instant::now() < deadline,
                "actual Windows Gateway receipts did not become publicly queryable"
            );
            thread::sleep(Duration::from_millis(100));
        }
    }
    assert_eq!(
        observed.len(),
        4,
        "route fixture must have four independent requests"
    );
    assert!(!sessions.is_empty());
    let status = cli(product, &["sessions", "status"], None);
    // The current production DTO requires both completeness enums and all status
    // fields. Missing/null fields cannot pass as an allegedly available store.
    let status: ObservationStatusV1 = serde_json::from_value(status["data"].clone())
        .expect("actual sessions.status must conform to its production contract");
    assert_eq!(
        status.completeness_scope,
        ObservationCompletenessScope::GatewayVisible
    );
    assert_eq!(status.production_exporter, "not_installed");
    // The current plan-quality query must not invent rated assessments for this fixed route.
    let quality = cli(
        product,
        &["observation", "plan-quality", "samples", "--plan-id", plan],
        None,
    );
    assert_eq!(quality["data"]["summary"]["scored_stage_count"], 0);
    assert!(
        quality["data"]["samples"]
            .as_array()
            .unwrap()
            .iter()
            .all(|sample| sample["assessment"].is_null())
    );
    let value = loop {
        let value = cli(product, &["value", "show", "--routing", plan], None)["data"].clone();
        if value["summary"]["pending_requests"] == 0 && sums(&value["summary"]) == (60, 27) {
            break value;
        }
        assert!(
            Instant::now() < deadline,
            "actual value worker did not settle all route usage"
        );
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(value["group_by"], "none");
    assert!(value["days"].as_array().unwrap().is_empty());
    assert_usage(&value["summary"]);
    let grouped = cli(
        product,
        &["value", "show", "--routing", plan, "--group-by", "day"],
        None,
    );
    assert_eq!(grouped["data"]["group_by"], "day");
    assert_eq!(grouped["data"]["day_timezone"], "UTC");
    let days = grouped["data"]["days"].as_array().unwrap();
    assert!(!days.is_empty());
    assert_eq!(
        days.iter()
            .map(sums)
            .fold((0, 0), |(a, b), (x, y)| (a + x, b + y)),
        (60, 27)
    );
    assert!(
        days.iter()
            .flat_map(|day| day["amounts"].as_array().unwrap())
            .all(|amount| amount["known_sum_micros"].is_null()),
        "unknown Native model prices cannot be fabricated as zero"
    );
}
fn sums(summary: &Value) -> (u64, u64) {
    let usage = summary["usage"].as_array().unwrap();
    let count = |metric| {
        usage.iter().find(|item| item["metric"] == metric).unwrap()["known_sum"]
            .as_u64()
            .unwrap_or(0)
    };
    (count("input"), count("output"))
}
fn assert_usage(summary: &Value) {
    assert_eq!(sums(summary), (60, 27));
    for metric in ["input", "output"] {
        assert_eq!(
            summary["usage"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["metric"] == metric)
                .unwrap()["coverage"],
            "complete"
        );
    }
    assert!(
        summary["amounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|amount| amount["known_sum_micros"].is_null())
    );
}
