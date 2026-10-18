//! The editable capability option is distinct from a sealed protocol selection.
use super::*;
use crate::{BranchRoutingV1, HistoricalApplicabilityV1, JudgmentSettingsV1, RouteBranchV1};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BranchRoutingEditorV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub historical_applicability: Option<HistoricalApplicabilityV1>,
    pub classifier: ComplexityClassifierModeV1,
    pub branches: Vec<RouteBranchV1>,
    pub default_branch_id: String,
    pub judgment: JudgmentSettingsV1,
    /// Retained for serialized compatibility only; candidate continuity is unconditional.
    #[serde(default)]
    pub reselect_on_user_message: bool,
}
impl BranchRoutingEditorV1 {
    pub(super) fn configuration(&self) -> BranchRoutingV1 {
        BranchRoutingV1 {
            native_protocol: self
                .historical_applicability
                .as_ref()
                .map(HistoricalApplicabilityV1::selection),
            classifier: self.classifier.clone(),
            branches: self.branches.clone(),
            default_branch_id: self.default_branch_id.clone(),
            judgment: self.judgment.clone(),
            reselect_on_user_message: self.reselect_on_user_message,
        }
    }
}
impl From<&BranchRoutingV1> for BranchRoutingEditorV1 {
    fn from(value: &BranchRoutingV1) -> Self {
        Self {
            historical_applicability: value.native_protocol.as_ref().map(Into::into),
            classifier: value.classifier.clone(),
            branches: value.branches.clone(),
            default_branch_id: value.default_branch_id.clone(),
            judgment: value.judgment.clone(),
            reselect_on_user_message: value.reselect_on_user_message,
        }
    }
}
