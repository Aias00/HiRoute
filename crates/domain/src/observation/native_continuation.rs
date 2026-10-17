//! Optional authenticated observation metadata. It never changes physical
//! session identity, ExecutionFact payloads, routing authority or Worker links.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProofObservationV1 {
    pub observer_id: String,
    pub sequence: u64,
    pub observed_at_ms: i64,
    pub age_ms: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCurrentStateV1 {
    pub value: NativeSessionStateV1,
    pub observation: NativeProofObservationV1,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSessionStateV1 {
    Open,
    CompressionClosed,
    OtherClosed,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeInvalidEdgesV1 {
    pub edges: Vec<[String; 2]>,
    pub observation: NativeProofObservationV1,
}
/// Wire-only native IDs. Gateway translates every ID through the same existing
/// scoped HMAC function before constructing the separate persisted carrier.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HermesCompressionLineageV1 {
    pub version: u8,
    pub ids: Vec<String>,
    pub complete: bool,
    pub proof_observation: NativeProofObservationV1,
    #[serde(default)]
    pub current_state: Option<NativeCurrentStateV1>,
    #[serde(default)]
    pub invalid_edges: Option<NativeInvalidEdgesV1>,
}
pub const NATIVE_CONTINUATION_SCHEMA_V1: &str = "hiroute.observation.native-continuation/v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeContinuationEvidenceV1 {
    pub schema_version: String,
    pub workspace_id: String,
    pub request_id: String,
    pub current_session_id: String,
    pub ids: Vec<String>,
    pub complete: bool,
    pub proof_observation: Option<NativeProofObservationV1>,
    pub current_state: Option<NativeCurrentStateV1>,
    pub invalid_edges: Option<NativeInvalidEdgesV1>,
    pub ingress_at_ms: i64,
    pub producer_id: String,
    pub producer_epoch: String,
    pub stream_id: String,
    pub sequence: u64,
    pub event_id: String,
}
