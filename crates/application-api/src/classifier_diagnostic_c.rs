//! Explicit C capability probe under the existing protected diagnostic operation.
use hiroute_domain::{CanonicalDigest, FrozenNativeProtocolV1};
use serde::{Deserialize, Serialize};

pub const CLASSIFIER_DECISION_TEST_SCHEMA_V2: &str = "hiroute.classifier-decision-test/v2";
pub const CLASSIFIER_DECISION_TEST_RESULT_SCHEMA_V2: &str =
    "hiroute.classifier-decision-test-result/v2";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CCapabilityShapeV1 {
    Standard,
    MaxBranches,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CCapabilityStateV1 {
    Verified,
    Unsupported,
    NotTested,
    TestFailed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifierCapabilityProbeV1 {
    pub service_id: String,
    pub service_revision: u64,
    pub protocol: FrozenNativeProtocolV1,
    pub shape: CCapabilityShapeV1,
}
impl ClassifierCapabilityProbeV1 {
    pub fn validate(&self) -> bool {
        !self.service_id.is_empty()
            && self.service_id.len() <= 128
            && self.service_revision > 0
            && self.protocol.validate().is_ok()
            && match (&self.protocol.decision, self.shape) {
                (
                    hiroute_domain::DecisionDefinitionV1::Ordinal { .. },
                    CCapabilityShapeV1::Standard,
                ) => self.protocol.scopes.len() == 1,
                (hiroute_domain::DecisionDefinitionV1::Categorical { options, .. }, shape) => {
                    (2..=16).contains(&options.len())
                        && options
                            .iter()
                            .all(|o| self.protocol.scopes.contains_key(&o.id))
                        && (shape != CCapabilityShapeV1::MaxBranches
                            || options.len() == 16
                                && options.iter().all(|o| o.refinement.is_some()))
                }
                _ => false,
            }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClassifierDecisionTestTargetV2 {
    Value {
        classifier: hiroute_domain::ComplexityClassifierModeV1,
    },
    Saved {
        service_id: String,
        service_revision: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol: Option<FrozenNativeProtocolV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shape: Option<CCapabilityShapeV1>,
    },
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifierDecisionTestRequestV2 {
    pub schema: String,
    pub target: ClassifierDecisionTestTargetV2,
}
impl ClassifierDecisionTestRequestV2 {
    pub fn value(classifier: hiroute_domain::ComplexityClassifierModeV1) -> Self {
        Self {
            schema: CLASSIFIER_DECISION_TEST_SCHEMA_V2.into(),
            target: ClassifierDecisionTestTargetV2::Value { classifier },
        }
    }
    pub fn capability(&self) -> Option<ClassifierCapabilityProbeV1> {
        match &self.target {
            ClassifierDecisionTestTargetV2::Saved {
                service_id,
                service_revision,
                protocol: Some(protocol),
                shape: Some(shape),
            } => Some(ClassifierCapabilityProbeV1 {
                service_id: service_id.clone(),
                service_revision: *service_revision,
                protocol: protocol.clone(),
                shape: *shape,
            }),
            _ => None,
        }
    }
    pub fn validate(&self) -> bool {
        self.schema == CLASSIFIER_DECISION_TEST_SCHEMA_V2
            && match &self.target {
                ClassifierDecisionTestTargetV2::Value { classifier } => {
                    !matches!(
                        classifier,
                        hiroute_domain::ComplexityClassifierModeV1::LocalRules
                    ) && classifier.validate().is_ok()
                }
                ClassifierDecisionTestTargetV2::Saved {
                    service_id,
                    service_revision,
                    protocol,
                    shape,
                } => {
                    !service_id.is_empty()
                        && service_id.len() <= 128
                        && *service_revision > 0
                        && match (protocol, shape) {
                            (None, None) => true,
                            (Some(_), Some(_)) => self.capability().is_some_and(|p| p.validate()),
                            _ => false,
                        }
                }
            }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifierDecisionTestResultV2 {
    pub schema: String,
    pub state: CCapabilityStateV1,
    pub verified_shapes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_digest: Option<CanonicalDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_digest: Option<CanonicalDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<CCapabilityShapeV1>,
    pub question_count: u16,
    pub tested_at_unix_ms: u64,
    pub duration_millis: u64,
    pub reason: String,
}
