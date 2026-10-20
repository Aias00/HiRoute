use super::*;
use hiroute_application_api::{AgentClaudePresetMappingsV2, CodexNativeModelModeV2};

fn id(value: &str) -> AgentPlanId {
    AgentPlanId::parse(value).unwrap()
}
fn plan(name: &str, trust: PlanTrustStateV1) -> reads::PlanFacts {
    reads::PlanFacts {
        name: name.into(),
        trust,
    }
}
fn uniform(level: &str, threshold_millis: u16) -> PlanTrustStateV1 {
    PlanTrustStateV1::Uniform {
        level: level.into(),
        threshold_millis,
    }
}
fn check(agent: &str) -> reads::Check {
    reads::Check {
        agent: agent.into(),
        launchable: true,
        check_reason_code: None,
    }
}
fn claude(opus: Option<&str>, sonnet: Option<&str>, haiku: Option<&str>) -> AgentModelSelectionV2 {
    let slot = |plan: Option<&str>| match plan {
        Some(plan) => AgentClaudePresetSelectionV2::Plan { plan_id: id(plan) },
        None => AgentClaudePresetSelectionV2::PreserveNative,
    };
    AgentModelSelectionV2::ClaudeLauncher {
        surfaces: BTreeSet::new(),
        fixed_models: Vec::new(),
        preset_mappings: AgentClaudePresetMappingsV2 {
            opus: slot(opus),
            sonnet: slot(sonnet),
            haiku: slot(haiku),
        },
    }
}
fn codex(default: AgentModelDefaultSelectionV2, allowed: &[&str]) -> AgentModelSelectionV2 {
    AgentModelSelectionV2::CodexDefault {
        native_model_mode: CodexNativeModelModeV2::HirouteOnly,
        fixed_models: Vec::new(),
        allowed_plan_ids: allowed.iter().map(|p| id(p)).collect(),
        default_selection: default,
    }
}
const NOW: i64 = 1_800_000_000_000;
fn facts(agents: Vec<(&str, AgentModelSelectionV2)>) -> Facts {
    Facts {
        agents: agents
            .into_iter()
            .map(|(agent, selection)| AgentFacts {
                check: check(agent),
                selection: Some(selection),
            })
            .collect(),
        plans: [
            (
                "plan-a",
                plan("Difficulty routing", uniform("trusted", 650)),
            ),
            (
                "plan-b",
                plan("Fast drafts", PlanTrustStateV1::NotApplicable),
            ),
            (
                "plan-c",
                plan("Per branch", PlanTrustStateV1::BranchSpecific),
            ),
        ]
        .into_iter()
        .map(|(id, facts)| (id.to_owned(), facts))
        .collect(),
        suggestions: reads::SuggestionRead::default(),
        latest: None,
        token_share_today: ObservationTokenShareHeadlineV1::NoRoutedRequests,
    }
}

#[test]
fn omarchy_summary_lists_each_claude_slot_plan_once_in_preset_order() {
    let summary = compose(
        facts(vec![(
            "claude-code",
            claude(Some("plan-b"), Some("plan-a"), Some("plan-b")),
        )]),
        NOW,
    )
    .unwrap();
    let agent = &summary.agents[0];
    let listed: Vec<_> = agent
        .slot_plans
        .iter()
        .map(|p| (p.plan_id.as_str(), p.slots.clone()))
        .collect();
    assert_eq!(
        listed,
        [
            ("plan-b", vec![Slot::Opus, Slot::Haiku]),
            ("plan-a", vec![Slot::Sonnet])
        ]
    );
    assert_eq!(agent.other_mapped_plans, 0);
    let value = serde_json::to_value(&summary).unwrap();
    assert_eq!(
        value["agents"][0]["slot_plans"][0]["slots"],
        json!(["opus", "haiku"])
    );
    assert_eq!(
        value["agents"][0]["slot_plans"][1]["plan_name"],
        "Difficulty routing"
    );
}

