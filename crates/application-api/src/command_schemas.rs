//! Installed, offline contracts for the management journey, derived from the wire DTOs.
//! These definitions describe syntax; current options and Preview remain authoritative for
//! dynamic IDs, permissions, capabilities and business validation.
use crate::*;
use schemars::{JsonSchema, generate::SchemaSettings};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(schemars::JsonSchema, Serialize)]
pub struct CommandSchemasV1 {
    pub request: Value,
    pub response: Value,
    pub examples: Vec<Value>,
}

fn schema<T: JsonSchema>(serialize: bool) -> Value {
    let settings = SchemaSettings::draft2020_12();
    let settings = if serialize {
        settings.for_serialize()
    } else {
        settings.for_deserialize()
    };
    let mut value = serde_json::to_value(settings.into_generator().into_root_schema_for::<T>())
        .expect("generated schema serializes");
    if !serialize {
        strict_objects(&mut value);
    }
    value
}

fn strict_objects(value: &mut Value) {
    match value {
        Value::Object(object) => {
            // CLI member validation is stricter than persisted serde readers. Maps retain their
            // typed additionalProperties; tagged unions retain their variant definitions.
            if object.contains_key("properties") && !object.contains_key("additionalProperties") {
                object.insert("additionalProperties".into(), json!(false));
            }
            for child in object.values_mut() {
                strict_objects(child);
            }
        }
        Value::Array(values) => {
            for value in values {
                strict_objects(value);
            }
        }
        _ => {}
    }
}

// Each root keeps its own definitions, so union alternatives cannot accidentally resolve a
// same-named definition from another wrapper. No reference ever leaves this document.
fn alternatives(values: Vec<Value>) -> Value {
    let mut defs = serde_json::Map::new();
    let mut refs = Vec::new();
    for (index, mut value) in values.into_iter().enumerate() {
        let name = format!("variant_{index}");
        rebase(&mut value, &format!("#/$defs/{name}"));
        value
            .as_object_mut()
            .expect("schema object")
            .remove("$schema");
        defs.insert(name.clone(), value);
        refs.push(json!({"$ref":format!("#/$defs/{name}")}));
    }
    json!({"$schema":"https://json-schema.org/draft/2020-12/schema", "anyOf":refs, "$defs":defs})
}
fn rebase(value: &mut Value, prefix: &str) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref") {
                if let Some(suffix) = reference.strip_prefix('#') {
                    *reference = format!("{prefix}{suffix}");
                }
            }
            for child in object.values_mut() {
                rebase(child, prefix);
            }
        }
        Value::Array(values) => {
            for value in values {
                rebase(value, prefix);
            }
        }
        _ => {}
    }
}

macro_rules! request { ($($ty:ty),+ $(,)?) => { alternatives(vec![$(schema::<$ty>(false)),+]) }; }
macro_rules! response { ($($ty:ty),+ $(,)?) => { alternatives(vec![$(schema::<MachineEnvelopeV2<$ty>>(true)),+]) }; }

