//! Opt-in projections of retained route-decision evidence. Rates are descriptive, not accuracy.
use crate::{CanonicalDigest, ExecutionGroupV1, ModelGroupReasonV1, RoutingObservationV1};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationRoutingDecisionV1 {
    pub plan_id: String,
    pub plan_revision: u64,
    pub inherited: bool,
    pub selected_branch_id: String,
    pub execution_group: ExecutionGroupV1,
    pub selection_reason: ModelGroupReasonV1,
    pub observation: RoutingObservationV1,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingObservationCohortV1 {
    pub protocol: String,
    pub content_digest: CanonicalDigest,
    pub bundle_digest: CanonicalDigest,
    pub scope_id: String,
    pub standard: String,
    pub provider: String,
    pub model: String,
    pub simple_threshold_millis: u16,
    pub competence_floor_millis: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingObservationCountsV1 {
    /// Fresh C decisions only; inherited decisions are counted separately.
    pub c_decisions: u64,
    pub requested: u64,
    pub unavailable: u64,
    pub no_target_omissions: u64,
    pub partial_target_omissions: u64,
    pub inherited: u64,
    pub required_question_missing: u64,
    pub gate_opportunities: u64,
    pub suppressed: u64,
    pub unavailable_per_requested: Option<f64>,
    pub suppressed_per_opportunity: Option<f64>,
    pub suppressed_per_c_decision: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingObservationCohortSummaryV1 {
    pub cohort: RoutingObservationCohortV1,
    pub counts: RoutingObservationCountsV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingObservationsSummaryV1 {
    pub schema: String,
    pub plan_id: Option<String>,
    pub session_id: Option<String>,
    pub plan_revision: Option<u64>,
    pub requested_from_ms: Option<i64>,
    pub requested_to_ms: Option<i64>,
    pub covered_from_ms: i64,
    pub covered_to_ms: i64,
    pub coverage: String,
    pub retained_decisions: u64,
    pub legacy_or_unknown_decisions: u64,
    pub unknown_scope_decisions: u64,
    pub missing_projection_events: u64,
    pub scan_limited: bool,
    pub cohorts: Vec<RoutingObservationCohortSummaryV1>,
}
