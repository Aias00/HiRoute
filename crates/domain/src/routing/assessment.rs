//! Assessment authoring intent and the bounded released-input adapter.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentStandardV1 {
    Default,
    Custom,
}

/// User-facing capability option. Protocol identity is supplied by HiRoute.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalApplicabilityV1 {
    pub prior_context_max_turns: u8,
}
impl Default for HistoricalApplicabilityV1 {
    fn default() -> Self {
        Self {
            prior_context_max_turns: 8,
        }
    }
}
impl HistoricalApplicabilityV1 {
    pub fn selection(&self) -> NativeProtocolSelectionV1 {
        NativeProtocolSelectionV1 {
            prior_context_max_turns: self.prior_context_max_turns,
            ..Default::default()
        }
    }
}
impl From<&NativeProtocolSelectionV1> for HistoricalApplicabilityV1 {
    fn from(value: &NativeProtocolSelectionV1) -> Self {
        Self {
            prior_context_max_turns: value.prior_context_max_turns,
        }
    }
}

/// Internal selection retained in effective authoring and sealed Plan configuration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProtocolSelectionV1 {
    pub schema: String,
    pub prior_context_max_turns: u8,
}
impl Default for NativeProtocolSelectionV1 {
    fn default() -> Self {
        Self {
            schema: SYSTEM_ONE_C_V1.into(),
            prior_context_max_turns: 8,
        }
    }
}
impl NativeProtocolSelectionV1 {
    pub fn resolve(&self) -> Result<NativeProtocolV1, NativeProtocolError> {
        let value = NativeProtocolV1 {
            schema: self.schema.clone(),
            prior_context_max_turns: self.prior_context_max_turns,
            ..Default::default()
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), NativeProtocolError> {
        self.resolve().map(|_| ())
    }
}

impl JudgmentSettingsV1 {
    /// Missing intent is supported released input, not a recurring equality heuristic.
    pub fn assessment_standard(&self) -> AssessmentStandardV1 {
        self.competence.standard.unwrap_or_else(|| {
            if self.competence.instructions == DEFAULT_COMPETENCE_INSTRUCTIONS
                && self.competence.criteria == DEFAULT_COMPETENCE_CRITERIA.map(str::to_owned)
            {
                AssessmentStandardV1::Default
            } else {
                AssessmentStandardV1::Custom
            }
        })
    }
    pub fn resolve_assessment(
        &self,
        protocol: Option<&NativeProtocolSelectionV1>,
    ) -> Result<Self, NativeProtocolError> {
        let resolved = protocol
            .map(NativeProtocolSelectionV1::resolve)
            .transpose()?;
        let mut value = self.clone();
        let standard = self.assessment_standard();
        if standard == AssessmentStandardV1::Default {
            let floor = value.competence.floor_millis;
            if let Some(p) = &resolved {
                p.stock_assessment(&mut value);
            } else {
                value.competence = Default::default();
            }
            value.competence.floor_millis = floor;
        }
        // Preserve released omission for untouched legacy/custom-extension paths.
        value.competence.standard =
            (protocol.is_some() || self.competence.standard.is_some()).then_some(standard);
        Ok(value)
    }
}

impl AgentPlanAuthoringV2 {
    pub fn native_protocol(&self) -> Option<&NativeProtocolSelectionV1> {
        match &self.strategy {
            AgentPlanStrategyV2::SmartSaving {
                native_protocol, ..
            } => native_protocol.as_ref(),
            AgentPlanStrategyV2::Branches { routing } => routing.native_protocol.as_ref(),
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
                    routing.branches.iter().filter_map(|b| {
                        b.judgment.as_ref().map(|j| (format!("branch/{}", b.id), j))
                    }),
                );
                scopes
            }
            _ => BTreeMap::new(),
        }
    }
}
