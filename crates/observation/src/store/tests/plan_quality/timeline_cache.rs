use super::*;
use crate::query_v2::ObservationV2Error;
use hiroute_domain::{RoutingTimelineQueryV1, WorkspaceId};

#[test]
fn timeline_evicted_frozen_page_restarts_without_skipping_retained_turns() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path());
    let first = Fixture::new("evicted-first");
    let channel = fact_channel(&first, 256 * 1024);
    let writer = writer(&store);
    for index in 0..2 {
        let mut fixture = Fixture::new(&format!("evicted-{index}"));
        fixture.session = first.session.clone();
        fixture.fact_stream = first.fact_stream.clone();
        assert!(matches!(
            offer_fact(
                &writer,
                &channel,
                fixture.fact(
                    index + 1,
                    finished(
                        &fixture,
                        "stage",
                        &format!("turn-{index}"),
                        index + 1,
                        "smart_saving",
                        1000 + index * 100
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
    let first_page = store
        .observed_routing_timeline(&reader(), &query, 5000)
        .unwrap();
    assert!(!first_page.complete);
    for session in ["session/other-a", "session/other-b"] {
        store
            .observed_routing_timeline(
                &reader(),
                &RoutingTimelineQueryV1 {
                    session_id: session.into(),
                    after_watermark: None,
                    limit: 1,
                },
                5000,
            )
            .unwrap();
    }
    query.after_watermark = Some(first_page.watermark);
    assert_eq!(
        store
            .observed_routing_timeline(&reader(), &query, 5000)
            .unwrap_err(),
        ObservationV2Error::Stale
    );
    query.after_watermark = None;
    let fresh = store
        .observed_routing_timeline(&reader(), &query, 5000)
        .unwrap();
    assert_eq!(fresh.turns[0].turn_id, first_page.turns[0].turn_id);
    query.after_watermark = Some(fresh.watermark);
    let last = store
        .observed_routing_timeline(&reader(), &query, 5000)
        .unwrap();
    assert!(last.complete);
    assert_ne!(last.turns[0].turn_id, first_page.turns[0].turn_id);
}

#[test]
fn timeline_cached_cursor_expires_without_a_retention_maintenance_write() {
    let directory = tempfile::tempdir().unwrap();
    let store = open_store(directory.path());
    let fixture = Fixture::new("expired-cached");
    let channel = fact_channel(&fixture, 256 * 1024);
    assert!(matches!(
        offer_fact(
            &writer(&store),
            &channel,
            fixture.fact(
                1,
                finished(&fixture, "stage", "turn", 1, "smart_saving", 1000),
                1010
            )
        ),
        WriterCycleOutcome::Ack(_)
    ));
    let reader = ObservationReaderContext::local_user(
        WorkspaceId::default(),
        "user".into(),
        1,
        SEVEN_DAYS_MILLIS * 2,
        false,
        false,
    )
    .unwrap();
    let mut query = RoutingTimelineQueryV1 {
        session_id: fixture.session.to_string(),
        after_watermark: None,
        limit: 50,
    };
    let page = store
        .observed_routing_timeline(&reader, &query, 5000)
        .unwrap();
    assert_eq!(page.turns.len(), 1);
    query.after_watermark = Some(page.watermark);
    let expired = SEVEN_DAYS_MILLIS + 2000;
    assert_eq!(
        store
            .observed_routing_timeline(&reader, &query, expired)
            .unwrap_err(),
        ObservationV2Error::Stale
    );
    query.after_watermark = None;
    assert!(
        store
            .observed_routing_timeline(&reader, &query, expired)
            .unwrap()
            .turns
            .is_empty()
    );
}
