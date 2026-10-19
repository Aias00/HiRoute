//! Ordered routed-launch admission over captured configuration and publication facts.
use crate::control::{AgentConnectionControlPort, CodexLaunchDescriptorRead, ControlReadError};
use hiroute_application_api::{
    AgentGrantRawRequestV1, AgentLaunchCheckV1, AgentLaunchDescriptorRequestV1,
    RoutedLaunchAgentV1, RoutedLaunchDescriptorV1, RoutedLaunchReadV1, RoutedLaunchRequestV1,
};

pub(crate) fn read(
    port: &dyn AgentConnectionControlPort,
    request: &RoutedLaunchRequestV1,
) -> Result<RoutedLaunchReadV1, ControlReadError> {
    if request.context.as_ref().is_some_and(|context| {
        AgentGrantRawRequestV1::new(&format!("agent-connection/{context}")).is_err()
    }) {
        return Err(ControlReadError::Denied);
    }
    let facts = port.routed_launch_context(request)?;
    let mut read = RoutedLaunchReadV1 {
        activation_mode: facts.activation_mode,
        check: AgentLaunchCheckV1::new(request.agent, Some(facts.context.clone())),
        descriptor: None,
    };
    read.check.candidate_contexts = facts.candidate_contexts;
    let repair = "hiroute agents connect preview --help";
    if request.agent == RoutedLaunchAgentV1::ClaudeCode
        && request.context.is_none()
        && facts.activation_mode != Some(hiroute_domain::AgentActivationModeV1::ManagedLaunch)
    {
        read.check.candidate_contexts.clear();
        read.check.record(
            "connection",
            false,
            "independent_connection_missing",
            repair,
        );
        return Ok(read);
    }
    if request.context.is_none() && read.check.candidate_contexts.len() > 1 {
        read.check.record(
            "connection",
            false,
            "ambiguous_context",
            "hiroute agent launch --help",
        );
        return Ok(read);
    }
    if !read.check.record(
        "connection",
        facts.connection_current,
        if read.check.candidate_contexts.is_empty() {
            "connection_missing"
        } else {
            "connection_not_current"
        },
        repair,
    ) {
        return Ok(read);
    }
    let descriptor_request = AgentLaunchDescriptorRequestV1 {
        connection_id: format!("agent-connection/{}", facts.context),
    };
    // A retained pre-snapshot Codex connection is still valid for status/restore. Only this
    // new launch is unavailable until the owner explicitly confirms a fresh settings save.
    let codex = if request.agent == RoutedLaunchAgentV1::Codex {
        match port.managed_codex_launch_descriptor(&descriptor_request) {
            Ok(CodexLaunchDescriptorRead::Ready(descriptor)) => Some(descriptor),
            result => {
                read.check.checks.clear();
                read.check.record(
                    "connection",
                    false,
                    match result {
                        Ok(CodexLaunchDescriptorRead::SnapshotMissing) => "launch_snapshot_missing",
                        Err(ControlReadError::SnapshotChanged) => "installation_changed",
                        _ => "connection_not_current",
                    },
                    repair,
                );
                return Ok(read);
            }
        }
    } else {
        None
    };
    let publication = port.routed_launch_publication()?;
    let publication_digest = publication
        .as_ref()
        .map(|value| value.digest())
        .transpose()
        .map_err(|_| ControlReadError::Corrupt)?;
    let published = publication
        .as_ref()
        .map(|publication| publication.published_agent_plans())
        .transpose()
        .map_err(|_| ControlReadError::Corrupt)?;
    let plans_ready = match (&facts.selection, &publication, published) {
        (Some(selection), Some(_publication), Some(plans)) => {
            codex.as_ref().is_none_or(|descriptor| {
                Some(&descriptor.publication_digest) == publication_digest.as_ref()
            }) && selection.allowed_plan_ids().iter().all(|id| {
                plans
                    .iter()
                    .any(|plan| &plan.agent_plan_id == id && plan.active)
            })
        }
        _ => false,
    };
    if !read.check.record(
        "plans",
        plans_ready,
        "plan_unpublished",
        "hiroute routing list",
    ) {
        return Ok(read);
    }
    read.descriptor = match codex {
        Some(descriptor) => Some(RoutedLaunchDescriptorV1::Codex(descriptor)),
        None => match port.managed_launch_descriptor(&descriptor_request) {
            Ok(descriptor)
                if Some(&descriptor.publication_digest) == publication_digest.as_ref() =>
            {
                read.check.record("managed_launch", true, "", "");
                Some(RoutedLaunchDescriptorV1::ClaudeCode(Box::new(descriptor)))
            }
            _ => {
                read.check
                    .record("managed_launch", false, "managed_launch_conflict", repair);
                None
            }
        },
    };
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::RoutedLaunchContextFacts;
    use hiroute_application_api::{
        AgentConnectSpecV1, AgentConnectionStatusRequestV1, AgentConnectionStatusV1,
        ManagedClaudeLaunchDescriptorV2,
    };
    use hiroute_domain::{
        CanonicalDigest, CodexLaunchSnapshotV1, GatewayPublicationV1,
        ManagedCodexLaunchDescriptorV1,
    };
    use std::sync::Mutex;

    struct Facts {
        candidates: Vec<String>,
        current: bool,
        snapshot: bool,
        calls: Mutex<Vec<&'static str>>,
    }
    impl AgentConnectionControlPort for Facts {
        fn planning_facts(
            &self,
            _: &AgentConnectSpecV1,
        ) -> Result<super::super::AgentConnectionPlanningInputV1, ControlReadError> {
            unreachable!()
        }
        fn connection_status(
            &self,
            _: &AgentConnectionStatusRequestV1,
        ) -> Result<AgentConnectionStatusV1, ControlReadError> {
            unreachable!()
        }
        fn managed_launch_descriptor(
            &self,
            _: &AgentLaunchDescriptorRequestV1,
        ) -> Result<ManagedClaudeLaunchDescriptorV2, ControlReadError> {
            self.calls.lock().unwrap().push("claude");
            Err(ControlReadError::Denied)
        }
        fn routed_launch_context(
            &self,
            _: &RoutedLaunchRequestV1,
        ) -> Result<RoutedLaunchContextFacts, ControlReadError> {
            self.calls.lock().unwrap().push("context");
            Ok(RoutedLaunchContextFacts {
                activation_mode: Some(hiroute_domain::AgentActivationModeV1::ManagedLaunch),
                context: "scope".into(),
                candidate_contexts: self.candidates.clone(),
                connection_current: self.current,
                selection: None,
            })
        }
        fn managed_codex_launch_descriptor(
            &self,
            _: &AgentLaunchDescriptorRequestV1,
        ) -> Result<CodexLaunchDescriptorRead, ControlReadError> {
            self.calls.lock().unwrap().push("codex");
            if !self.snapshot {
                return Ok(CodexLaunchDescriptorRead::SnapshotMissing);
            }
            Ok(CodexLaunchDescriptorRead::Ready(Box::new(
                ManagedCodexLaunchDescriptorV1::trusted(
                    "agent-connection/scope".into(),
                    CodexLaunchSnapshotV1 {
                        schema: "hiroute.codex-launch-snapshot/v1".into(),
                        executable: "/owned/codex".into(),
                        codex_home: "/owned/home".into(),
                        profile_name: "hiroute".into(),
                        installation_identity: CanonicalDigest::of_bytes(b"installation"),
                    },
                    1,
                    CanonicalDigest::of_bytes(b"publication"),
                )
                .unwrap(),
            )))
        }
        fn routed_launch_publication(
            &self,
        ) -> Result<Option<GatewayPublicationV1>, ControlReadError> {
            self.calls.lock().unwrap().push("plans");
            Ok(None)
        }
    }
    fn facts(candidates: &[&str], current: bool, snapshot: bool) -> Facts {
        Facts {
            candidates: candidates.iter().map(|s| (*s).into()).collect(),
            current,
            snapshot,
            calls: Mutex::default(),
        }
    }
    fn request(agent: RoutedLaunchAgentV1) -> RoutedLaunchRequestV1 {
        RoutedLaunchRequestV1 {
            agent,
            context: None,
        }
    }

    #[test]
    fn absent_and_ambiguous_contexts_never_read_a_descriptor_or_publication() {
        for (f, reason) in [
            (facts(&[], false, true), "connection_missing"),
            (facts(&["scope", "other"], true, true), "ambiguous_context"),
        ] {
            let result = read(&f, &request(RoutedLaunchAgentV1::Codex)).unwrap();
            assert!(!result.check.launchable && result.descriptor.is_none());
            assert_eq!(result.check.checks[0].reason_code.as_deref(), Some(reason));
            assert_eq!(*f.calls.lock().unwrap(), ["context"]);
        }
    }

    #[test]
    fn old_codex_connection_needs_an_explicit_save_before_plans_are_checked() {
        let f = facts(&["scope"], true, false);
        let result = read(&f, &request(RoutedLaunchAgentV1::Codex)).unwrap();
        assert!(!result.check.launchable && result.descriptor.is_none());
        assert_eq!(result.check.checks.len(), 1);
        assert_eq!(
            result.check.checks[0].reason_code.as_deref(),
            Some("launch_snapshot_missing")
        );
        assert_eq!(*f.calls.lock().unwrap(), ["context", "codex"]);
    }

    #[test]
    fn missing_publication_stops_before_claude_managed_preflight() {
        let f = facts(&["scope"], true, true);
        let result = read(&f, &request(RoutedLaunchAgentV1::ClaudeCode)).unwrap();
        assert!(!result.check.launchable && result.descriptor.is_none());
        assert_eq!(
            result
                .check
                .checks
                .iter()
                .map(|check| check.check.as_str())
                .collect::<Vec<_>>(),
            ["connection", "plans"]
        );
        assert_eq!(
            result.check.checks[1].reason_code.as_deref(),
            Some("plan_unpublished")
        );
        assert_eq!(*f.calls.lock().unwrap(), ["context", "plans"]);
    }
}
