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

fn resolved_protocol() -> Value {
    object(
        &["schema", "fixed", "prior_context_max_turns"],
        json!({"schema":{"const":hiroute_domain::SYSTEM_ONE_C_V1},"fixed":{"const":hiroute_domain::NativeFixedContentV1::c_v1()},"prior_context_max_turns":{"const":8}}),
    )
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
        json!({"assessment":assessment,"simple_threshold_millis":threshold(),"competence_floor_millis":threshold(),"standard":{"enum":["default","custom"]}}),
    );
    object(
        &["protocol", "context_layout", "decision", "scopes", "digest"],
        json!({"protocol":resolved_protocol(),"context_layout":{"const":hiroute_domain::C_CONTEXT_LAYOUT},"decision":{"oneOf":[ordinal,categorical]},"scopes":{"type":"object","minProperties":1,"maxProperties":16,"additionalProperties":scope},"digest":digest()}),
    )
}

pub(super) fn files() -> [(&'static str, String); 2] {
    let shape = json!({"enum":["standard","max_branches"]});
    let mut request = object(
        &[
            "schema",
            "service_id",
            "service_revision",
            "protocol",
            "shape",
        ],
        json!({"schema":{"const":crate::HISTORICAL_APPLICABILITY_TEST_SCHEMA_V1},"service_id":text(),"service_revision":{"type":"integer","minimum":1},"protocol":frozen(),"shape":shape}),
    );
    request["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    let mut result = object(
        &[
            "schema",
            "state",
            "service_id",
            "service_revision",
            "endpoint_digest",
            "model",
            "protocol_digest",
            "shape",
            "question_count",
            "tested_at_unix_ms",
            "duration_millis",
            "reason",
        ],
        json!({
            "schema":{"const":crate::HISTORICAL_APPLICABILITY_TEST_RESULT_SCHEMA_V1},"state":{"enum":["verified","unsupported","not_tested","test_failed"]},
            "service_id":text(),"service_revision":{"type":"integer","minimum":1},"endpoint_digest":digest(),"model":text(),"protocol_digest":digest(),"shape":shape,
            "question_count":{"type":"integer","minimum":1,"maximum":19},"tested_at_unix_ms":{"type":"integer","minimum":0},"duration_millis":{"type":"integer","minimum":0},"reason":{"type":"string","maxLength":128}
        }),
    );
    result["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    [
        (
            "historical-applicability-test.v1.schema.json",
            pretty_json(&request),
        ),
        (
            "historical-applicability-test-result.v1.schema.json",
            pretty_json(&result),
        ),
    ]
}