#[test]
fn omarchy_summary_omits_native_and_fixed_model_slots_but_keeps_plans_without_controls() {
    let summary = compose(
        facts(vec![
            ("claude-code", claude(None, Some("plan-b"), None)),
            (
                "codex",
                codex(
                    AgentModelDefaultSelectionV2::FixedModel {
                        client_model_id: "gpt-5".into(),
                    },
                    &["plan-a"],
                ),
            ),
        ]),
        NOW,
    )
    .unwrap();
    let claude = &summary.agents[0];
    assert_eq!(claude.slot_plans.len(), 1);
    assert_eq!(claude.slot_plans[0].slots, [Slot::Sonnet]);
    assert_eq!(claude.slot_plans[0].trust_state, TrustState::NotApplicable);
    assert_eq!(claude.slot_plans[0].trust_level, None);
    assert_eq!(claude.slot_plans[0].threshold_millis, None);
    let codex = &summary.agents[1];
    assert!(codex.slot_plans.is_empty());
    assert_eq!(codex.other_mapped_plans, 1);
}

#[test]
fn omarchy_summary_counts_codex_plans_outside_the_default_slot_exactly() {
    let mut allowed = vec!["plan-a".to_owned()];
    allowed.extend((1..=9).map(|n| format!("plan-extra-{n}")));
    let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
    let summary = compose(
        facts(vec![(
            "codex",
            codex(
                AgentModelDefaultSelectionV2::Plan {
                    plan_id: id("plan-a"),
                },
                &allowed,
            ),
        )]),
        NOW,
    )
    .unwrap();
    let agent = &summary.agents[0];
    assert_eq!(agent.slot_plans.len(), 1);
    assert_eq!(agent.slot_plans[0].slots, [Slot::Default]);
    assert_eq!(agent.slot_plans[0].trust_state, TrustState::Uniform);
    assert_eq!(agent.slot_plans[0].trust_level.as_deref(), Some("trusted"));
    assert_eq!(agent.slot_plans[0].threshold_millis, Some(650));
    assert_eq!(agent.other_mapped_plans, 9);
}

#[test]
fn omarchy_summary_never_guesses_an_unreadable_slot_plan() {
    let mut facts = facts(vec![("claude-code", claude(Some("plan-a"), None, None))]);
    facts.plans.clear();
    assert!(compose(facts, NOW).is_none());
}

#[test]
fn omarchy_summary_counts_suggestions_across_every_mapped_plan_once() {
    let mapped: BTreeSet<String> = ["plan-a", "plan-extra"].map(String::from).into();
    let read = reads::parse_suggestions(
        &json!({
            "suggestions": [
                {"plan_id":"plan-extra","branches":[{"id":"s-shared"},{"id":"s-extra"}]},
                {"plan_id":"plan-a","branches":[{"id":"s-shared"}]},
                {"plan_id":"plan-unmapped","branches":[{"id":"s-other"}]}
            ],
            "unavailable": [
                {"plan_id":"plan-a","reason":"stale"},
                {"plan_id":"plan-unmapped","reason":"unavailable"}
            ]
        }),
        &mapped,
    )
    .unwrap();
    assert_eq!(read.ids.len(), 2);
    assert_eq!(read.unavailable, BTreeSet::from(["plan-a".to_owned()]));
    let mut facts = facts(vec![(
        "codex",
        codex(
            AgentModelDefaultSelectionV2::Plan {
                plan_id: id("plan-a"),
            },
            &["plan-a", "plan-extra"],
        ),
    )]);
    facts.suggestions = read;
    let summary = compose(facts, NOW).unwrap();
    assert_eq!(
        summary.suggestions,
        Some(Suggestions {
            count: 2,
            ids: vec!["s-extra".into(), "s-shared".into()],
            unavailable_plans: 1,
        })
    );
    // A malformed read is not evidence of zero suggestions.
    assert!(reads::parse_suggestions(&json!({"suggestions": []}), &mapped).is_none());
    assert!(
        reads::parse_suggestions(
            &json!({"suggestions":[{"plan_id":"plan-a","branches":[{"id":"bad id"}]}],"unavailable":[]}),
            &mapped
        )
        .is_none()
    );
}

