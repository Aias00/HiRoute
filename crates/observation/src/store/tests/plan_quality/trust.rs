use super::*;
use hiroute_domain::TrustPopulationQueryV1;

#[test]
fn trust_population_keeps_full_active_history_beside_the_selected_window() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path());
    let fixture = Fixture::new("trust-population");
    let channel = fact_channel(&fixture, 256 * 1024);
    let writer = writer(&store);
    for (index, at) in [1000, 2000].into_iter().enumerate() {
        let fact = finished(
            &fixture,
            &format!("stage-{index}"),
            &format!("turn-{index}"),
            index as u64 + 1,
            "smart_saving",
            at,
        );
        assert!(matches!(
            offer_fact(&writer, &channel, fixture.fact(index as u64 + 1, fact, at)),
            WriterCycleOutcome::Ack(_)
        ));
    }
    let query = TrustPopulationQueryV1 {
        active_revision: 17,
        plan_id: PLAN.into(),
        branch_id: None,
        revision: Some(17),
        from_ms: Some(1500),
        to_ms: Some(3000),
    };
    let population = store
        .observed_trust_population(&reader(), &query, 5000)
        .unwrap();
    assert_eq!(population.stages.len(), 1);
    assert_eq!(population.active_stages.len(), 2);
    assert!(population.active_stages[0].has_later_turn);
    assert!(!population.active_stages[1].has_later_turn);
    assert_eq!(population.from_ms, 1500);
    assert_eq!(population.active_stages[0].sample.assessment, None);
    assert!(matches!(
        offer_fact(
            &writer,
            &channel,
            fixture.fact(
                3,
                assessment(&fixture, "stage-0", ("turn-0", "turn-0", 1, 1), 2100, 0.4),
                2100
            )
        ),
        WriterCycleOutcome::Ack(_)
    ));
    let updated = store
        .observed_trust_population(&reader(), &query, 5000)
        .unwrap();
    assert!(updated.generation > population.generation);
    assert_eq!(
        updated.active_stages[0]
            .sample
            .assessment
            .as_ref()
            .unwrap()
            .score,
        0.4
    );
    assert_eq!(updated.stages.len(), 1);
}
#[test]
fn trust_population_cannot_expand_a_run_scoped_reader() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path());
    let fixture = Fixture::new("trust-scoped");
    let channel = fact_channel(&fixture, 256 * 1024);
    let writer = writer(&store);
    assert!(matches!(
        offer_fact(
            &writer,
            &channel,
            fixture.fact(
                1,
                finished(&fixture, "stage", "turn", 1, "smart_saving", 1000),
                1000
            )
        ),
        WriterCycleOutcome::Ack(_)
    ));
    let reader = ObservationReaderContext::run_scoped(
        hiroute_domain::WorkspaceId::default(),
        "test".into(),
        1,
        6000,
        std::collections::BTreeSet::from(["unrelated-run".into()]),
        false,
        false,
    )
    .unwrap();
    let query = TrustPopulationQueryV1 {
        active_revision: 17,
        plan_id: PLAN.into(),
        branch_id: None,
        revision: None,
        from_ms: None,
        to_ms: None,
    };
    let population = store
        .observed_trust_population(&reader, &query, 5000)
        .unwrap();
    assert!(population.stages.is_empty());
    assert!(population.active_stages.is_empty());
}
