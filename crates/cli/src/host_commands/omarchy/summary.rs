//! `hiroute.omarchy-summary/v1`: one bounded read for the desktop indicator. Every figure is
//! current or absent; an unreadable fact makes the whole summary unavailable.
use super::*;
use hiroute_application_api::{
    AgentClaudePresetSelectionV2, AgentModelDefaultSelectionV2, AgentModelSelectionV2, AgentPlanId,
    ObservationTokenShareHeadlineV1, PlanTrustStateV1, TrustLevelV1,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

pub(super) const SCHEMA: &str = "hiroute.omarchy-summary/v1";
const MAX_IDS: usize = 20;
/// Attribution checks are exact-evidence reads. Only the newest few candidates are tried.
const MAX_ATTRIBUTION_CHECKS: usize = 8;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Summary {
    pub schema: String,
    pub generated_at_ms: i64,
    pub service: Service,
    pub agents: Vec<Agent>,
    pub suggestions: Option<Suggestions>,
    pub latest_routed_session: Option<LatestSession>,
    pub token_share_today: Option<ObservationTokenShareHeadlineV1>,
    /// The local notification preference, which the panel toggles. It needs no service.
    pub notifications: bool,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Service {
    pub state: ServiceState,
    pub reason_code: Option<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ServiceState {
    Ready,
    Unavailable,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Agent {
    pub agent: String,
    pub launchable: bool,
    pub check_reason_code: Option<String>,
    pub slot_plans: Vec<SlotPlan>,
    pub other_mapped_plans: u32,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SlotPlan {
    pub plan_id: String,
    pub plan_name: String,
    pub trust_state: TrustState,
    pub trust_level: Option<String>,
    pub threshold_millis: Option<u16>,
    pub slots: Vec<Slot>,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TrustState {
    Uniform,
    NotApplicable,
    BranchSpecific,
}
/// Start slots in presentation order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Slot {
    Default,
    Opus,
    Sonnet,
    Haiku,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Suggestions {
    pub count: u32,
    pub ids: Vec<String>,
    /// Mapped Plans whose current suggestions could not be read. `count` covers the rest.
    pub unavailable_plans: u32,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LatestSession {
    pub session_id: String,
    pub agent: String,
    pub plan_id: String,
    pub last_activity_ms: i64,
}

pub(super) fn unavailable(generated_at_ms: i64, reason: &str) -> Summary {
    Summary {
        schema: SCHEMA.into(),
        generated_at_ms,
        service: Service {
            state: ServiceState::Unavailable,
            reason_code: Some(reason.into()),
        },
        agents: Vec::new(),
        suggestions: None,
        latest_routed_session: None,
        token_share_today: None,
        notifications: false,
    }
}

/// The Plans behind an agent's start slots, in Opus, Sonnet, Haiku order. A slot that keeps
/// the native model or names a fixed model is not a Plan and is omitted.
pub(super) fn start_slots(selection: &AgentModelSelectionV2) -> Vec<(Slot, String)> {
    match selection {
        AgentModelSelectionV2::CodexDefault {
            default_selection: AgentModelDefaultSelectionV2::Plan { plan_id },
            ..
        } => vec![(Slot::Default, plan_id.as_str().to_owned())],
        AgentModelSelectionV2::ClaudeLauncher {
            preset_mappings, ..
        } => [
            (Slot::Opus, &preset_mappings.opus),
            (Slot::Sonnet, &preset_mappings.sonnet),
            (Slot::Haiku, &preset_mappings.haiku),
        ]
        .into_iter()
        .filter_map(|(slot, selection)| match selection {
            AgentClaudePresetSelectionV2::Plan { plan_id } => {
                Some((slot, plan_id.as_str().to_owned()))
            }
            AgentClaudePresetSelectionV2::PreserveNative => None,
        })
        .collect(),
        _ => Vec::new(),
    }
}

/// Facts already read for one routed agent.
pub(super) struct AgentFacts {
    pub check: reads::Check,
    pub selection: Option<AgentModelSelectionV2>,
}
pub(super) struct Facts {
    pub agents: Vec<AgentFacts>,
    pub plans: BTreeMap<String, reads::PlanFacts>,
    pub suggestions: reads::SuggestionRead,
    pub latest: Option<LatestSession>,
    pub token_share_today: ObservationTokenShareHeadlineV1,
}
pub(super) fn compose(facts: Facts, generated_at_ms: i64) -> Option<Summary> {
    let mut mapped = BTreeSet::new();
    let mut agents = Vec::new();
    for agent in facts.agents {
        let (slots, allowed) = agent.selection.as_ref().map_or_else(
            || (Vec::new(), BTreeSet::new()),
            |s| {
                (
                    start_slots(s),
                    s.allowed_plan_ids()
                        .into_iter()
                        .map(|p| p.as_str().to_owned())
                        .collect::<BTreeSet<_>>(),
                )
            },
        );
        let mut slot_plans: Vec<SlotPlan> = Vec::new();
        for (slot, plan_id) in slots {
            if let Some(entry) = slot_plans.iter_mut().find(|p| p.plan_id == plan_id) {
                entry.slots.push(slot);
                continue;
            }
            // A start slot that names a Plan must be described, never guessed.
            let plan = facts.plans.get(&plan_id)?;
            let (trust_state, trust_level, threshold_millis) = match &plan.trust {
                PlanTrustStateV1::Uniform {
                    level,
                    threshold_millis,
                } => (
                    TrustState::Uniform,
                    Some(level.clone()),
                    Some(*threshold_millis),
                ),
                PlanTrustStateV1::NotApplicable => (TrustState::NotApplicable, None, None),
                PlanTrustStateV1::BranchSpecific => (TrustState::BranchSpecific, None, None),
            };
            slot_plans.push(SlotPlan {
                plan_id,
                plan_name: plan.name.clone(),
                trust_state,
                trust_level,
                threshold_millis,
                slots: vec![slot],
            });
        }
        let in_slots: BTreeSet<_> = slot_plans.iter().map(|p| p.plan_id.clone()).collect();
        let other = allowed.difference(&in_slots).count();
        mapped.extend(allowed);
        mapped.extend(in_slots);
        agents.push(Agent {
            agent: agent.check.agent,
            launchable: agent.check.launchable,
            check_reason_code: agent.check.check_reason_code,
            slot_plans,
            other_mapped_plans: u32::try_from(other).ok()?,
        });
    }
    let ids: Vec<_> = facts
        .suggestions
        .ids
        .iter()
        .filter(|(_, plan)| mapped.contains(*plan))
        .map(|(id, _)| id.clone())
        .collect();
    let summary = Summary {
        schema: SCHEMA.into(),
        generated_at_ms,
        service: Service {
            state: ServiceState::Ready,
            reason_code: None,
        },
        agents,
        suggestions: Some(Suggestions {
            count: u32::try_from(ids.len()).ok()?,
            ids: ids.into_iter().take(MAX_IDS).collect(),
            unavailable_plans: u32::try_from(
                facts.suggestions.unavailable.intersection(&mapped).count(),
            )
            .ok()?,
        }),
        latest_routed_session: facts.latest,
        token_share_today: Some(facts.token_share_today),
        notifications: false,
    };
    summary.validate().then_some(summary)
}

impl Summary {
    pub fn validate(&self) -> bool {
        if self.schema != SCHEMA || self.generated_at_ms <= 0 {
            return false;
        }
        match self.service.state {
            ServiceState::Unavailable => {
                self.service.reason_code.as_deref().is_some_and(reason_code)
                    && self.agents.is_empty()
                    && self.suggestions.is_none()
                    && self.latest_routed_session.is_none()
                    && self.token_share_today.is_none()
            }
            ServiceState::Ready => {
                self.service.reason_code.is_none()
                    && self.agents.len() <= 2
                    && self
                        .agents
                        .iter()
                        .map(|a| a.agent.as_str())
                        .collect::<BTreeSet<_>>()
                        .len()
                        == self.agents.len()
                    && self.agents.iter().all(Agent::validate)
                    && self.suggestions.as_ref().is_some_and(Suggestions::validate)
                    && self.latest_routed_session.as_ref().is_none_or(|s| {
                        s.validate(self.generated_at_ms)
                            && self.agents.iter().any(|a| a.agent == s.agent)
                    })
                    && self.token_share_today.as_ref().is_some_and(valid_headline)
            }
        }
    }
}
impl Agent {
    fn validate(&self) -> bool {
        let (max, allowed): (usize, &[Slot]) = match self.agent.as_str() {
            "codex" => (1, &[Slot::Default][..]),
            "claude-code" => (3, &[Slot::Opus, Slot::Sonnet, Slot::Haiku][..]),
            _ => return false,
        };
        let slots: Vec<Slot> = self
            .slot_plans
            .iter()
            .flat_map(|p| p.slots.iter().copied())
            .collect();
        let first: Vec<Slot> = self
            .slot_plans
            .iter()
            .filter_map(|p| p.slots.first().copied())
            .collect();
        self.launchable == self.check_reason_code.is_none()
            && self.check_reason_code.as_deref().is_none_or(reason_code)
            && self.slot_plans.len() <= max
            && slots.iter().all(|s| allowed.contains(s))
            && slots.iter().collect::<BTreeSet<_>>().len() == slots.len()
            && first.len() == self.slot_plans.len()
            && first.windows(2).all(|pair| pair[0] < pair[1])
            && self
                .slot_plans
                .iter()
                .all(|p| p.slots.windows(2).all(|pair| pair[0] < pair[1]))
            && self
                .slot_plans
                .iter()
                .map(|p| p.plan_id.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                == self.slot_plans.len()
            && self.slot_plans.iter().all(SlotPlan::validate)
            && self.other_mapped_plans <= 4096
    }
}
impl SlotPlan {
    fn validate(&self) -> bool {
        let trust = match (
            self.trust_state,
            self.trust_level.as_deref(),
            self.threshold_millis,
        ) {
            (TrustState::Uniform, Some("custom"), Some(threshold)) => {
                threshold <= 1000 && TrustLevelV1::from_threshold(threshold).is_none()
            }
            (TrustState::Uniform, Some(level), Some(threshold)) => {
                TrustLevelV1::from_threshold(threshold).is_some_and(|l| l.id == level)
            }
            (TrustState::NotApplicable | TrustState::BranchSpecific, None, None) => true,
            _ => false,
        };
        trust
            && AgentPlanId::parse(&self.plan_id).is_ok()
            && !self.plan_name.is_empty()
            && self.plan_name.chars().count() <= 80
            && !self.plan_name.chars().any(char::is_control)
            && !self.slots.is_empty()
    }
}
impl Suggestions {
    fn validate(&self) -> bool {
        self.ids.len() <= MAX_IDS
            && self.ids.len()
                == usize::try_from(self.count)
                    .unwrap_or(usize::MAX)
                    .min(MAX_IDS)
            && self.ids.iter().collect::<BTreeSet<_>>().len() == self.ids.len()
            && self.ids.iter().all(|id| reads::valid_suggestion_id(id))
            && self.unavailable_plans <= 4096
    }
}
impl LatestSession {
    fn validate(&self, generated_at_ms: i64) -> bool {
        matches!(self.agent.as_str(), "codex" | "claude-code")
            && !self.session_id.is_empty()
            && self.session_id.len() <= 256
            && !self.session_id.contains(char::is_control)
            && AgentPlanId::parse(&self.plan_id).is_ok()
            && self.last_activity_ms >= generated_at_ms - reads::LATEST_WINDOW_MS
            && self.last_activity_ms <= generated_at_ms
    }
}
fn reason_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 80
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn valid_headline(headline: &ObservationTokenShareHeadlineV1) -> bool {
    match headline {
        ObservationTokenShareHeadlineV1::Share {
            outside_subscription_ratio_millis,
            ..
        } => *outside_subscription_ratio_millis <= 1000,
        _ => true,
    }
}

pub(super) fn read(context: &Context, globals: &Globals) -> Summary {
    let mut summary = read_service(context, globals);
    summary.notifications = context
        .load()
        .ok()
        .flatten()
        .is_some_and(|manifest| manifest.notifications);
    summary
}
/// Read every service fact under one deadline. Launch checks run beside the service reads.
fn read_service(context: &Context, globals: &Globals) -> Summary {
    let generated_at_ms = now_ms();
    let Ok(query) = reads::Query::new(globals.timeout.unwrap_or(Duration::from_secs(5))) else {
        return unavailable(generated_at_ms, "summary_read_unavailable");
    };
    if !reads::service_ready(&query) {
        return unavailable(generated_at_ms, "service_unavailable");
    }
    let Ok(connections) = reads::connections(&query) else {
        return unavailable(generated_at_ms, "summary_read_unavailable");
    };
    let program = context.stable_cli();
    let composed = std::thread::scope(|scope| {
        let checks: Vec<_> = connections
            .iter()
            .map(|c| {
                let (program, query) = (&program, &query);
                scope.spawn(move || reads::check(program, &c.agent, query.remaining()))
            })
            .collect();
        let facts = service_facts(&query, &connections, generated_at_ms);
        let checks: Option<Vec<_>> = checks.into_iter().map(|h| h.join().ok()).collect();
        let (plans, suggestions, latest, token_share_today) = facts.ok()?;
        let agents = checks?
            .into_iter()
            .zip(&connections)
            .map(|(check, c)| AgentFacts {
                check,
                selection: c.selection.clone(),
            })
            .collect();
        compose(
            Facts {
                agents,
                plans,
                suggestions,
                latest,
                token_share_today,
            },
            generated_at_ms,
        )
    });
    composed.unwrap_or_else(|| unavailable(generated_at_ms, "summary_read_unavailable"))
}
type ServiceFacts = (
    BTreeMap<String, reads::PlanFacts>,
    reads::SuggestionRead,
    Option<LatestSession>,
    ObservationTokenShareHeadlineV1,
);
fn service_facts(
    query: &reads::Query,
    connections: &[reads::Connection],
    now: i64,
) -> Result<ServiceFacts, Failure> {
    let mut plans = BTreeMap::new();
    for plan in connections
        .iter()
        .filter_map(|c| c.selection.as_ref())
        .flat_map(start_slots)
        .map(|(_, plan)| plan)
    {
        if let Entry::Vacant(entry) = plans.entry(plan) {
            let facts = reads::plan(query, entry.key())?;
            entry.insert(facts);
        }
    }
    let mapped = reads::mapped_plans(connections);
    let suggestions = reads::suggestion_read(query, &mapped);
    let latest = latest_routed_session(query, connections, now)?;
    let token = reads::token_share_today(query)?;
    Ok((plans, suggestions, latest, token))
}
/// The newest session in the window served by a mapped Plan and verified as this agent's.
fn latest_routed_session(
    query: &reads::Query,
    connections: &[reads::Connection],
    now: i64,
) -> Result<Option<LatestSession>, Failure> {
    let routed: Vec<_> = connections
        .iter()
        .filter_map(|c| {
            let plans: BTreeSet<String> = c
                .selection
                .as_ref()?
                .allowed_plan_ids()
                .into_iter()
                .map(|p| p.as_str().to_owned())
                .collect();
            Some((c.agent.as_str(), c.context.as_deref()?, plans))
        })
        .collect();
    let plans: BTreeSet<String> = routed.iter().flat_map(|(_, _, p)| p.clone()).collect();
    if plans.is_empty() {
        return Ok(None);
    }
    let candidates = reads::session_candidates(query, &plans, now)?;
    choose_latest(&candidates, &routed, |context, candidate| {
        reads::attributed(query, context, candidate)
    })
}
pub(super) fn choose_latest(
    candidates: &[reads::SessionCandidate],
    routed: &[(&str, &str, BTreeSet<String>)],
    mut attributed: impl FnMut(&str, &reads::SessionCandidate) -> Result<bool, Failure>,
) -> Result<Option<LatestSession>, Failure> {
    let mut checks = 0;
    for candidate in candidates {
        for (agent, context, plans) in routed {
            if !plans.contains(&candidate.plan_id) {
                continue;
            }
            if checks == MAX_ATTRIBUTION_CHECKS {
                return Ok(None);
            }
            checks += 1;
            if attributed(context, candidate)? {
                return Ok(Some(LatestSession {
                    session_id: candidate.session_id.clone(),
                    agent: (*agent).into(),
                    plan_id: candidate.plan_id.clone(),
                    last_activity_ms: candidate.last_activity_ms,
                }));
            }
        }
    }
    Ok(None)
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(1)
}

#[cfg(test)]
#[path = "summary_tests.rs"]
mod tests;
