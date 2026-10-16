//! Account-free Free-mode admission boundaries. No provider Key is invented and
//! no paid custom source is relabelled as free. Positive provider execution is a
//! separate credentialed gate; these are real public negative/catalog scenarios.
use super::InstalledProduct;
use super::windows_headless_business::{cli, failure};
use super::windows_routing_business::judgment;
use hiroute_application_api::ComputeConnectionOptionsResultV1;
use hiroute_domain::BillingClass;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn run(product: &InstalledProduct, paid_primary: &str) {
    let before_sources = cli(product, &["compute", "list"], None)["data"].clone();
    let before_plans = cli(product, &["routing", "list"], None)["data"].clone();
    let options = cli(product, &["compute", "connection", "options"], None);
    let typed: ComputeConnectionOptionsResultV1 = serde_json::from_value(options["data"].clone())
        .expect("current Free catalog must match its public production DTO");
    let free: Vec<_> = typed
        .options
        .iter()
        .filter(|option| option.billing_class == BillingClass::Free)
        .collect();
    assert_eq!(
        free.iter()
            .map(|option| option.connection_option_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "groq.free-plan.global.v1",
            "mistral.free-experiment.global.v1",
            "modelscope.free.cn.v1",
            "openrouter.free.global.v1",
            "siliconflow.free.cn.v1"
        ]),
        "new released Free provider options require an explicit native admission gate update"
    );
    for (index, option) in free.iter().enumerate() {
        assert_eq!(option.authentication, "provider_api_key");
        assert!(option.registered_check_available);
        assert!(!option.endpoints.is_empty());
        // The unregistered candidate is a correlation ref, not fabricated
        // credentials. Production returns before invoking its HTTP probe.
        let candidate = format!("candidate/native/windows-free-missing-{index}");
        let request = json!({"kind":"registered","request":{
            "connection_option_id":option.connection_option_id,"display_name":"Windows Free admission",
            "expected_catalog":options["data"]["catalog"],"candidate_ref":candidate,
            "lineage_ref":format!("lineage/windows-free-missing-{index}"),"edit_revision":1,
            "check_id":format!("check/windows/free-missing-{index}"),
            "input_candidate":{"candidate_ref":candidate,"candidate_revision":1}}});
        failure(
            product,
            &["compute", "connection", "test"],
            &request,
            "RESOURCE_NOT_FOUND",
        );
        let mut stale = request;
        stale["request"]["expected_catalog"]["connector_registry_digest"] =
            json!(format!("sha256:{}", "0".repeat(64)));
        failure(
            product,
            &["compute", "connection", "test"],
            &stale,
            "REVISION_CONFLICT",
        );
    }
    for fallback in [false, true] {
        let editor = json!({"schema":"hiroute.plan-editor/v2","display_name":"Windows empty Free route","purpose":"Account-free admission",
            "mode":"free_first","candidates":[],"smart":{"economy":[],"primary":[],"judgment":judgment(),
                "reselect_on_user_message":false,"classifier":{"kind":"local_rules"},"complex_keywords":[]},
            "free":{"candidates":[],"primary":[{"binding_id":paid_primary}],"primary_fallback":fallback},
            "delegation_enabled":false,"requirements":{},"limits":{"maximum_attempts":1,"request_timeout_ms":30000,"attempt_timeout_ms":30000}});
        failure(
            product,
            &["routing", "preview"],
            &json!({"change":{"schema":"hiroute.plan-content-change/v2",
            "target":{"intent":"create","creation_key":format!("windows-empty-free-{fallback}")},"editor":editor,"consumed_draft":null}}),
            "INVALID_ARGUMENTS",
        );
    }
    assert_eq!(
        cli(product, &["routing", "list"], None)["data"],
        before_plans
    );
    assert_eq!(
        stable_sources(&cli(product, &["compute", "list"], None)["data"]),
        stable_sources(&before_sources)
    );
    println!(
        "Windows public Free admission: current credentialed catalog, absent Key rejection, empty free pool with both fallback choices and unchanged sources/plans verified; no positive free provider execution claimed"
    );
}
fn stable_sources(snapshot: &Value) -> Value {
    let mut sources = snapshot["sources"].clone();
    for source in sources.as_array_mut().unwrap() {
        for model in source["models"].as_array_mut().unwrap() {
            // Availability is query-time evidence. Exclude only its clock, while
            // preserving actual state, credentials, targets and all model facts.
            model["presentation"]
                .as_object_mut()
                .unwrap()
                .remove("evaluated_at_ms");
        }
    }
    sources
}
