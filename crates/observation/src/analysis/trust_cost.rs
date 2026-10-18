//! Select one exact trial scope; all token pricing and rounding stay in comparison::evaluate.
use super::{
    comparison,
    legacy::{ProjectedComparison, aggregate},
    schema,
};
use crate::query_v2::ObservationV2Error as Error;
use hiroute_domain::*;
use rusqlite::{Connection, named_params};

pub(crate) fn trust_comparison(
    db: &Connection,
    reader: &ObservationReaderContext,
    scope: &TrustCostScopeV1,
    reference: &ApiEquivalentPriceV1,
    prices: &[ApiEquivalentPriceV1],
    retained: i64,
    now: i64,
) -> Result<ProjectedComparison, Error> {
    if !schema::available(db) {
        return Err(Error::Unavailable);
    }
    let group = match scope.group {
        ExecutionGroupV1::Regular => "regular",
        ExecutionGroupV1::Primary => "primary",
    };
    // Missing stage identity cannot silently disappear from the denominator. Readiness is
    // conservative across the Plan window, including evidence not yet assigned to a revision.
    let pending: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM logical_requests r
      JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
      JOIN valuation_requests_v2 v ON v.workspace_id=r.workspace_id AND v.request_id=r.request_id
      LEFT JOIN observation_analysis_turns_v1 t ON t.workspace_id=r.workspace_id AND t.request_id=r.request_id
      LEFT JOIN plan_quality_segments q ON q.workspace_id=r.workspace_id AND q.segment_id=t.segment_id AND q.session_id=r.session_id
      WHERE r.workspace_id=:workspace AND v.plan_id=:plan AND r.started_at_ms>=:from AND r.started_at_ms<:to
        AND r.traffic_kind='normal' AND (s.tombstone_reason IS NULL OR s.content_completeness='deleted')
        AND (t.segment_id IS NULL OR t.partial!=0 OR q.plan_id IS NULL OR q.plan_id!=:plan
          OR json_extract(q.branch_execution_json,'$.group') IS NULL
          OR EXISTS(SELECT 1 FROM observation_analysis_dirty_v1 d WHERE d.workspace_id=r.workspace_id AND d.request_id=r.request_id)
          OR NOT EXISTS(SELECT 1 FROM observation_analysis_requests_v1 a WHERE a.workspace_id=r.workspace_id AND a.request_id=r.request_id)))",
      named_params! {":workspace":reader.workspace().as_str(),":plan":scope.plan_id,":from":scope.from_ms.max(retained),":to":scope.to_ms}, |r| r.get(0))?;
    if pending {
        return Err(Error::Unavailable);
    }
    let selected = "WITH selected AS (SELECT a.*,r.session_id AS anchor_id FROM logical_requests r
      JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
      JOIN observation_analysis_turns_v1 t ON t.workspace_id=r.workspace_id AND t.request_id=r.request_id AND t.partial=0
      JOIN plan_quality_segments q ON q.workspace_id=r.workspace_id AND q.segment_id=t.segment_id AND q.session_id=r.session_id
      JOIN observation_analysis_requests_v1 a ON a.workspace_id=r.workspace_id AND a.request_id=r.request_id
      WHERE r.workspace_id=:workspace AND q.plan_id=:plan AND (:revision IS NULL OR q.plan_revision=:revision)
        AND (:branch IS NULL OR COALESCE(q.executed_branch_id,q.selected_branch_id)=:branch)
        AND json_extract(q.branch_execution_json,'$.group')=:group
        AND r.started_at_ms>=:from AND r.started_at_ms<:to
        AND (s.tombstone_reason IS NULL OR s.content_completeness='deleted')
        AND a.revision=(SELECT MAX(latest.revision) FROM observation_analysis_requests_v1 latest WHERE latest.workspace_id=r.workspace_id AND latest.request_id=r.request_id))";
    let data = comparison::evaluate(
        db,
        selected,
        named_params! {":workspace":reader.workspace().as_str(),":plan":scope.plan_id,":revision":scope.revision,":branch":scope.branch_id,":group":group,":from":scope.from_ms.max(retained),":to":scope.to_ms},
        prices,
        Some(reference),
        &scope.currency,
        reader,
        now,
    )?;
    Ok(aggregate(data))
}
