//! A separate safe projection: published compute snapshots retain their fingerprint semantics.

use serde::{Deserialize, Serialize};

pub const GET_COMPUTE_KEY_MASKS_OPERATION_V1: &str = "GetComputeKeyMasksV1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputeKeyMasksQueryV1 {
    pub source_id: String,
    pub expected_source_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputeKeyMaskViewV1 {
    pub key_id: String,
    pub generation: u64,
    pub masked_key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputeKeyMasksViewV1 {
    pub source_id: String,
    pub source_revision: u64,
    pub keys: Vec<ComputeKeyMaskViewV1>,
}
