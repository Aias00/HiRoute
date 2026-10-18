//! Schemas owned by the current authoring/diagnostic contracts; fixed text comes from Domain.
use super::*;

fn object(required: &[&str], properties: Value) -> Value {
    json!({"type":"object","additionalProperties":false,"required":required,"properties":properties})
}
fn digest() -> Value {
    json!({"type":"string","pattern":"^sha256:[0-9a-f]{64}$"})
}
fn text() -> Value {
    json!({"type":"string","minLength":1})
}
fn threshold() -> Value {
    json!({"type":"integer","minimum":0,"maximum":1000})
}

pub(super) fn authoring() -> Value {
    object(
        &["schema", "fixed", "prior_context_max_turns"],
        json!({
            "schema":{"const":hiroute_domain::SYSTEM_ONE_C_V1},"fixed":{"const":hiroute_domain::NativeFixedContentV1::c_v1()},
            "prior_context_max_turns":{"type":"integer","minimum":0,"maximum":8,"default":8}
        }),
    )
}
pub(super) fn choices() -> Value {
    let source = object(
        &["plan", "scope"],
        json!({"scope":text(),"plan":object(&["workspace_id","plan_id","content_revision","content_digest"],json!({
            "workspace_id":text(),"plan_id":text(),"content_revision":{"type":"integer","minimum":1},"content_digest":digest()
        }))}),
    );
    let review = object(
        &["method"],
        json!({
            "method":{"enum":["stock","reviewed_custom","historical"]},"source":source
        }),
    );
    json!({"type":"object","maxProperties":17,"additionalProperties":review})
}
fn frozen() -> Value {
    let level = object(
        &["id", "criterion"],
        json!({"id":{"enum":["simple","complex"]},"criterion":text()}),
    );
    let ordinal = object(
        &["kind", "instructions", "levels"],
        json!({"kind":{"const":"ordinal"},"instructions":text(),"levels":{"type":"array","minItems":2,"maxItems":2,"items":level}}),
    );
    let option = object(
        &["id", "criterion"],
        json!({"id":text(),"criterion":text(),"refinement":ordinal}),
    );
    let categorical = object(
        &["kind", "instructions", "options"],
        json!({"kind":{"const":"categorical"},"instructions":text(),"options":{"type":"array","minItems":2,"maxItems":16,"items":option}}),
    );
    let assessment = object(
        &["type", "instructions", "criteria"],
        json!({"type":{"const":"score"},"instructions":text(),"criteria":{"type":"array","minItems":3,"maxItems":3,"items":text()}}),
    );
    let scope = object(
        &[
            "assessment",
            "simple_threshold_millis",
            "competence_floor_millis",
            "standard",
        ],
        json!({"assessment":assessment,"simple_threshold_millis":threshold(),"competence_floor_millis":threshold(),"standard":{"enum":["stock","custom"]}}),
    );
    object(
        &["protocol", "context_layout", "decision", "scopes", "digest"],
        json!({"protocol":authoring(),"context_layout":{"const":hiroute_domain::C_CONTEXT_LAYOUT},"decision":{"oneOf":[ordinal,categorical]},"scopes":{"type":"object","minProperties":1,"maxProperties":16,"additionalProperties":scope},"digest":digest()}),
    )
}

pub(super) fn files() -> [(&'static str, String); 2] {
    let shape = json!({"enum":["standard","max_branches"]});
    let mut saved = object(
        &["kind", "service_id", "service_revision"],
        json!({"kind":{"const":"saved"}, "service_id":text(), "service_revision":{"type":"integer","minimum":1}, "protocol":frozen(),"shape":shape}),
    );
    saved["dependentRequired"] = json!({"protocol":["shape"],"shape":["protocol"]});
    let value = object(
        &["kind", "classifier"],
        json!({"kind":{"const":"value"},"classifier":super::plan_authoring::classifier_definition()["oneOf"][1].clone()}),
    );
    let mut request = object(
        &["schema", "target"],
        json!({"schema":{"const":crate::CLASSIFIER_DECISION_TEST_SCHEMA_V2},"target":{"oneOf":[value,saved]}}),
    );
    request["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    let mut result = object(
        &[
            "schema",
            "state",
            "verified_shapes",
            "question_count",
            "tested_at_unix_ms",
            "duration_millis",
            "reason",
        ],
        json!({
            "schema":{"const":crate::CLASSIFIER_DECISION_TEST_RESULT_SCHEMA_V2},"state":{"enum":["verified","unsupported","not_tested","test_failed"]},"verified_shapes":{"type":"array","items":{"enum":["connectivity","category","degree","historical_assessment","applicability"]}},"branch_id":text(),"failure_code":text(),
            "service_id":text(),"service_revision":{"type":"integer","minimum":1},"endpoint_digest":digest(),"model":text(),"protocol_digest":digest(),"shape":shape,
            "question_count":{"type":"integer","minimum":1,"maximum":19},"tested_at_unix_ms":{"type":"integer","minimum":0},"duration_millis":{"type":"integer","minimum":0},"reason":{"type":"string","maxLength":128}
        }),
    );
    result["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    [
        (
            "classifier-decision-test.v2.schema.json",
            pretty_json(&request),
        ),
        (
            "classifier-decision-test-result.v2.schema.json",
            pretty_json(&result),
        ),
    ]
}
