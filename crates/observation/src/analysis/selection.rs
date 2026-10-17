use super::scope::Scope;
use hiroute_domain::ObservationAnalysisQueryV1;
use rusqlite::{ToSql, named_params};

pub(super) const CTE:&str="WITH selected AS (
 SELECT a.*,m.anchor_id FROM observation_analysis_requests_v1 a
 JOIN logical_requests source ON source.workspace_id=a.workspace_id AND source.request_id=a.request_id
   AND source.session_id=a.session_id AND source.started_at_ms=a.at_ms
 JOIN sessions live_session ON live_session.workspace_id=source.workspace_id AND live_session.session_id=source.session_id
 JOIN observation_analysis_members_v1 m ON m.scope_id=:scope AND m.session_id=a.session_id
 WHERE a.workspace_id=:workspace AND a.at_ms>=:from AND a.at_ms<:to AND source.rowid<=:watermark
   AND a.revision=(SELECT MAX(version.revision) FROM observation_analysis_requests_v1 version
     WHERE version.workspace_id=a.workspace_id AND version.request_id=a.request_id AND version.revision<=:e)
   AND (:plan IS NULL OR a.plan_id=:plan) AND (:agent IS NULL OR a.agent_id=:agent)
   AND (:base_request IS NULL OR a.request_id=:base_request)
   AND (:session IS NULL OR m.anchor_id=(SELECT anchor_id FROM observation_analysis_members_v1 WHERE scope_id=:scope AND session_id=:session) OR m.anchor_id=:session)
   AND (:request IS NULL OR a.request_id=:request)
 ) ";

pub(super) fn params<'a>(
    s: &'a Scope,
    q: &'a ObservationAnalysisQueryV1,
) -> Vec<(&'static str, &'a dyn ToSql)> {
    named_params!{":scope":s.id,":workspace":s.workspace,":from":s.range.effective_from_ms,":to":s.range.effective_to_ms,
      ":watermark":s.source_watermark,":e":s.e,":plan":s.plan,":agent":s.agent,":base_request":s.initial_request,
      ":session":q.session_id,":request":q.request_id}.to_vec()
}
