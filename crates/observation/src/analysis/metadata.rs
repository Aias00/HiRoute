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