#[test]
fn omarchy_summary_bounds_suggestion_ids_but_keeps_the_exact_count() {
    let mut facts = facts(vec![(
        "codex",
        codex(
            AgentModelDefaultSelectionV2::Plan {
                plan_id: id("plan-a"),
            },
            &["plan-a"],
        ),
    )]);
    facts.suggestions.ids = (0..25)
        .map(|n| (format!("s-{n:02}"), "plan-a".to_owned()))
        .collect();
    let suggestions = compose(facts, NOW).unwrap().suggestions.unwrap();
    assert_eq!(suggestions.count, 25);
    assert_eq!(suggestions.ids.len(), 20);
}

#[test]
fn omarchy_summary_attributes_the_latest_session_or_reports_none() {
    let candidates = vec![
        reads::SessionCandidate {
            session_id: "other-client".into(),
            plan_id: "plan-a".into(),
            last_activity_ms: NOW - 10,
        },
        reads::SessionCandidate {
            session_id: "codex-session".into(),
            plan_id: "plan-a".into(),
            last_activity_ms: NOW - 20,
        },
    ];
    let routed = vec![(
        "codex",
        "context-codex",
        BTreeSet::from(["plan-a".to_owned()]),
    )];
    let mut asked = Vec::new();
    let latest = choose_latest(&candidates, &routed, |context, candidate| {
        asked.push((context.to_owned(), candidate.session_id.clone()));
        Ok(candidate.session_id == "codex-session")
    })
    .unwrap()
    .unwrap();
    assert_eq!(latest.session_id, "codex-session");
    assert_eq!(latest.agent, "codex");
    assert_eq!(asked.len(), 2);
    assert_eq!(
        choose_latest(&candidates, &routed, |_, _| Ok(false)).unwrap(),
        None
    );
    assert!(choose_latest(&candidates, &routed, |_, _| Err(invalid())).is_err());

    let mut present = facts(vec![(
        "codex",
        codex(
            AgentModelDefaultSelectionV2::Plan {
                plan_id: id("plan-a"),
            },
            &["plan-a"],
        ),
    )]);
    present.latest = Some(latest);
    let value = serde_json::to_value(compose(present, NOW).unwrap()).unwrap();
    assert_eq!(
        value["latest_routed_session"]["session_id"],
        "codex-session"
    );
    let absent = compose(
        facts(vec![(
            "codex",
            codex(
                AgentModelDefaultSelectionV2::Plan {
                    plan_id: id("plan-a"),
                },
                &["plan-a"],
            ),
        )]),
        NOW,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(absent).unwrap()["latest_routed_session"],
        json!(null)
    );
}

#[test]
fn omarchy_summary_carries_every_token_share_headline() {
    let headlines = [
        ObservationTokenShareHeadlineV1::NoRoutedRequests,
        ObservationTokenShareHeadlineV1::NoSubscriptionInPeriod,
        ObservationTokenShareHeadlineV1::SubscriptionUsageUnknown {
            unknown_usage_requests: 3,
        },
        ObservationTokenShareHeadlineV1::NoComparableUsage,
        ObservationTokenShareHeadlineV1::Share {
            outside_subscription_ratio_millis: 420,
            known_classified_tokens: 1000,
            unknown_usage_requests: 1,
        },
    ];
    for headline in headlines {
        let mut facts = facts(Vec::new());
        facts.token_share_today = headline.clone();
        let summary = compose(facts, NOW).unwrap();
        assert_eq!(summary.token_share_today, Some(headline));
    }
    let mut facts = facts(Vec::new());
    facts.token_share_today = ObservationTokenShareHeadlineV1::Share {
        outside_subscription_ratio_millis: 1001,
        known_classified_tokens: 1,
        unknown_usage_requests: 0,
    };
    assert!(compose(facts, NOW).is_none());
}

