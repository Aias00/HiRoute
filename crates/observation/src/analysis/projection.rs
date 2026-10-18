//! Versioned numeric inputs. Source writes and this derived state share their
//! transaction; released writers can leave a SQL-only dirty notification.
use super::schema;
use crate::{
    LocalObservationStore,
    valuation::{RawUsage, ValuationRecordV2},
};
use hiroute_domain::{
    CanonicalDigest, ExecutionPricingEvidenceV1, PriceBillingContextV1, PriceModelIdentityV1,
    UsageSemanticsV1,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct PricingIdentity {
    pub source_id: String,
    pub source_digest: String,
    pub model_id: String,
    pub standard_tokens: bool,
    pub semantics: UsageSemanticsV1,
}

pub(super) fn model_key(model: &PriceModelIdentityV1) -> &str {
    match model {
        PriceModelIdentityV1::CatalogModel(id) | PriceModelIdentityV1::LocalModel(id) => id,
    }
}

fn pricing_key(db: &Connection, json: Option<&str>) -> rusqlite::Result<Option<String>> {
    let Some(pricing) = json
        .filter(|s| s.len() <= 65536)
        .and_then(|s| serde_json::from_str::<ExecutionPricingEvidenceV1>(s).ok())
        .filter(|p| p.validate().is_ok())
    else {
        return Ok(None);
    };
    let Some(quote) = pricing.quote else {
        return Ok(None);
    };
    let identity = PricingIdentity {
        source_id: quote.exact_target.source_id.to_string(),
        source_digest: quote.exact_target.source_identity_digest.to_string(),
        model_id: model_key(&quote.exact_target.model_identity).into(),
        standard_tokens: quote.billing_context == PriceBillingContextV1::StandardTokens,
        semantics: pricing.usage_semantics,
    };
    let key = CanonicalDigest::of(&identity)
        .map_err(|_| rusqlite::Error::InvalidQuery)?
        .to_string();
    let body = serde_json::to_string(&identity).map_err(|_| rusqlite::Error::InvalidQuery)?;
    db.execute(
        "INSERT OR IGNORE INTO observation_analysis_pricing_v1(key,body) VALUES(?1,?2)",
        params![key, body],
    )?;
    Ok(Some(key))
}

/// Never create more than 200 request versions while holding the writer. A
/// backlog's fixed target is not restarted by new, projection-aware traffic.
pub(crate) fn sync(db: &Connection, limit: usize, now_ms: i64) -> rusqlite::Result<usize> {
    if !schema::available(db) {
        return Ok(0);
    }
    let work = {
        let mut stmt=db.prepare("SELECT workspace_id,request_id FROM observation_analysis_dirty_v1 ORDER BY sequence DESC,workspace_id,request_id LIMIT ?1")?;
        stmt.query_map([limit.min(200)], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    if !work.is_empty() && schema::get(db, "job_status")? == 0 {
        let total: u64 = db.query_row(
            "SELECT COUNT(*) FROM observation_analysis_dirty_v1",
            [],
            |r| r.get(0),
        )?;
        schema::set(db, "job_total", &total.to_string())?;
        schema::set(db, "job_processed", "0")?;
        schema::set(db, "job_started_ms", &now_ms.max(0).to_string())?;
        schema::set(db, "job_last_progress_ms", &now_ms.max(0).to_string())?;
        let target = schema::get(db, "dirty_sequence")?;
        schema::set(db, "job_target", &target.to_string())?;
        schema::set(db, "job_status", "1")?;
    }
    let revision = if work.is_empty() {
        0
    } else {
        schema::advance(db, "numeric_revision")?
    };
    for (workspace, request) in &work {
        project_request(db, workspace, request, revision)?;
        db.execute(
            "DELETE FROM observation_analysis_dirty_v1 WHERE workspace_id=?1 AND request_id=?2",
            params![workspace, request],
        )?;
    }
    if !work.is_empty() {
        let n = schema::get(db, "job_processed")?.saturating_add(work.len() as u64);
        schema::set(db, "job_processed", &n.to_string())?;
        schema::set(db, "job_last_progress_ms", &now_ms.max(0).to_string())?;
    }
    let remaining: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM observation_analysis_dirty_v1)",
        [],
        |r| r.get(0),
    )?;
    if !remaining {
        schema::save_checkpoint(db)?;
        schema::set(db, "job_status", "0")?;
    }
    Ok(work.len())
}

fn project_request(db: &Connection, w: &str, id: &str, rev: u64) -> rusqlite::Result<()> {
    // Join current source rows on every read too. Deletion correctness never
    // depends on the background removal of these historical versions.
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM logical_requests WHERE workspace_id=?1 AND request_id=?2)",
        params![w, id],
        |r| r.get(0),
    )?;
    if !exists {
        db.execute(
            "DELETE FROM observation_native_roots_v1 WHERE workspace_id=?1 AND request_id=?2",
            params![w, id],
        )?;
        for table in [
            "observation_analysis_attempts_v1",
            "observation_analysis_recorded_v1",
            "observation_analysis_requests_v1",
            "observation_analysis_models_v1",
            "observation_analysis_reasoning_v1",
            "observation_analysis_turns_v1",
        ] {
            db.execute(
                &format!("DELETE FROM {table} WHERE workspace_id=?1 AND request_id=?2"),
                params![w, id],
            )?;
        }
        return Ok(());
    }
    super::metadata::backfill(db, w, id)?;
    // Bounded, request-indexed metadata extraction also supports admission of
    // older stores. Deleted payloads leave explicitly unknown fields.
    let model:Option<(String,String,Option<String>)>=db.query_row("SELECT m.native_model,m.model_id,reason.reasoning_profile_id FROM observation_analysis_models_v1 m JOIN valuation_requests_v2 v ON v.workspace_id=m.workspace_id AND v.request_id=m.request_id AND v.accepted_ordinal=m.ordinal LEFT JOIN observation_analysis_reasoning_v1 reason ON reason.workspace_id=m.workspace_id AND reason.request_id=m.request_id AND reason.model_id=m.model_id AND reason.profile_digest=m.profile_digest WHERE m.workspace_id=?1 AND m.request_id=?2",params![w,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let (native, model, reason) = model
        .map(|(n, m, r)| (Some(n), Some(m), r))
        .unwrap_or_default();
    let epoch:Option<String>=db.query_row("SELECT producer_epoch FROM execution_fact_events WHERE workspace_id=?1 AND request_id=?2 ORDER BY sequence LIMIT 1",params![w,id],|r|r.get(0)).optional()?;
    db.execute("INSERT INTO observation_analysis_requests_v1(workspace_id,request_id,revision,session_id,turn_id,at_ms,finished_ms,plan_id,agent_id,traffic,terminal,partial,accepted_ordinal,input_digest,recorded_pending,native_model,model_id,reasoning_profile_id,producer_epoch,turn_ordinal)
        SELECT r.workspace_id,r.request_id,?3,r.session_id,r.turn_id,r.started_at_ms,r.finished_at_ms,v.plan_id,s.agent_id,r.traffic_kind,COALESCE(v.terminal,0),COALESCE(v.partial,1),v.accepted_ordinal,COALESCE(v.input_digest,''),
          NOT EXISTS(SELECT 1 FROM valuation_records_v2 vr WHERE vr.workspace_id=r.workspace_id AND vr.request_id=r.request_id AND vr.input_digest=v.input_digest),?4,?5,?6,?7,NULL
        FROM logical_requests r JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
        LEFT JOIN valuation_requests_v2 v ON v.workspace_id=r.workspace_id AND v.request_id=r.request_id
        WHERE r.workspace_id=?1 AND r.request_id=?2",params![w,id,rev,native,model,reason,epoch])?;
    let inputs = {
        let mut stmt=db.prepare("SELECT ordinal,pricing_json,usage_json FROM valuation_attempt_inputs_v2 WHERE workspace_id=?1 AND request_id=?2 ORDER BY ordinal LIMIT 257")?;
        stmt.query_map(params![w, id], |r| {
            Ok((
                r.get::<_, u64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    for (ordinal, pricing, usage) in inputs.into_iter().take(256) {
        let key = pricing_key(db, pricing.as_deref())?;
        let raw = usage
            .as_deref()
            .filter(|s| s.len() <= 65536)
            .and_then(|s| serde_json::from_str::<RawUsage>(s).ok())
            .unwrap_or_default();
        let text = |n: Option<u64>| n.map(|n| n.to_string());
        db.execute(
            "INSERT INTO observation_analysis_attempts_v1 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                w,
                id,
                rev,
                ordinal,
                key,
                text(raw.input),
                text(raw.output),
                text(raw.read),
                text(raw.write),
                text(raw.reasoning)
            ],
        )?;
    }
    let record:Option<String>=db.query_row("SELECT body_json FROM valuation_records_v2 WHERE workspace_id=?1 AND request_id=?2 ORDER BY revision DESC LIMIT 1",params![w,id],|r|r.get(0)).optional()?;
    if let Some(record) = record.and_then(|s| serde_json::from_str::<ValuationRecordV2>(&s).ok()) {
        for amount in record.amounts {
            let kind = serde_json::to_value(amount.valuation_kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .ok_or(rusqlite::Error::InvalidQuery)?;
            let coverage = serde_json::to_value(amount.coverage)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .ok_or(rusqlite::Error::InvalidQuery)?;
            db.execute(
                "INSERT INTO observation_analysis_recorded_v1 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    w,
                    id,
                    rev,
                    amount.currency,
                    kind,
                    amount.known_sum_micros.map(|v| v.to_string()),
                    coverage
                ],
            )?;
        }
    }
    Ok(())
}

impl LocalObservationStore {
    pub fn reconcile_analysis_batch(&self, now_ms: i64) -> rusqlite::Result<usize> {
        let Some(mut db) = self.connection.try_lock() else {
            return Ok(0);
        };
        let tx = db.transaction()?;
        let n = sync(&tx, 200, now_ms)?;
        if schema::available(&tx) {
            super::scope::expire(&tx, now_ms)?;
        }
        let workspaces = {
            let mut stmt=tx.prepare("SELECT DISTINCT c.workspace_id FROM observation_analysis_cuts_v1 c JOIN observation_native_event_members_v1 m ON m.workspace_id=c.workspace_id AND m.session_id=c.session_id LIMIT 200")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut cleaned = 0;
        for workspace in workspaces {
            cleaned += super::native::redact_events(&tx, &workspace, None, 200)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            if cleaned >= 200 {
                break;
            }
        }
        tx.commit()?;
        Ok(n.max(cleaned))
    }
}
