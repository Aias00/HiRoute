use super::tests::{link, open, query, reader, seed};
use super::*;
use hiroute_domain::{ContentCompleteness, FactsCompleteness, ObservationQueryPort};
use rusqlite::params;

fn retire_during_session_read(store: &crate::LocalObservationStore) {
    let mut db = store.connection.lock();
    let transaction = db.transaction().unwrap();
    transaction
        .execute("DELETE FROM logical_requests WHERE request_id='first'", [])
        .unwrap();
    invalidate_visibility(&transaction).unwrap();
    transaction.commit().unwrap();
}

#[test]
fn a_fresh_session_page_restarts_after_retirement_without_returning_expired_membership() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(directory.path());
    seed(&store, "first", 100);
    seed(&store, "second", 200);
    *store.session_read_test_hook.lock() = Some(Box::new(retire_during_session_read));
    let page = store
        .observed_sessions(&reader(None), &query(50), 500)
        .unwrap();
    assert_eq!(page.sessions.len(), 1);
    assert_eq!(page.sessions[0].request_count, 1);
}

#[test]
fn a_paged_session_read_stays_stale_after_retirement_and_keeps_its_cursor_contract() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(directory.path());
    seed(&store, "first", 100);
    seed(&store, "second", 200);
    {
        let db = store.connection.lock();
        db.execute("INSERT INTO sessions(workspace_id,session_id,started_at_ms,updated_at_ms) VALUES(?1,'other-session',200,200)", [WorkspaceId::DEFAULT]).unwrap();
        db.execute(
            "UPDATE logical_requests SET session_id='other-session' WHERE request_id='second'",
            [],
        )
        .unwrap();
    }
    let first = store
        .observed_sessions(&reader(None), &query(1), 500)
        .unwrap();
    assert!(first.next_cursor.is_some());
    *store.session_read_test_hook.lock() = Some(Box::new(retire_during_session_read));
    let next = ObservationRequestQuery {
        cursor: first.next_cursor,
        ..query(1)
    };
    assert!(matches!(
        store.observed_sessions(&reader(None), &next, 500),
        Err(ObservationV2Error::Stale)
    ));
}

#[test]
fn a_fresh_session_page_never_extends_an_existing_query_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(directory.path());
    seed(&store, "first", 100);
    let scope = reader(None).with_query_deadline(std::time::Instant::now());
    assert!(matches!(
        store.observed_sessions(&scope, &query(50), 500),
        Err(ObservationV2Error::Timeout)
    ));
}

#[test]
fn session_content_completeness_covers_all_visible_requests_and_recovers() {
    let root = tempfile::tempdir().unwrap();
    let store = open(root.path());
    seed(&store, "first", 100);
    seed(&store, "second", 200);
    store
        .link_observed_request(&link("first", "allowed"))
        .unwrap();
    store
        .link_observed_request(&link("second", "other"))
        .unwrap();
    {
        let db = store.connection.lock();
        db.execute(
            "UPDATE logical_requests SET outcome='accepted',finished_at_ms=300",
            [],
        )
        .unwrap();
        db.execute("UPDATE sessions SET content_completeness='complete'", [])
            .unwrap();
        for direction in ["request_input", "response_delivered"] {
            db.execute("INSERT INTO transcript_roots_v2 VALUES(?1,?2,'session','first',?3,'fork',NULL,'finish',100000000)", params![WorkspaceId::DEFAULT,format!("first-{direction}"),direction]).unwrap();
        }
    }
    let state = |scope: ObservationReaderContext, q: ObservationRequestQuery| {
        store.observed_sessions(&scope, &q, 500).unwrap().sessions[0].content_completeness
    };
    assert_eq!(state(reader(None), query(50)), ContentCompleteness::Partial);
    // A complete title request cannot establish completeness for its sibling,
    // but neither may that sibling leak into a narrower authorized/query scope.
    assert_eq!(
        state(reader(Some("allowed")), query(50)),
        ContentCompleteness::Complete
    );
    assert_eq!(
        state(
            reader(None),
            ObservationRequestQuery {
                to_ms: 150,
                ..query(50)
            }
        ),
        ContentCompleteness::Complete
    );
    assert_eq!(
        state(
            reader(None),
            ObservationRequestQuery {
                request_id: Some("first".into()),
                ..query(50)
            }
        ),
        ContentCompleteness::Complete
    );
    {
        let db = store.connection.lock();
        for direction in ["request_input", "response_delivered"] {
            db.execute("INSERT INTO transcript_roots_v2 VALUES(?1,?2,'session','second',?3,'fork',NULL,'finish',200000000)", params![WorkspaceId::DEFAULT,format!("second-{direction}"),direction]).unwrap();
        }
    }
    assert_eq!(
        state(reader(None), query(50)),
        ContentCompleteness::Complete
    );
    store
        .connection
        .lock()
        .execute("UPDATE sessions SET content_completeness='deleted'", [])
        .unwrap();
    assert_eq!(state(reader(None), query(50)), ContentCompleteness::Deleted);
}

#[test]
fn status_truncates_gap_details_without_losing_health_or_full_scope_completeness() {
    let root = tempfile::tempdir().unwrap();
    let store = open(root.path());
    seed(&store, "first", 100);
    {
        let db = store.connection.lock();
        db.execute(
            "UPDATE sessions SET facts_completeness='complete',content_completeness='complete'",
            [],
        )
        .unwrap();
        for n in 0..201 {
            db.execute("INSERT INTO observation_gaps(channel,producer_id,producer_epoch,stream_id,first_sequence,last_sequence,known_loss,workspace_id,session_id) VALUES(?1,'producer','epoch','stream',?2,?2,1,?3,'session')", params![if n == 200 { "fact" } else { "content" }, n, WorkspaceId::DEFAULT]).unwrap();
        }
    }
    let status = store.get_status(&WorkspaceId::default()).unwrap();
    assert_eq!(status.gaps.len(), 200);
    assert!(status.gaps_truncated);
    assert_eq!(status.content_completeness, ContentCompleteness::Partial);
    assert_eq!(
        status.facts_completeness,
        FactsCompleteness::Partial,
        "gap outside the detail window still affects the aggregate"
    );
}
