//! Whitelisted per-attempt evidence; no prompt, private reasoning or credential
//! bytes are retained. Full deletion owns these rows with their source request.
use super::{schema, turns};
use hiroute_domain::{CanonicalDigest, ExecutionFactEnvelopeV1, ExecutionFactV1};
use rusqlite::{Connection, params};

pub(crate) fn project(db: &Connection, e: &ExecutionFactEnvelopeV1) -> rusqlite::Result<()> {
    if !schema::available(db) {
        return Ok(());
    }
    turns::project(db, e)?;
    let w = e.correlation.workspace_id.as_str();
    let request = e.correlation.request_id.as_str();
    match &e.fact {
        ExecutionFactV1::AttemptStarted {
            ordinal,
            request_model,
            model_configuration_id,
            profile_digest,
            credential_ref,
            key_id,
            ..
        } => {
            let credential = CanonicalDigest::of(&(credential_ref, key_id))
                .map_err(|_| rusqlite::Error::InvalidQuery)?
                .to_string();
            db.execute(
                "INSERT OR IGNORE INTO observation_analysis_models_v1 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    w,
                    request,
                    ordinal,
                    request_model,
                    model_configuration_id,
                    profile_digest.as_str(),
                    credential
                ],
            )?;
        }
        ExecutionFactV1::CandidateDecision(c) => {
            db.execute(
                "INSERT OR IGNORE INTO observation_analysis_reasoning_v1 VALUES(?1,?2,?3,?4,?5)",
                params![
                    w,
                    request,
                    c.model_configuration_id,
                    c.profile_digest.as_str(),
                    c.reasoning_profile_id
                ],
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Use the released small, non-text event catalog before opening original
/// payloads. A retained prompt/response in another fact must never be parsed
/// just to find a model or turn boundary.
pub(super) fn backfill(db: &Connection, w: &str, request: &str) -> rusqlite::Result<()> {
    let events = {
        let mut stmt = db.prepare("SELECT f.original_digest,p.body_json FROM observation_safe_facts_v2 f
            JOIN execution_fact_events e ON e.workspace_id=f.workspace_id AND e.request_id=f.request_id AND e.envelope_digest=f.original_digest
            JOIN observation_sensitive_payloads_v2 p ON p.id='fact:'||f.original_digest
            WHERE f.workspace_id=?1 AND f.request_id=?2
              AND json_extract(f.body_json,'$.event_kind') IN ('attempt_started','candidate_decision','agent_turn_finished')
              AND octet_length(p.body_json)<=65536 ORDER BY e.sequence LIMIT 2048")?;
        stmt.query_map(params![w, request], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    for (digest, body) in events {
        let Ok(e) = serde_json::from_str::<ExecutionFactEnvelopeV1>(&body) else {
            continue;
        };
        if e.correlation.workspace_id.as_str() != w
            || e.correlation.request_id.as_str() != request
            || e.validate_persisted_contract().is_err()
            || !CanonicalDigest::of(&e).is_ok_and(|d| d.as_str() == digest)
        {
            continue;
        }
        project(db, &e)?;
    }
    Ok(())
}
