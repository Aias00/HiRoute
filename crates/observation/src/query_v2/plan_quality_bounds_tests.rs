use super::{plan_quality::summary_sql, routing_observations::SUMMARY_SQL};
use crate::WriterCycleOutcome;
use crate::store::tests::support::{
    Fixture, fact_channel, finished, offer_fact, open_store, request_facts, writer,
};
use hiroute_domain::CanonicalDigest;
use rusqlite::{Connection, StatementStatus, named_params, types::Value};

const PLAN: &str = "plan/codex-daily";

fn seed() -> (
    tempfile::TempDir,
    std::sync::Arc<crate::LocalObservationStore>,
) {
    let root = tempfile::tempdir().unwrap();
    let store = open_store(root.path());
    let fixture = Fixture::new("quality-read-work");
    let channel = fact_channel(&fixture, 256 * 1024);
    let writer = writer(&store);
    for fact in request_facts(&fixture, "attempt-bounds", finished("attempt-bounds"), 1000) {
        assert!(matches!(
            offer_fact(&writer, &channel, fact),
            WriterCycleOutcome::Ack(_)
        ));
    }
    {
        let db = store.connection.lock();
        // Many stages can share one request across compaction boundaries. Keep
        // native-name and reasoning attribution in the aggregate being measured.
        db.execute("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<128)
            INSERT INTO plan_quality_segments(workspace_id,segment_id,session_id,plan_id,plan_revision,
              selected_branch_id,executed_branch_id,model_configuration_id,profile_digest,attribution,
              first_at_ms,last_at_ms,first_request_id,last_request_id,score,assessment_partial)
            SELECT ?1,'stage-'||i,?2,?3,17,'writing','writing','model-a',?4,'single',1000,1100,?5,?5,0.8,0 FROM n",
            rusqlite::params![fixture.workspace.as_str(),fixture.session.as_str(),PLAN,
                CanonicalDigest::of_bytes(b"profile-a").to_string(),fixture.request.as_str()]).unwrap();
        // Retained execution facts need not all have a safe projection. They
        // belong to other requests and must not multiply this Plan's lookup work.
        db.execute_batch(
            "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<8192)
            INSERT INTO execution_fact_events
            SELECT workspace_id,session_id,turn_id,'unrelated-'||i,producer_id,producer_epoch,
              'retained-unrelated',i,'event-unrelated-'||i,envelope_json,'digest-unrelated-'||i
            FROM (SELECT * FROM execution_fact_events LIMIT 1),n;",
        )
        .unwrap();
    }
    drop(writer);
    (root, store)
}

fn measured(db: &Connection, sql: &str) -> (Vec<Vec<Value>>, i32) {
    let mut statement = db.prepare(sql).unwrap();
    let columns = statement.column_count();
    let rows = statement
        .query_map(
            named_params! {
                ":workspace":"personal/default", ":plan":PLAN, ":session":Option::<String>::None,
                ":revision":17, ":from":0, ":to":5000, ":runs":Option::<String>::None,
            },
            |row| {
                (0..columns)
                    .map(|i| row.get(i))
                    .collect::<rusqlite::Result<Vec<Value>>>()
            },
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    (rows, statement.get_status(StatementStatus::VmStep))
}

#[test]
fn quality_summary_bounds_repeated_execution_projection_work() {
    let (_root, store) = seed();
    let db = store.connection.lock();
    let sql = summary_sql();
    let (rows, bounded) = measured(&db, &sql);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][6], Value::Text("native-a".into()));
    assert_eq!(rows[0][7], Value::Text("reasoning-high".into()));
    assert_eq!(rows[0][8], Value::Integer(128));
    // Negative control: the former flattened CTE repeats correlated execution
    // lookups inside the aggregate. Compare VM work, never machine wall time.
    let (previous_rows, previous) = measured(&db, &sql.replace("AS MATERIALIZED (", "AS ("));
    assert_eq!(rows, previous_rows);
    assert!(
        bounded * 4 < previous * 3,
        "summary used {bounded} VM steps; repeated projections used {previous}"
    );
}

#[test]
fn routing_summary_bounds_unrelated_execution_history_work() {
    let (_root, store) = seed();
    let db = store.connection.lock();
    let (rows, bounded) = measured(&db, SUMMARY_SQL);
    assert_eq!(
        rows.len(),
        1,
        "the one real route decision must remain included"
    );
    let (previous_rows, previous) = measured(
        &db,
        &SUMMARY_SQL.replace(
            "CROSS JOIN execution_fact_events",
            "JOIN execution_fact_events",
        ),
    );
    assert_eq!(rows, previous_rows);
    assert!(
        bounded * 4 < previous,
        "routing summary used {bounded} VM steps; workspace event scan used {previous}"
    );
}
