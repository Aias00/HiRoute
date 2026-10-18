//! Creation evidence can prove a new edge after deletion; a fresh lookup alone
//! cannot. Keep it outside strict v1 native event JSON for retained readers.
use crate::query_v2::ObservationV2Error as Error;
use hiroute_domain::NativeContinuationEvidenceV1;
use rusqlite::{Connection, params};

pub(super) fn record(db: &Connection, e: &NativeContinuationEvidenceV1) -> Result<(), Error> {
    let Some(state) = &e.current_state else {
        return Ok(());
    };
    let Some(started) = state.started_at_ms else {
        return Ok(());
    };
    let sample = &state.observation;
    // Admit only an own-row sample with bounded clock uncertainty. Subtract
    // the entire allowance: near-boundary or skewed clocks cannot revive links.
    let at_ingress = sample.observed_at_ms.saturating_add(sample.age_ms as i64);
    if started < 0
        || started > sample.observed_at_ms
        || sample.age_ms > 1000
        || at_ingress.abs_diff(e.ingress_at_ms) > 1000
    {
        return Ok(());
    }
    db.execute(
        "INSERT OR IGNORE INTO observation_native_origins_v1 VALUES(?1,?2,?3)",
        params![
            e.workspace_id,
            e.current_session_id,
            started.saturating_sub(1000)
        ],
    )?;
    Ok(())
}

pub(super) fn edge_retained(
    db: &Connection,
    w: &str,
    parent: &str,
    child: &str,
) -> Result<bool, Error> {
    Ok(db.query_row("SELECT NOT EXISTS(
        SELECT 1 FROM observation_analysis_cuts_v1 c
        WHERE c.workspace_id=?1 AND c.session_id IN(?2,?3)
          AND NOT EXISTS(SELECT 1 FROM observation_native_origins_v1 o
            WHERE o.workspace_id=c.workspace_id AND o.session_id=?3 AND o.not_before_ms>c.through_ms))",
        params![w,parent,child], |r|r.get(0))?)
}