fn ready_value() -> serde_json::Value {
    serde_json::to_value(
        compose(
            facts(vec![(
                "claude-code",
                claude(Some("plan-a"), Some("plan-b"), Some("plan-c")),
            )]),
            NOW,
        )
        .unwrap(),
    )
    .unwrap()
}
fn accepts(value: serde_json::Value) -> bool {
    serde_json::from_value::<Summary>(value).is_ok_and(|s| s.validate())
}

#[test]
fn omarchy_summary_validates_the_trust_state_and_value_pair() {
    let value = ready_value();
    assert!(accepts(value.clone()));
    let plans = &value["agents"][0]["slot_plans"];
    assert_eq!(plans[0]["trust_state"], "uniform");
    assert_eq!(plans[1]["trust_state"], "not_applicable");
    assert_eq!(plans[2]["trust_state"], "branch_specific");
    for (index, field, replacement) in [
        (0, "trust_level", json!(null)),
        (0, "threshold_millis", json!(null)),
        (0, "trust_level", json!("trial")),
        (0, "trust_level", json!("custom")),
        (1, "trust_level", json!("trusted")),
        (1, "threshold_millis", json!(650)),
        (2, "threshold_millis", json!(900)),
        (2, "trust_state", json!("mixed")),
        (1, "trust_state", json!("uniform")),
    ] {
        let mut changed = value.clone();
        changed["agents"][0]["slot_plans"][index][field] = replacement.clone();
        assert!(
            !accepts(changed),
            "accepted {field}={replacement} on Plan {index}"
        );
    }
    let mut custom = value.clone();
    custom["agents"][0]["slot_plans"][0]["trust_level"] = json!("custom");
    custom["agents"][0]["slot_plans"][0]["threshold_millis"] = json!(720);
    assert!(accepts(custom));
}

#[test]
fn omarchy_summary_rejects_unbounded_or_inconsistent_output() {
    let value = ready_value();
    type Mutation = Box<dyn Fn(&mut serde_json::Value)>;
    let cases: Vec<Mutation> = vec![
        Box::new(|v: &mut serde_json::Value| v["schema"] = json!("hiroute.omarchy-summary")),
        Box::new(|v: &mut serde_json::Value| v["unexpected"] = json!(true)),
        Box::new(|v: &mut serde_json::Value| {
            v["service"]["reason_code"] = json!("service_unavailable")
        }),
        Box::new(|v: &mut serde_json::Value| v["suggestions"] = json!(null)),
        Box::new(|v: &mut serde_json::Value| v["token_share_today"] = json!(null)),
        Box::new(|v: &mut serde_json::Value| v["agents"][0]["agent"] = json!("hermes")),
        Box::new(|v: &mut serde_json::Value| v["agents"][0]["launchable"] = json!(false)),
        Box::new(|v: &mut serde_json::Value| {
            v["agents"][0]["check_reason_code"] = json!("Not Ready")
        }),
        Box::new(|v: &mut serde_json::Value| {
            v["agents"][0]["slot_plans"][0]["slots"] = json!(["default"])
        }),
        Box::new(|v: &mut serde_json::Value| v["agents"][0]["slot_plans"][0]["slots"] = json!([])),
        Box::new(|v: &mut serde_json::Value| {
            v["agents"][0]["slot_plans"][1]["slots"] = json!(["opus"])
        }),
        Box::new(|v: &mut serde_json::Value| {
            v["agents"][0]["slot_plans"][0]["plan_name"] = json!("x".repeat(81))
        }),
        Box::new(|v: &mut serde_json::Value| {
            v["agents"][0]["slot_plans"][0]["plan_name"] = json!("a\u{7}b")
        }),
        Box::new(|v: &mut serde_json::Value| {
            let plans = v["agents"][0]["slot_plans"].as_array_mut().unwrap();
            plans.swap(0, 1);
        }),
        Box::new(|v: &mut serde_json::Value| v["suggestions"]["ids"] = json!(["s-1"])),
        Box::new(|v: &mut serde_json::Value| {
            v["suggestions"]["count"] = json!(21);
            v["suggestions"]["ids"] = json!((0..21).map(|n| format!("s-{n}")).collect::<Vec<_>>());
        }),
        Box::new(|v: &mut serde_json::Value| {
            v["latest_routed_session"] = json!({"session_id":"s","agent":"codex","plan_id":"plan-a","last_activity_ms":NOW - 1});
        }),
        Box::new(|v: &mut serde_json::Value| {
            v["latest_routed_session"] = json!({"session_id":"s","agent":"claude-code","plan_id":"plan-a","last_activity_ms":NOW - reads::LATEST_WINDOW_MS - 1});
        }),
    ];
    for (index, case) in cases.iter().enumerate() {
        let mut changed = value.clone();
        case(&mut changed);
        assert!(!accepts(changed), "accepted invalid case {index}");
    }
}

