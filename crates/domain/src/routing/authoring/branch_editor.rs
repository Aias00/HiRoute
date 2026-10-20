//! Authoring has one native default; protocol identity belongs to sealed configuration.
use super::*;
use crate::{BranchRoutingV1, JudgmentSettingsV1, NativeProtocolSelectionV1, RouteBranchV1};

#[derive(schemars::JsonSchema, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BranchRoutingEditorV1 {
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
            native_protocol: super::super::native_mode(&self.classifier)
                .then(NativeProtocolSelectionV1::default),
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
            classifier: value.classifier.clone(),
            branches: value.branches.clone(),
            default_branch_id: value.default_branch_id.clone(),
            judgment: value.judgment.clone(),
            reselect_on_user_message: value.reselect_on_user_message,
        }
    }
}
