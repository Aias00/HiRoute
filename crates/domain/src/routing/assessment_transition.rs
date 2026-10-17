//! Assessment-only adoption/rollback. No historical version is held or read by execution.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentRollbackOptionV1 {
    pub source: HistoricalAssessmentSourceV1,
    pub instructions: String,
    pub criteria: [String; 3],
}

impl AgentPlanAuthoringV2 {
    pub fn native_protocol(&self) -> Option<&NativeProtocolV1> {
        match &self.strategy {
            AgentPlanStrategyV2::SmartSaving {
                native_protocol, ..
            } => native_protocol.as_ref(),
            AgentPlanStrategyV2::Branches { routing } => routing.native_protocol.as_ref(),
            _ => None,
        }
    }
    pub fn assessment_reviews(&self) -> Option<&AssessmentReviewsV1> {
        match &self.strategy {
            AgentPlanStrategyV2::SmartSaving {
                assessment_review, ..
            } => assessment_review.as_ref(),
            AgentPlanStrategyV2::Branches { routing } => routing.assessment_review.as_ref(),
            _ => None,
        }
    }
    pub fn assessment_scopes(&self) -> BTreeMap<String, &JudgmentSettingsV1> {
        match &self.strategy {
            AgentPlanStrategyV2::SmartSaving { judgment, .. } => {
                BTreeMap::from([(SMART_SAVING_SCOPE_ID.into(), judgment)])
            }
            AgentPlanStrategyV2::Branches { routing } => {
                let mut scopes = BTreeMap::new();
                if routing.branches.iter().any(|b| b.judgment.is_none()) {
                    scopes.insert("default".into(), &routing.judgment);
                }
                scopes.extend(
                    routing
                        .branches
                        .iter()
                        .map(|b| (b.id.clone(), routing.judgment_for(b))),
                );
                scopes
            }
            _ => BTreeMap::new(),
        }
    }
    pub fn review_for_scope(&self, scope: &str) -> Option<&AssessmentReviewV1> {
        let reviews = self.assessment_reviews()?;
        reviews.get(scope).or_else(|| match &self.strategy {
            AgentPlanStrategyV2::Branches { routing }
                if routing
                    .branches
                    .iter()
                    .any(|b| b.id == scope && b.judgment.is_none()) =>
            {
                reviews.get("default")
            }
            _ => None,
        })
    }
}

pub fn validate_assessment_transition(
    configuration: &AgentPlanAuthoringV2,
    previous: Option<&AgentPlanAuthoringV2>,
    draft: Option<&PlanEditorStateV2>,
    sources: &[AssessmentRollbackOptionV1],
) -> Result<(), NativeProtocolError> {
    let native = match &configuration.strategy {
        AgentPlanStrategyV2::SmartSaving { classifier, .. } => native_mode(classifier),
        AgentPlanStrategyV2::Branches { routing } => native_mode(&routing.classifier),
        _ => false,
    };
    if !native {
        return Ok(());
    }
    let old_c = previous.is_some_and(|c| c.native_protocol().is_some())
        || draft.is_some_and(|d| match configuration.mode {
            PlanEditorMode::SmartSaving => d.smart.native_protocol.is_some(),
            PlanEditorMode::CustomBranches => d
                .branch_routing
                .as_ref()
                .is_some_and(|r| r.native_protocol.is_some()),
            _ => false,
        });
    let destination = configuration.native_protocol();
    for (scope, judgment) in configuration.assessment_scopes() {
        let review = configuration.review_for_scope(&scope);
        let mut stock_c = JudgmentSettingsV1::default();
        NativeProtocolV1::default().stock_assessment(&mut stock_c);
        let dangling_c = destination.is_none()
            && review.is_none()
            && judgment.competence.instructions == stock_c.competence.instructions
            && previous
                .and_then(|p| p.assessment_scopes().get(&scope).copied())
                .is_none_or(|j| {
                    j.competence.instructions != judgment.competence.instructions
                        || j.competence.criteria != judgment.competence.criteria
                });
        if destination.is_some() || old_c || dangling_c {
            review
                .ok_or(NativeProtocolError::Review)?
                .validate(destination, judgment)?;
        }
        if let Some(review) = review {
            review.validate(destination, judgment)?;
            if let Some(source) = &review.source {
                // A copied, already-published rubric retains its trusted review even after
                // the old source is reclaimed. Only a newly selected restoration reads it.
                if previous.and_then(|c| c.review_for_scope(&scope)) == Some(review) {
                    continue;
                }
                let inherited_review = configuration
                    .assessment_reviews()
                    .is_some_and(|r| !r.contains_key(&scope));
                if source.scope != if inherited_review { "default" } else { &scope } {
                    return Err(NativeProtocolError::HistoricalUnavailable);
                }
                let source = sources
                    .iter()
                    .find(|s| &s.source == source)
                    .ok_or(NativeProtocolError::HistoricalUnavailable)?;
                if source.instructions != judgment.competence.instructions
                    || source.criteria != judgment.competence.criteria
                {
                    return Err(NativeProtocolError::HistoricalUnavailable);
                }
            }
        }
    }
    Ok(())
}
