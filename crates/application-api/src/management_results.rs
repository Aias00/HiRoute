//! Existing management read results in the public contract owner.
use crate::*;
use hiroute_domain::{AgentSurfaceCheckStateV1, DecisionServiceV1, GatewayPublicationRevision};
use serde::Serialize;

#[derive(schemars::JsonSchema, Serialize)]
pub struct DecisionServicesListV1 {
    pub services: Vec<DecisionServiceV1>,
}

#[derive(schemars::JsonSchema, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum AgentCheckResultV1 {
    Configuration {
        schema: String,
        suite: AgentCheckSuiteV1,
        agent: DiscoveredAgentV1,
        model_call: bool,
        gateway_listener: String,
    },
    NativeAuthentication {
        schema: String,
        agent_id: String,
        native_authentication: String,
        model_call: bool,
        model_verified: bool,
    },
    Collaboration {
        schema: String,
        agent_id: String,
        skill_loading: String,
        trusted_cli_execution: String,
        model_call: bool,
    },
    Live {
        schema: String,
        agent_id: String,
        surface: AgentModelSurfaceV2,
        applied_revision: GatewayPublicationRevision,
        checked_model_ids: Vec<String>,
        requested_call_count: usize,
        call_count: usize,
        state: AgentSurfaceCheckStateV1,
        reason_code: Option<String>,
        model_call: bool,
    },
}
