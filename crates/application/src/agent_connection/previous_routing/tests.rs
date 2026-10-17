use super::*;

fn fixture(id: &str, previous: Option<&str>, opus: &str, sonnet: &str) -> Node {
    let settings: AgentModelSelectionV2 = serde_json::from_value(serde_json::json!({
        "mode":"claude_launcher", "surfaces":["claude_cli"],"fixed_models":[],
        "preset_mappings":{"opus":{"kind":"plan","plan_id":opus}, "sonnet":{"kind":"plan","plan_id":sonnet},"haiku":{"kind":"preserve_native"}}
    })).unwrap();
    Node {
        id: OperationId::parse(id).unwrap(),
        previous: previous.map(|id| OperationId::parse(id).unwrap()),
        resolved: BTreeMap::from([
            ("opus".into(), serde_json::json!(opus)),
            ("sonnet".into(), serde_json::json!(sonnet)),
            ("haiku".into(), serde_json::json!("native-haiku")),
        ]),
        spec: AgentSettingsSpecV2 {
            schema_version: AGENT_SETTINGS_SCHEMA_V2,
            context_id: "context".into(),
            model: AgentFacetIntent::Configure {
                settings: settings.clone(),
            },
            collaboration: Default::default(),
            restore_native_model: None,
            protected_native_model_ids: vec![],
            access_token: AgentAccessTokenIntentV1::Regenerate,
        },
        settings,
    }
}

#[test]
fn previous_routing_skips_unchanged_opus_and_preserves_later_sonnet() {
    let current = fixture("op-3", Some("op-2"), "plan/trial", "plan/new-sonnet");
    let mut chain = BTreeMap::from([
        (
            "op-2",
            fixture("op-2", Some("op-1"), "plan/trial", "plan/sonnet"),
        ),
        ("op-1", fixture("op-1", None, "plan/opus", "plan/sonnet")),
    ]);
    let result = recover(current, &BTreeSet::from(["opus".into()]), |id| {
        chain.remove(id.as_str()).ok_or("history_missing")
    });
    let Preview::Available {
        spec,
        restored_from,
        ..
    } = result
    else {
        panic!("history should be recoverable")
    };
    let AgentFacetIntent::Configure {
        settings: AgentModelSelectionV2::ClaudeLauncher {
            preset_mappings, ..
        },
    } = spec.model
    else {
        panic!()
    };
    assert_eq!(
        preset_mappings.opus,
        AgentClaudePresetSelectionV2::Plan {
            plan_id: AgentPlanId::parse("plan/opus").unwrap()
        }
    );
    assert_eq!(
        preset_mappings.sonnet,
        AgentClaudePresetSelectionV2::Plan {
            plan_id: AgentPlanId::parse("plan/new-sonnet").unwrap()
        }
    );
    assert_eq!(restored_from["opus"].as_str(), "op-1");
    assert_eq!(spec.collaboration, AgentFacetIntent::Keep);
    assert_eq!(spec.access_token, AgentAccessTokenIntentV1::Keep);
}

#[test]
fn previous_routing_never_infers_native_from_missing_or_cyclic_history() {
    for (current, reason) in [
        (
            fixture("op-1", None, "plan/trial", "plan/sonnet"),
            "no_previous_configuration",
        ),
        (
            fixture("op-1", Some("op-1"), "plan/trial", "plan/sonnet"),
            "history_cycle",
        ),
        (
            fixture("op-1", Some("op-0"), "plan/trial", "plan/sonnet"),
            "history_missing",
        ),
    ] {
        let result = recover(current, &BTreeSet::from(["opus".into()]), |_| {
            Err("history_missing")
        });
        assert!(matches!(result, Preview::Unavailable { reason: actual, .. } if actual == reason));
    }
}

#[test]
fn previous_routing_bound_is_explicit_even_when_every_mapping_is_unchanged() {
    let result = recover(
        fixture("op-0", Some("op-1"), "plan/trial", "plan/sonnet"),
        &BTreeSet::from(["opus".into()]),
        |id| {
            let n: u64 = id.as_str().strip_prefix("op-").unwrap().parse().unwrap();
            Ok(fixture(
                id.as_str(),
                Some(&format!("op-{}", n + 1)),
                "plan/trial",
                "plan/sonnet",
            ))
        },
    );
    assert!(
        matches!(result, Preview::Unavailable { reason, .. } if reason == "history_bound_reached")
    );
}
