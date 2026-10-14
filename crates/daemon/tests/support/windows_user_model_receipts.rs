//! Public observation joins actual independently counted native model requests.
use super::InstalledProduct;
use super::windows_headless_business::cli;
use super::windows_routing_business::MODEL;
use hiroute_domain::ObservationFactsPageV2;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn committed(
    product: &InstalledProduct,
    plan: &str,
    expected: &[(u64, u64)],
) -> BTreeSet<String> {
    assert!(!expected.is_empty());
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    let now = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    let query = |session: Option<&str>| {
        json!({"from_ms":now()-600_000,"to_ms":now(),"session_id":session,"request_id":null,
        "plan_id":plan,"limit":50,"cursor":null,"only_model_switch":false})
    };
    let observation = |view: &str, query| json!({"schema":"hiroute.observation.query/v2","intent":{"view":view,"query":query}});
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut observed = BTreeMap::new();
    loop {
        let sessions = cli(
            product,
            &["sessions", "list"],
            Some(&observation("sessions", query(None))),
        );
        for session in sessions["data"]["sessions"].as_array().unwrap() {
            let id = session["session_id"].as_str().unwrap();
            let timeline = cli(
                product,
                &["sessions", "show"],
                Some(&observation("timeline", query(Some(id)))),
            );
            for row in timeline["data"]["requests"].as_array().unwrap() {
                assert_eq!(row["routing_context"]["state"], "recorded");
                assert_eq!(row["routing_context"]["plan_id"], plan);
                let request = row["request_id"].as_str().unwrap();
                let receipt = cli(
                    product,
                    &["sessions", "receipt"],
                    Some(&observation(
                        "facts",
                        json!({"request_id":request,"limit":100,"cursor":null}),
                    )),
                );
                let facts: ObservationFactsPageV2 =
                    serde_json::from_value(receipt["data"].clone()).unwrap();
                assert!(!facts.projection_partial);
                let Some(finished) = facts
                    .facts
                    .iter()
                    .find(|fact| fact.event_kind == "request_finished")
                else {
                    continue;
                };
                assert_eq!(finished.outcome.as_deref(), Some("accepted"));
                let attempts: Vec<_> = facts
                    .facts
                    .iter()
                    .filter(|fact| fact.event_kind == "attempt_started")
                    .collect();
                let usage: Vec<_> = facts
                    .facts
                    .iter()
                    .filter(|fact| fact.event_kind == "usage_and_cache")
                    .collect();
                assert_eq!(attempts.len(), 1);
                assert_eq!(usage.len(), 1);
                assert_eq!(attempts[0].native_model.as_deref(), Some(MODEL));
                assert!(attempts[0].attempt_ordinal.is_some());
                assert_eq!(usage[0].attempt_ordinal, attempts[0].attempt_ordinal);
                assert!(usage[0].native_model.is_none());
                let pair = (
                    usage[0].input_tokens.unwrap(),
                    usage[0].output_tokens.unwrap(),
                );
                assert!(
                    expected.contains(&pair),
                    "public usage must correlate an actual independently measured native request"
                );
                if let Some(prior) = observed.insert(pair, request.to_owned()) {
                    assert_eq!(prior, request);
                }
            }
        }
        if observed.len() == expected.len() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "ordinary native model receipts did not become queryable"
        );
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(observed.keys().copied().collect::<BTreeSet<_>>(), expected);
    observed.into_values().collect()
}
