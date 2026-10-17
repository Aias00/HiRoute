use super::scope::Scope;
use hiroute_domain::ObservationAnalysisQueryV1;
use rusqlite::{ToSql, named_params};

// Drive once from the narrow live-source index, then fetch the latest pinned
// numeric version by rowid. CROSS JOIN keeps SQLite from scanning a version
// and probing its primary index a second time for every retained request.
pub(super) const CTE: &str = "WITH selected AS (
 SELECT a.*,m.anchor_id FROM logical_requests source INDEXED BY analysis_source_guard_v1
 CROSS JOIN observation_analysis_members_v1 m ON m.scope_id=:scope AND m.session_id=source.session_id
 CROSS JOIN sessions live_session ON live_session.workspace_id=source.workspace_id AND live_session.session_id=source.session_id
 CROSS JOIN observation_analysis_requests_v1 a ON a.rowid=(
   SELECT version.rowid FROM observation_analysis_requests_v1 version
   WHERE version.workspace_id=source.workspace_id AND version.request_id=source.request_id AND version.revision<=:e
   ORDER BY version.revision DESC LIMIT 1)
 WHERE source.workspace_id=:workspace AND source.started_at_ms>=:from AND source.started_at_ms<:to AND source.rowid<=:watermark
   AND a.session_id=source.session_id AND a.at_ms=source.started_at_ms
   AND (:plan IS NULL OR a.plan_id=:plan) AND (:agent IS NULL OR a.agent_id=:agent)
   AND (:base_request IS NULL OR a.request_id=:base_request)
   AND (:session IS NULL OR m.anchor_id=(SELECT anchor_id FROM observation_analysis_members_v1 WHERE scope_id=:scope AND session_id=:session) OR m.anchor_id=:session)
   AND (:request IS NULL OR a.request_id=:request)
 )
";

pub(super) fn params<'a>(
    s: &'a Scope,
    q: &'a ObservationAnalysisQueryV1,
) -> Vec<(&'static str, &'a dyn ToSql)> {
    named_params!{":scope":s.id,":workspace":s.workspace,":from":s.range.effective_from_ms,":to":s.range.effective_to_ms,
      ":watermark":s.source_watermark,":e":s.e,":plan":s.plan,":agent":s.agent,":base_request":s.initial_request,
      ":session":q.session_id,":request":q.request_id}.to_vec()
}
