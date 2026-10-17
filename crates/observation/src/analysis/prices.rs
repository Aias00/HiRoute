//! Deduplicated, bounded Application price captures. A reused scope never asks
//! the control plane to freeze a second vector of current prices.
use super::scope::{Scope, digest};
use crate::query_v2::ObservationV2Error as Error;
use hiroute_domain::*;
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn load(db: &Connection, s: &Scope) -> Result<Option<AnalysisPricesV1>, Error> {
    let Some(id) = &s.p else { return Ok(None) };
    let body:Option<String>=db.query_row("SELECT body FROM observation_analysis_prices_v1 WHERE workspace_id=?1 AND id=?2 AND expires_ms>=?3",params![s.workspace,id,s.expires_ms],|r|r.get(0)).optional()?;
    body.map(|body| serde_json::from_str(&body).map_err(|_| Error::Unavailable))
        .transpose()
}

pub(super) fn capture(
    db: &Connection,
    s: &mut Scope,
    prices: &AnalysisPricesV1,
) -> Result<(), Error> {
    if prices.prices.len() > 4096 || prices.evaluated_at_ms < 0 {
        return Err(Error::Invalid);
    }
    for p in &prices.prices {
        if p.quote.exact_target.workspace_id.as_str() != s.workspace
            || p.quote.valuation_kind != PriceValuationKindV1::ApiEquivalent
            || p.quote.verify_digest().is_err()
            || p.quote.exact_target.validate().is_err()
        {
            return Err(Error::Invalid);
        }
    }
    let mut semantic = serde_json::to_value(prices).map_err(|_| Error::Invalid)?;
    if let Some(object) = semantic.as_object_mut() {
        object.remove("evaluated_at_ms");
    }
    if let Some(vector) = semantic.get_mut("prices").and_then(|v| v.as_array_mut()) {
        for p in vector.iter_mut() {
            if let Some(quote) = p.get_mut("quote").and_then(|v| v.as_object_mut()) {
                quote.remove("quote_digest");
                quote.remove("attempt_execution_at");
            }
        }
        vector.sort_by_key(|p| {
            p.get("binding_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned()
        });
    }
    let hash = digest(&semantic)?;
    let existing: Option<String> = db
        .query_row(
            "SELECT id FROM observation_analysis_prices_v1 WHERE workspace_id=?1 AND digest=?2",
            params![s.workspace, hash],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        db.execute(
            "UPDATE observation_analysis_prices_v1 SET expires_ms=MAX(expires_ms,?2) WHERE id=?1",
            params![id, s.expires_ms],
        )?;
        s.p = Some(id);
        return Ok(());
    }
    db.execute(
        "DELETE FROM observation_analysis_prices_v1 WHERE workspace_id=?1 AND expires_ms<=?2",
        params![s.workspace, s.created_ms],
    )?;
    let (count,bytes):(u64,u64)=db.query_row("SELECT COUNT(*),COALESCE(SUM(bytes),0) FROM observation_analysis_prices_v1 WHERE workspace_id=?1",[&s.workspace],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let body = serde_json::to_string(prices).map_err(|_| Error::Invalid)?;
    if count >= 64 || bytes.saturating_add(body.len() as u64) > 16 * 1024 * 1024 {
        return Err(Error::Busy);
    }
    let id = digest(&(&s.workspace, &hash))?;
    db.execute(
        "INSERT INTO observation_analysis_prices_v1 VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            id,
            s.workspace,
            hash,
            body,
            prices.evaluated_at_ms,
            s.expires_ms,
            body.len()
        ],
    )?;
    s.p = Some(id);
    Ok(())
}
