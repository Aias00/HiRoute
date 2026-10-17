use super::*;
use hiroute_domain::RoutingTimelineQueryV1;

#[test]
fn timeline_pages_replay_late_scores_without_losing_or_repeating_changes() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path());
    let first = Fixture::new("timeline-first");
    let mut second = Fixture::new("timeline-second");
    second.session = first.session.clone();
    second.fact_stream = first.fact_stream.clone();
    let channel = fact_channel(&first, 256 * 1024);
    let writer = writer(&store);
    for (index, fixture) in [&first, &second].into_iter().enumerate() {
        assert!(matches!(
            offer_fact(
                &writer,
                &channel,
                fixture.fact(
                    index as u64 + 1,
                    finished(
                        fixture,
                        &format!("segment-{index}"),
                        &format!("agent-turn-{index}"),
                        index as u64 + 1,
                        "smart_saving",
                        1000 + index as u64 * 100
                    ),
                    1010 + index as i64 * 100
                )
            ),
            WriterCycleOutcome::Ack(_)
        ));
    }
    let mut query = RoutingTimelineQueryV1 {
        session_id: first.session.to_string(),
        after_watermark: None,
        limit: 1,
    };
    let page = store
        .observed_routing_timeline(&reader(), &query, 5000)
        .unwrap();
    assert_eq!(page.turns.len(), 1);
    assert!(!page.complete);
    assert_eq!(page.turns[0].assessment_state, "unrated");
    let first_id = page.turns[0].turn_id.clone();
    let old_digest = page.turns[0].content_digest.clone();
    // A score appended while paging is delivered after the frozen initial snapshot.
    assert!(matches!(
        offer_fact(
            &writer,
            &channel,
            second.fact(
                3,
                assessment(
                    &second,
                    "segment-0",
                    ("agent-turn-0", "agent-turn-0", 1, 1),
                    1200,
                    0.4
                ),
                1200
            )
        ),
        WriterCycleOutcome::Ack(_)
    ));
    query.after_watermark = Some(page.watermark);
    let page = store
        .observed_routing_timeline(&reader(), &query, 5000)
        .unwrap();
    assert!(page.complete);
    assert_eq!(page.turns.len(), 1);
    assert_ne!(page.turns[0].turn_id, first_id);
    query.after_watermark = Some(page.watermark);
    let page = store
        .observed_routing_timeline(&reader(), &query, 5000)
        .unwrap();
    assert_eq!(page.turns[0].turn_id, first_id);
    assert_eq!(page.turns[0].assessment.as_ref().unwrap().score, 0.4);
    assert_ne!(page.turns[0].content_digest, old_digest);
    // Advancing through the remaining changed turn leaves an empty read.
    query.after_watermark = Some(page.watermark);
    let page = store
        .observed_routing_timeline(&reader(), &query, 5000)
        .unwrap();
    query.after_watermark = Some(page.watermark);
    assert!(
        store
            .observed_routing_timeline(&reader(), &query, 5000)
            .unwrap()
            .turns
            .is_empty()
    );
    // A cursor is bound to the reader and its session/page size.
    query.limit = 2;
    assert!(matches!(
        store.observed_routing_timeline(&reader(), &query, 5000),
        Err(crate::query_v2::ObservationV2Error::Stale)
    ));
    query.limit = 1;
    crate::query_v2::invalidate_visibility(&store.connection.lock()).unwrap();
    assert!(matches!(
        store.observed_routing_timeline(&reader(), &query, 5000),
        Err(crate::query_v2::ObservationV2Error::Stale)
    ));
}

#[test]
fn timeline_refuses_oversized_stored_fact_before_deserializing_it() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path());
    let fixture = Fixture::new("timeline-bounded");
    let channel = fact_channel(&fixture, 256 * 1024);
    assert!(matches!(
        offer_fact(
            &writer(&store),
            &channel,
            fixture.fact(
                1,
                finished(&fixture, "segment", "turn", 1, "smart_saving", 1000),
                1010
            )
        ),
        WriterCycleOutcome::Ack(_)
    ));
    store
        .connection
        .lock()
        .execute(
            "UPDATE observation_sensitive_payloads_v2 SET body_json=?1",
            ["x".repeat(1024 * 1024 + 1)],
        )
        .unwrap();
    assert!(matches!(
        store.observed_routing_timeline(
            &reader(),
            &RoutingTimelineQueryV1 {
                session_id: fixture.session.to_string(),
                after_watermark: None,
                limit: 100,
            },
            5000
        ),
        Err(crate::query_v2::ObservationV2Error::ScopeTooLarge)
    ));
}