#[test]
fn omarchy_summary_unavailable_has_a_reason_and_no_figures() {
    let summary = unavailable(NOW, "service_unavailable");
    assert!(summary.validate());
    let value = serde_json::to_value(&summary).unwrap();
    assert_eq!(value["service"]["state"], "unavailable");
    assert_eq!(value["agents"], json!([]));
    assert_eq!(value["suggestions"], json!(null));
    let mut figures = value.clone();
    figures["token_share_today"] = json!({"kind":"no_routed_requests"});
    assert!(!accepts(figures));
    let mut no_reason = value;
    no_reason["service"]["reason_code"] = json!(null);
    assert!(!accepts(no_reason));
}

#[test]
fn omarchy_plan_labels_are_bounded_without_dropping_the_start() {
    let long = "名".repeat(100);
    let label = reads::label(&long);
    assert_eq!(label.chars().count(), 80);
    assert!(label.ends_with('…'));
    assert_eq!(reads::label("Difficulty routing"), "Difficulty routing");
}

/// The indicator's reader is tested against this same file, so producer and consumer agree.
#[test]
fn omarchy_summary_matches_the_shared_indicator_fixture() {
    let mut facts = facts(vec![
        (
            "codex",
            codex(
                AgentModelDefaultSelectionV2::Plan {
                    plan_id: id("plan-a"),
                },
                &["plan-a", "plan-extra"],
            ),
        ),
        (
            "claude-code",
            claude(Some("plan-a"), Some("plan-b"), Some("plan-c")),
        ),
    ]);
    facts.agents[1].check = reads::Check {
        agent: "claude-code".into(),
        launchable: false,
        check_reason_code: Some("native_login_required".into()),
    };
    facts.suggestions = reads::SuggestionRead {
        ids: [("s-extra", "plan-extra"), ("s-shared", "plan-a")]
            .map(|(id, plan)| (id.to_owned(), plan.to_owned()))
            .into(),
        unavailable: BTreeSet::from(["plan-c".to_owned()]),
    };
    facts.latest = Some(LatestSession {
        session_id: "codex-session".into(),
        agent: "codex".into(),
        plan_id: "plan-a".into(),
        last_activity_ms: NOW - 1000,
    });
    facts.token_share_today = ObservationTokenShareHeadlineV1::Share {
        outside_subscription_ratio_millis: 425,
        known_classified_tokens: 12000,
        unknown_usage_requests: 1,
    };
    let mut summary = compose(facts, NOW).unwrap();
    summary.notifications = true;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("summary.fixture.json")).unwrap();
    assert_eq!(serde_json::to_value(&summary).unwrap(), fixture);
    assert!(accepts(fixture));
}
