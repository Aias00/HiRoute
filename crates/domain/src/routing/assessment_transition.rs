//! Transient protocol choices and server-composed Preview. No review lives in editor state.
use super::*;
use crate::CanonicalDigest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentRollbackOptionV1 {
    pub source: HistoricalAssessmentSourceV1,
    pub instructions: String,
    pub criteria: [String; 3],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentChoiceV1 {
    pub method: AssessmentAdoptionV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<HistoricalAssessmentSourceV1>,
}
pub type AssessmentChoicesV1 = BTreeMap<String, AssessmentChoiceV1>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentTransitionScopeV1 {
    pub scope: String,
    pub from_protocol: Option<String>,
    pub protocol: String,
    pub assessment: Value,
    pub assessment_digest: CanonicalDigest,
    pub available_methods: Vec<AssessmentAdoptionV1>,
    pub historical_sources: Vec<AssessmentRollbackOptionV1>,
    pub choice: Option<AssessmentChoiceV1>,
}
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentTransitionPreviewV1 {
    pub scopes: Vec<AssessmentTransitionScopeV1>,
}
impl AssessmentTransitionPreviewV1 {
    pub fn require_acknowledged(&self) -> Result<(), NativeProtocolError> {
        if let Some(s) = self.scopes.iter().find(|s| s.choice.is_none()) {
            return Err(review_error(&s.scope, "protocol transition is not acknowledged"));
        }
        Ok(())
    }
    pub fn records(&self) -> Option<AssessmentReviewsV1> {
        if self.scopes.is_empty() || self.require_acknowledged().is_err() { return None; }
        Some(self.scopes.iter().map(|s| {
            let choice = s.choice.as_ref().expect("acknowledged");
            (s.scope.clone(), AssessmentReviewV1 {
                from_protocol: s.from_protocol.clone(), protocol: s.protocol.clone(), assessment_digest: s.assessment_digest.clone(),
                method: choice.method, source: choice.source.clone(),
            })
        }).collect())
    }
}

impl AgentPlanAuthoringV2 {
    pub fn native_protocol(&self) -> Option<&NativeProtocolV1> {
        match &self.strategy {
            AgentPlanStrategyV2::SmartSaving { native_protocol, .. } => native_protocol.as_ref(),
            AgentPlanStrategyV2::Branches { routing } => routing.native_protocol.as_ref(),
            _ => None,
        }
    }
    pub fn assessment_scopes(&self) -> BTreeMap<String, &JudgmentSettingsV1> {
        match &self.strategy {
            AgentPlanStrategyV2::SmartSaving { judgment, .. } => BTreeMap::from([(SMART_SAVING_SCOPE_ID.into(), judgment)]),
            AgentPlanStrategyV2::Branches { routing } => {
                let mut scopes = BTreeMap::new();
                if routing.branches.iter().any(|b| b.judgment.is_none()) { scopes.insert("default".into(), &routing.judgment); }
                scopes.extend(routing.branches.iter().filter_map(|b| b.judgment.as_ref().map(|j| (b.id.clone(), j))));
                scopes
            }
            _ => BTreeMap::new(),
        }
    }
    fn native(&self) -> bool {
        match &self.strategy {
            AgentPlanStrategyV2::SmartSaving { classifier, .. } => native_mode(classifier),
            AgentPlanStrategyV2::Branches { routing } => native_mode(&routing.classifier),
            _ => false,
        }
    }
}
fn review_error(scope: &str, reason: &str) -> NativeProtocolError {
    NativeProtocolError::Review { scope: scope.into(), reason: reason.into() }
}

/// Obtain scopes before protocol-specific text validation, so choosing stock can repair old text.
fn inspection(editor: &PlanEditorStateV2) -> Result<AgentPlanAuthoringV2, NativeProtocolError> {
    let mut value = editor.clone();
    let smart = value.smart.native_protocol.take();
    let branches = value.branch_routing.as_mut().and_then(|r| r.native_protocol.take());
    let mut c = value.effective().map_err(|_| NativeProtocolError::ContentMismatch)?;
    match &mut c.strategy {
        AgentPlanStrategyV2::SmartSaving { native_protocol, classifier, .. } if native_mode(classifier) => *native_protocol = smart,
        AgentPlanStrategyV2::Branches { routing } if native_mode(&routing.classifier) => routing.native_protocol = branches,
        _ => {}
    }
    Ok(c)
}
fn set_assessment(editor: &mut PlanEditorStateV2, scope: &str, judgment: &JudgmentSettingsV1) -> Result<(), NativeProtocolError> {
    let destination = match editor.mode {
        PlanEditorMode::SmartSaving if scope == SMART_SAVING_SCOPE_ID => &mut editor.smart.judgment,
        PlanEditorMode::CustomBranches => {
            let routing = editor.branch_routing.as_mut().ok_or_else(|| review_error(scope, "missing branch settings"))?;
            if scope == "default" { &mut routing.judgment } else {
                routing.branches.iter_mut().find(|b| b.id == scope).and_then(|b| b.judgment.as_mut()).ok_or_else(|| review_error(scope, "missing independent scope"))?
            }
        }
        _ => return Err(review_error(scope, "unknown assessment scope")),
    };
    destination.competence.instructions = judgment.competence.instructions.clone();
    destination.competence.criteria = judgment.competence.criteria.clone();
    Ok(())
}

pub fn preview_assessment_transition(
    editor: &PlanEditorStateV2,
    previous: Option<&AgentPlanAuthoringV2>,
    draft: Option<&PlanEditorStateV2>,
    sources: &[AssessmentRollbackOptionV1],
    choices: &AssessmentChoicesV1,
) -> Result<(PlanEditorStateV2, AssessmentTransitionPreviewV1), NativeProtocolError> {
    let configuration = inspection(editor)?;
    if !configuration.native() {
        if let Some(scope) = choices.keys().next() { return Err(review_error(scope, "destination has no native assessment")); }
        return Ok((editor.clone(), AssessmentTransitionPreviewV1::default()));
    }
    let protocol = configuration.native_protocol();
    let destination = protocol.map_or(SYSTEM_ONE_LEGACY, |p| p.schema.as_str());
    let parked_c = draft.is_some_and(|d| d.smart.native_protocol.is_some() || d.branch_routing.as_ref().is_some_and(|r| r.native_protocol.is_some()));
    let previous_scopes = previous.map(AgentPlanAuthoringV2::assessment_scopes).unwrap_or_default();
    let mut resolved = editor.clone();
    let mut preview = AssessmentTransitionPreviewV1::default();
    for (scope, judgment) in configuration.assessment_scopes() {
        let old_protocol = previous.filter(|p| p.native() && previous_scopes.contains_key(&scope)).map(|p| p.native_protocol().map_or(SYSTEM_ONE_LEGACY, |p| p.schema.as_str()));
        let changed = if protocol.is_some() { old_protocol != Some(destination) } else { old_protocol == Some(SYSTEM_ONE_C_V1) || parked_c };
        if !changed {
            if choices.contains_key(&scope) { return Err(review_error(&scope, "scope has no protocol transition")); }
            let mut stock = judgment.clone(); NativeProtocolV1::default().stock_assessment(&mut stock);
            if protocol.is_none() && judgment.competence.instructions == stock.competence.instructions
                && previous_scopes.get(&scope).is_none_or(|j| j.competence.instructions != judgment.competence.instructions || j.competence.criteria != judgment.competence.criteria) {
                return Err(review_error(&scope, "applicability assessment text requires an explicit protocol transition"));
            }
            continue;
        }
        let historical: Vec<_> = sources.iter().filter(|s| protocol.is_none() && s.source.scope == scope).cloned().collect();
        let mut available_methods = vec![AssessmentAdoptionV1::Stock, AssessmentAdoptionV1::ReviewedCustom];
        if !historical.is_empty() { available_methods.push(AssessmentAdoptionV1::Historical); }
        let choice = choices.get(&scope);
        let mut effective = judgment.clone();
        if let Some(choice) = choice {
            if (choice.method == AssessmentAdoptionV1::Historical) != choice.source.is_some() { return Err(review_error(&scope, "historical choice requires exactly one pinned source")); }
            match choice.method {
                AssessmentAdoptionV1::Stock => {
                    let mut stock = JudgmentSettingsV1::default();
                    if let Some(p) = protocol { p.stock_assessment(&mut stock); }
                    effective.competence.instructions = stock.competence.instructions;
                    effective.competence.criteria = stock.competence.criteria;
                }
                AssessmentAdoptionV1::ReviewedCustom => {}
                AssessmentAdoptionV1::Historical => {
                    let source = historical.iter().find(|s| Some(&s.source) == choice.source.as_ref()).ok_or_else(|| review_error(&scope, "historical source is stale, reclaimed or unavailable"))?;
                    effective.competence.instructions = source.instructions.clone();
                    effective.competence.criteria = source.criteria.clone();
                }
            }
            set_assessment(&mut resolved, &scope, &effective)?;
        }
        let review = AssessmentReviewV1::new(protocol, &effective, AssessmentAdoptionV1::ReviewedCustom, None)?;
        preview.scopes.push(AssessmentTransitionScopeV1 { scope, from_protocol: old_protocol.map(str::to_owned), protocol: destination.into(), assessment: composed_assessment(protocol, &effective), assessment_digest: review.assessment_digest, available_methods, historical_sources: historical, choice: choice.cloned() });
    }
    if let Some(scope) = choices.keys().find(|scope| !preview.scopes.iter().any(|s| &s.scope == *scope)) { return Err(review_error(scope, "unknown or unchanged assessment scope")); }
    Ok((resolved, preview))
}