pub fn command_schemas(command_id: &str) -> Option<CommandSchemasV1> {
    let (request, response) = match command_id {
        "agents.scan" | "agents.list" => (
            request!(AgentScanRequestV1),
            response!(AgentDiscoveryListV1),
        ),
        "system.client-status" => (
            request!(ClientEmptyRequestV1),
            response!(ClientServiceStatusV1),
        ),
        "compute.scan" => (
            request!(ComputeScanRequestV1, ComputeScanRequestV2),
            response!(ComputeScanResultV1, ComputeScanResultV2),
        ),
        "compute.list" | "compute.show" => (
            request!(ComputeManagementQueryV2),
            response!(ComputeManagementSnapshotV2),
        ),
        "compute.connection.options" => (
            request!(ComputeConnectionOptionsRequestV1),
            response!(ComputeConnectionOptionsResultV1),
        ),
        "compute.connection.prepare" => (
            request!(PrepareDiscoveredModelConnectionRequestV1),
            response!(ComputeCandidateViewV2),
        ),
        "compute.connection.authorize" => (
            request!(ComputeConnectionAuthorizationRequestV1),
            response!(ComputeSubscriptionCheckResultV2),
        ),
        "compute.connection.preview" => (
            request!(
                ComputeConnectionPreviewRequestV1,
                ComputeSavePreviewRequestV2,
                ComputeSubscriptionCheckPreviewRequestV2
            ),
            response!(
                ComputeConnectionPreviewResultV1,
                ComputeSavePreviewV2,
                ComputeSubscriptionCheckPreviewV2
            ),
        ),
        "compute.connection.apply" => (
            request!(ComputeConnectionApplyRequestV1),
            response!(ClientOperationViewV1),
        ),
        "compute.connection.login" => (
            request!(ComputeSubscriptionLoginRequestV1),
            response!(ComputeSubscriptionLoginResultV1),
        ),
        "compute.connection.test" => (
            request!(ComputeConnectionTestRequestV1),
            response!(ModelConnectionCheckViewV1),
        ),
        "routing.options" => (
            request!(PlanEditorOptionsRequestV1),
            response!(PlanEditorOptionsV1),
        ),
        "routing.list" => (
            request!(AgentPlanCatalogQueryV2),
            response!(AgentPlanCatalogViewV2),
        ),
        "routing.show" => (request!(AgentPlanLookupV1), response!(AgentPlanStatusV2)),
        "routing.preview" => (
            request!(
                PlanContentPreviewRequestV2,
                PlanDraftPreviewRequestV1,
                PlanLifecyclePreviewRequestV1,
                PublicationCheckpointPreviewRequestV1
            ),
            response!(
                PlanContentPreviewV2,
                PlanDraftPreviewV1,
                PlanLifecyclePreviewV1,
                PublicationCheckpointPreviewV1
            ),
        ),
        "routing.apply" => (
            request!(
                PlanContentApplyRequestV2,
                PlanDraftApplyRequestV1,
                PlanLifecycleApplyRequestV1,
                PublicationCheckpointApplyRequestV1
            ),
            response!(ClientOperationViewV1),
        ),
        "agents.connect.preview" => (
            request!(
                AgentConnectionPreviewRequestV1,
                AgentSettingsPreviewRequestV2
            ),
            response!(AgentConnectionPreviewResultV1, AgentSettingsPreviewV2),
        ),
        "agents.restore.preview" => (
            request!(AgentSettingsPreviewRequestV2),
            response!(AgentSettingsPreviewV2),
        ),
        "agents.connect.apply" => (
            request!(AgentConnectionApplyRequestV1, AgentSettingsApplyV2),
            response!(ClientOperationViewV1),
        ),
        "agents.restore.apply" => (
            request!(AgentSettingsApplyV2),
            response!(ClientOperationViewV1),
        ),
        "agents.connect.status" => (
            request!(AgentConnectionStatusRequestV1, AgentSettingsStatusRequestV2),
            response!(AgentConnectionStatusV1, AgentSettingsStatusV2),
        ),
        "operations.get" => (
            request!(OperationLookupV1),
            response!(ClientOperationViewV1),
        ),
        "operations.find" => (
            request!(OperationIdempotencyLookupV1),
            response!(OperationIdempotencyResultV1),
        ),
        "decision.services.apply" => (
            request!(PreviewRequestV1, ApplyRequestV1),
            response!(PreviewResultV1, ClientOperationViewV1),
        ),
        "decision.services.test" => (
            request!(ClassifierDecisionTestRequestV1),
            response!(ClassifierDecisionTestResultV1),
        ),
        _ => return None,
    };
    Some(CommandSchemasV1 {
        request,
        response,
        examples: examples(command_id),
    })
}

fn examples(command_id: &str) -> Vec<Value> {
    match command_id {
        "routing.options" => vec![json!({}), json!({"authoring":{}})],
        "compute.scan" => vec![json!({}), json!({"schema":COMPUTE_SCAN_REQUEST_SCHEMA_V2})],
        "compute.connection.options"
        | "compute.list"
        | "routing.list"
        | "agents.scan"
        | "agents.list" => (
            request!(AgentScanRequestV1),
            response!(AgentDiscoveryListV1),
        ),
        "system.client-status" => vec![json!({})],
        _ => vec![],
    }
}
