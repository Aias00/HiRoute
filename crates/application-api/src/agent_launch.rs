//! Opt-in routed launch input and non-secret preflight presentation. The released
//! connection-id descriptor request and Claude descriptor keep their original bytes.
use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutedLaunchAgentV1 {
    Codex,
    ClaudeCode,
}

impl RoutedLaunchAgentV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedLaunchRequestV1 {
    pub agent: RoutedLaunchAgentV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLaunchCheckEntryV1 {
    pub check: String,
    pub ok: bool,
    pub reason_code: Option<String>,
    pub repair: Option<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLaunchCheckV1 {
    pub schema: String,
    pub agent: RoutedLaunchAgentV1,
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_contexts: Vec<String>,
    pub checks: Vec<AgentLaunchCheckEntryV1>,
    pub launchable: bool,
}

impl AgentLaunchCheckV1 {
    pub fn new(agent: RoutedLaunchAgentV1, context: Option<String>) -> Self {
        Self {
            schema: "hiroute.agent-launch-check/v1".into(),
            agent,
            context,
            candidate_contexts: Vec::new(),
            checks: Vec::new(),
            launchable: false,
        }
    }

    pub fn record(&mut self, check: &str, ok: bool, reason: &str, repair: &str) -> bool {
        self.checks.push(AgentLaunchCheckEntryV1 {
            check: check.into(),
            ok,
            reason_code: (!ok).then(|| reason.into()),
            repair: (!ok).then(|| repair.into()),
        });
        self.launchable = self.checks.iter().all(|entry| entry.ok);
        ok
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RoutedLaunchDescriptorV1 {
    Codex(Box<crate::ManagedCodexLaunchDescriptorV1>),
    ClaudeCode(Box<crate::ManagedClaudeLaunchDescriptorV2>),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedLaunchReadV1 {
    pub check: AgentLaunchCheckV1,
    pub descriptor: Option<RoutedLaunchDescriptorV1>,
}
