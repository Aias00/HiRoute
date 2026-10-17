use super::schema;
use hiroute_domain::{ExecutionFactEnvelopeV1, ExecutionFactV1};
use rusqlite::{Connection, params};

pub(crate) fn project(db: &Connection, e: &ExecutionFactEnvelopeV1) -> rusqlite::Result<()> {
    if !schema::available(db) {
        return Ok(());
    }
    let ExecutionFactV1::AgentTurnFinished {
        agent_turn_id,
        ordinal,
        history_partial,
        first_request_id: Some(first),
        last_request_id: Some(last),
        ..
    } = &e.fact
    else {
        return Ok(());
    };
    let w = e.correlation.workspace_id.as_str();
    let session = e.correlation.conversation_id.as_str();
    let rows = {
        let mut stmt=db.prepare("SELECT r.request_id FROM logical_requests r WHERE r.workspace_id=?1 AND r.session_id=?2
      AND r.started_at_ms BETWEEN (SELECT started_at_ms FROM logical_requests WHERE workspace_id=?1 AND request_id=?3 AND session_id=?2)
        AND (SELECT started_at_ms FROM logical_requests WHERE workspace_id=?1 AND request_id=?4 AND session_id=?2)")?;
        stmt.query_map(params![w, session, first.as_str(), last.as_str()], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    for request in rows {
        db.execute(
            "INSERT INTO observation_analysis_turns_v1 VALUES(?1,?2,?3,?4,?5,?6)
          ON CONFLICT(workspace_id,request_id) DO UPDATE SET partial=MAX(partial,excluded.partial)",
            params![
                w,
                request,
                agent_turn_id,
                ordinal,
                e.producer.stream.producer_epoch.as_str(),
                history_partial
            ],
        )?;
    }
    Ok(())
}
