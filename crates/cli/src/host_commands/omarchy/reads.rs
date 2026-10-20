//! Bounded composition of public reads, shared by the host summary and notification frontends.
use super::*;
use hiroute_application_api::{
    AgentLaunchCheckV1, AgentModelSelectionV2, AgentPlanStatusV2, AgentSettingsStatusV2,
    ObservationSessionPageV2, ObservationTokenShareHeadlineV1, PlanTrustStateV1,
    RoutedLaunchReadV1, SystemStatusV1,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Query {
    deadline: Instant,
}
impl Query {
    pub fn new(timeout: Duration) -> Result<Self, Failure> {
        Ok(Self {
            deadline: Instant::now()
                .checked_add(timeout.min(Duration::from_secs(5)))
                .ok_or_else(invalid)?,
        })
    }
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
    pub fn call(
        &self,
        operation: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, Failure> {
        let remaining = self.remaining();
        if remaining.is_zero() {
            return Err((
                ErrorCode::DaemonUnavailable,
                "Integration read deadline elapsed.".into(),
            ));
        }
        let envelope = LocalControlClient::cli_from_environment()
            .map_err(|_| {
                (
                    ErrorCode::DaemonUnavailable,
                    "The local service is unavailable.".into(),
                )
            })?
            .with_timeout(remaining)
            .call(LocalControlWireRequestV2 {
                schema_version: LOCAL_CONTROL_SCHEMA_V2,
                request_id: crate::next_request_id(),
                operation_id: operation.into(),
                payload,
                protected_grant: None,
            })
            .map_err(|_| {
                (
                    ErrorCode::DaemonUnavailable,
                    "The local service read is unavailable.".into(),
                )
            })?;
        if envelope.status != MachineStatus::Succeeded || self.remaining().is_zero() {
            return Err((
                envelope
                    .error
                    .map_or(ErrorCode::DaemonUnavailable, |e| e.code),
                "The local service could not supply current integration data.".into(),
            ));
        }
        envelope.data.ok_or((
            ErrorCode::DaemonUnavailable,
            "The local service returned no integration data.".into(),
        ))
    }
}
pub(super) fn service_ready(query: &Query) -> bool {
    query
        .call("GetSystemStatus", json!({}))
        .ok()
        .and_then(|v| serde_json::from_value::<SystemStatusV1>(v).ok())
        .is_some_and(|s| {
            s.daemon == "role_all"
                && s.gateway == "ready"
                && s.control_store == "ready"
                && s.observation_store == "ready"
                && s.local_control.ends_with(":authenticated")
        })
}
pub(super) struct Connection {
    pub agent: String,
    /// Present only for the one connection a routed launch would use.
    pub context: Option<String>,
    pub selection: Option<AgentModelSelectionV2>,
}
pub(super) fn connections(query: &Query) -> Result<Vec<Connection>, Failure> {
    let mut result = Vec::new();
    for agent in ["codex", "claude-code"] {
        let read: RoutedLaunchReadV1 = serde_json::from_value(
            query.call("GetManagedAgentLaunchDescriptor", json!({"agent":agent}))?,
        )
        .map_err(|_| invalid())?;
        if read.check.candidate_contexts.is_empty() {
            continue;
        }
        if read.check.candidate_contexts.len() > 32 || read.check.agent.as_str() != agent {
            return Err(invalid());
        }
        let (context, selection) = if let [context] = read.check.candidate_contexts.as_slice() {
            let status: AgentSettingsStatusV2 = serde_json::from_value(query.call(
                "GetAgentConnectionStatus",
                json!({"schema_version":{"major":2,"minor":0},"context_id":context}),
            )?)
            .map_err(|_| invalid())?;
            if !status.valid() || status.context_id() != context {
                return Err(invalid());
            }
            (
                Some(context.clone()),
                status.model().and_then(|s| s.current_selection.clone()),
            )
        } else {
            (None, None)
        };
        result.push(Connection {
            agent: agent.into(),
            context,
            selection,
        });
    }
    Ok(result)
}
pub(super) fn mapped_plans(connections: &[Connection]) -> BTreeSet<String> {
    connections
        .iter()
        .flat_map(|c| {
            c.selection.iter().flat_map(|s| {
                s.allowed_plan_ids()
                    .into_iter()
                    .map(|p| p.as_str().to_owned())
            })
        })
        .collect()
}
pub(super) struct PlanFacts {
    pub name: String,
    pub trust: PlanTrustStateV1,
}
/// The current Plan version: the same configuration that trust evidence evaluates.
pub(super) fn plan(query: &Query, plan_id: &str) -> Result<PlanFacts, Failure> {
    let status: AgentPlanStatusV2 =
        serde_json::from_value(query.call("GetAgentPlanStatus", json!({"agent_plan_id":plan_id}))?)
            .map_err(|_| invalid())?;
    if status.agent_plan_id.as_str() != plan_id || status.desired.display_name.validate().is_err() {
        return Err(invalid());
    }
    Ok(PlanFacts {
        name: label(status.desired.display_name.as_str()),
        trust: status.desired.trust_state(),
    })
}
/// A display label of at most 80 characters. Plan names are already free of controls.
pub(super) fn label(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() <= 80 {
        clean
    } else {
        clean.chars().take(79).chain(['…']).collect()
    }
}
/// Current suggestions for the mapped Plans. A Plan whose evidence could not be read is
/// counted as unavailable, never as having no suggestion.
#[derive(Debug, Default, PartialEq)]
pub(super) struct SuggestionRead {
    /// Suggestion id to the first mapped Plan that carries it.
    pub ids: BTreeMap<String, String>,
    pub unavailable: BTreeSet<String>,
}
pub(super) fn suggestion_read(query: &Query, mapped: &BTreeSet<String>) -> SuggestionRead {
    if mapped.is_empty() {
        return SuggestionRead::default();
    }
    crate::trust::payload("observation.plan-trust.suggestions", &[])
        .map_err(|_| invalid())
        .and_then(|payload| query.call("GetPlanTrustSuggestions", payload))
        .ok()
        .and_then(|value| parse_suggestions(&value, mapped))
        .unwrap_or_else(|| SuggestionRead {
            ids: BTreeMap::new(),
            unavailable: mapped.clone(),
        })
}
pub(super) fn parse_suggestions(
    value: &serde_json::Value,
    mapped: &BTreeSet<String>,
) -> Option<SuggestionRead> {
    let mut read = SuggestionRead::default();
    for row in value.get("suggestions")?.as_array()? {
        let plan = row.get("plan_id")?.as_str()?;
        if !mapped.contains(plan) {
            continue;
        }
        for branch in row.get("branches")?.as_array()? {
            let id = branch
                .get("id")?
                .as_str()
                .filter(|id| valid_suggestion_id(id))?;
            read.ids.entry(id.to_owned()).or_insert_with(|| plan.into());
        }
    }
    for row in value.get("unavailable")?.as_array()? {
        let plan = row.get("plan_id")?.as_str()?;
        if mapped.contains(plan) {
            read.unavailable.insert(plan.into());
        }
    }
    Some(read)
}
/// Notification labels use the current Plan name. A suggestion whose Plan cannot be named
/// now is left for a later attempt rather than being sent with a substitute label.
pub(super) fn suggestions(
    query: &Query,
    connections: &[Connection],
) -> Result<Vec<host::notifications::SuggestionNotice>, Failure> {
    let read = suggestion_read(query, &mapped_plans(connections));
    let mut names = BTreeMap::new();
    let mut notices = Vec::new();
    for (id, plan) in read.ids {
        if !names.contains_key(&plan) {
            let name = plan_name(query, &plan);
            names.insert(plan.clone(), name);
        }
        if let Some(Some(name)) = names.get(&plan) {
            notices.push(host::notifications::SuggestionNotice {
                id,
                plan_name: name.clone(),
            });
        }
    }
    Ok(notices)
}
fn plan_name(query: &Query, plan: &str) -> Option<String> {
    plan(query, plan).ok().map(|facts| facts.name)
}
pub(super) const LATEST_WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1000;
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SessionCandidate {
    pub session_id: String,
    pub plan_id: String,
    pub last_activity_ms: i64,
}
/// Recent sessions served by each mapped Plan, newest first. Gateway sessions do not name
/// their Agent, so attribution is a separate step.
pub(super) fn session_candidates(
    query: &Query,
    plans: &BTreeSet<String>,
    now_ms: i64,
) -> Result<Vec<SessionCandidate>, Failure> {
    let mut candidates = Vec::new();
    for plan in plans {
        let page: ObservationSessionPageV2 = serde_json::from_value(query.call(
            "ListSessions",
            json!({"schema":"hiroute.observation.query/v2","intent":{"view":"sessions","query":{
                "from_ms": now_ms - LATEST_WINDOW_MS, "to_ms": now_ms, "session_id": null,
                "limit": 4, "cursor": null, "plan_id": plan}}}),
        )?)
        .map_err(|_| invalid())?;
        if page.sessions.len() > 4 {
            return Err(invalid());
        }
        candidates.extend(page.sessions.into_iter().map(|s| SessionCandidate {
            session_id: s.session_id,
            plan_id: plan.clone(),
            last_activity_ms: s.last_request_at_ms,
        }));
    }
    candidates.sort_by(|a, b| {
        b.last_activity_ms
            .cmp(&a.last_activity_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
            .then_with(|| a.plan_id.cmp(&b.plan_id))
    });
    Ok(candidates)
}
/// Whether the agent's connection made a request in this session through this Plan. This
/// uses the public exact-evidence match, so it recognizes the accepted configuration and
/// current publication only.
pub(super) fn attributed(
    query: &Query,
    context: &str,
    candidate: &SessionCandidate,
) -> Result<bool, Failure> {
    let status: AgentSettingsStatusV2 = serde_json::from_value(query.call(
        "GetAgentConnectionStatus",
        json!({"schema_version":{"major":2,"minor":0},"context_id":context,
            "effective":{"plan_id":candidate.plan_id,"session_id":candidate.session_id}}),
    )?)
    .map_err(|_| invalid())?;
    if !status.valid() || status.context_id() != context {
        return Err(invalid());
    }
    Ok(status
        .model()
        .and_then(|m| m.effective.as_ref())
        .and_then(|e| e.observed_request.as_ref())
        .is_some_and(|r| r.session_id == candidate.session_id))
}
pub(super) fn token_share_today(query: &Query) -> Result<ObservationTokenShareHeadlineV1, Failure> {
    let payload = crate::observation::value_show(&[
        "--breakdown".into(),
        "billing_class".into(),
        "--period".into(),
        "today".into(),
    ])
    .map_err(|_| invalid())?;
    let value = query.call("GetValue", payload)?;
    serde_json::from_value(
        value
            .pointer("/token_share/headline")
            .cloned()
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())
}
pub(super) fn valid_suggestion_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
#[derive(Serialize)]
pub(super) struct Check {
    pub agent: String,
    pub launchable: bool,
    pub check_reason_code: Option<String>,
}
pub(super) fn check(program: &Path, agent: &str, timeout: Duration) -> Check {
    check_with_tail(program, agent, &[], timeout)
}
pub(super) fn check_with_tail(
    program: &Path,
    agent: &str,
    tail: &[OsString],
    timeout: Duration,
) -> Check {
    check_at(program, agent, tail, timeout, None)
}
pub(super) fn check_at(
    program: &Path,
    agent: &str,
    tail: &[OsString],
    timeout: Duration,
    directory: Option<&Path>,
) -> Check {
    let mut args: Vec<OsString> = ["agent", "launch", "--agent", agent, "--check", "--"]
        .map(OsString::from)
        .into();
    args.extend_from_slice(tail);
    let result = (!timeout.is_zero())
        .then(|| host::process::run_in(program, &args, timeout, directory))
        .and_then(Result::ok)
        .and_then(|out| serde_json::from_slice::<serde_json::Value>(&out.stdout).ok())
        .and_then(|v| v.get("data").cloned())
        .and_then(|v| serde_json::from_value::<AgentLaunchCheckV1>(v).ok())
        .filter(|v| {
            v.agent.as_str() == agent
                && v.schema == "hiroute.agent-launch-check/v1"
                && v.checks.len() <= 4
        });
    let launchable = result
        .as_ref()
        .is_some_and(|v| v.launchable && !v.checks.is_empty() && v.checks.iter().all(|c| c.ok));
    let reason = if launchable {
        None
    } else {
        Some(
            result
                .as_ref()
                .and_then(|r| r.checks.iter().find(|e| !e.ok))
                .and_then(|e| e.reason_code.clone())
                .filter(|c| {
                    !c.is_empty()
                        && c.len() <= 80
                        && c.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                })
                .unwrap_or_else(|| "launch_check_unavailable".into()),
        )
    };
    Check {
        agent: agent.into(),
        launchable,
        check_reason_code: reason,
    }
}
