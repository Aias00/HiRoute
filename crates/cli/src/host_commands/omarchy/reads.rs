//! Bounded composition of public reads, shared by the host summary and notification frontends.
use super::*;
use hiroute_application_api::{
    AgentLaunchCheckV1, AgentModelSelectionV2, AgentSettingsStatusV2, RoutedLaunchReadV1,
    SystemStatusV1,
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
        let selection = if let [context] = read.check.candidate_contexts.as_slice() {
            let status: AgentSettingsStatusV2 = serde_json::from_value(query.call(
                "GetAgentConnectionStatus",
                json!({"schema_version":{"major":2,"minor":0},"context_id":context}),
            )?)
            .map_err(|_| invalid())?;
            if !status.valid() || status.context_id() != context {
                return Err(invalid());
            }
            status.model().and_then(|s| s.current_selection.clone())
        } else {
            None
        };
        result.push(Connection {
            agent: agent.into(),
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
pub(super) fn suggestions(
    query: &Query,
    connections: &[Connection],
) -> Result<Vec<host::notifications::SuggestionNotice>, Failure> {
    let mapped = mapped_plans(connections);
    if mapped.is_empty() {
        return Ok(Vec::new());
    }
    let payload =
        crate::trust::payload("observation.plan-trust.suggestions", &[]).map_err(|_| invalid())?;
    let value = query.call("GetPlanTrustSuggestions", payload)?;
    let rows = value
        .get("suggestions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(invalid)?;
    let mut notices = BTreeMap::new();
    for row in rows {
        let plan = row
            .get("plan_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(invalid)?;
        if !mapped.contains(plan) {
            continue;
        }
        for branch in row
            .get("branches")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(invalid)?
        {
            let id = branch
                .get("id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| valid_suggestion_id(id))
                .ok_or_else(invalid)?;
            notices
                .entry(id.to_owned())
                .or_insert_with(|| host::notifications::SuggestionNotice {
                    id: id.into(),
                    plan_name: plan.chars().filter(|c| !c.is_control()).take(80).collect(),
                });
        }
    }
    Ok(notices.into_values().collect())
}
fn valid_suggestion_id(id: &str) -> bool {
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
