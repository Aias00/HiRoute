use super::*;
use hiroute_domain::{
    CanonicalDigest, ObservationSearchQueryV2, ObservationSessionSearchPageV1,
    ObservationTextAnchorV2,
};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Instant};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema: String,
    binding: String,
    last_at: i64,
    last_id: String,
}

// These predicates are shared by hits, gaps and the final current read.
// Only content in the reader's workspace/run/query scope can contribute a hit
// or an index gap; cache readiness never grants visibility on its own.
const CONTENT_FILTERS: &str = "c.workspace_id=?1 AND c.state='complete'
 AND c.content_kind NOT IN ('provider_state','reasoning_delta','reasoning_finished')
 AND (c.canonical_media_type LIKE 'text/%' OR c.canonical_media_type='application/json' OR c.canonical_media_type LIKE 'application/vnd.hiroute.%')";
const REQUEST_FILTERS: &str = "r.started_at_ms>=?2 AND r.started_at_ms<?3
 AND (?4 IS NULL OR (l.conflicted=0 AND l.run_id IN(SELECT value FROM json_each(?4))))
 AND (?5 IS NULL OR r.session_id=?5) AND (?6 IS NULL OR s.agent_id=?6)
 AND (?7 IS NULL OR EXISTS(SELECT 1 FROM valuation_requests_v2 v WHERE v.workspace_id=r.workspace_id AND v.request_id=r.request_id AND v.plan_id=?7))
 AND (?8 IS NULL OR EXISTS(SELECT 1 FROM observation_attempt_models_v2 a WHERE a.workspace_id=r.workspace_id AND a.request_id=r.request_id AND a.model_id=?8))
 AND (?9 IS NULL OR r.outcome=?9)
 AND (s.tombstone_reason IS NULL OR EXISTS(SELECT 1 FROM observation_tombstones t WHERE t.workspace_id=s.workspace_id AND t.session_id=s.session_id AND t.delete_scope='content_only'))";
const READY_SOURCES: &str =
    "SELECT i.digest FROM observation_text_index_v2 i INDEXED BY observation_text_index_ready
 JOIN observation_text_lookup_sources_v1 k ON k.workspace=i.workspace AND k.digest=i.digest
 WHERE i.workspace=?1 AND i.state='ready'";

fn all_filters() -> String {
    format!("{CONTENT_FILTERS} AND {REQUEST_FILTERS}")
}

const REQUEST_JOINS: &str =
    // Keep known content/request identities driving PK lookups. Reordering
    // through the request time range scatters the large readiness scan and
    // amplifies common keyword blocks by every request in the range.
    "CROSS JOIN logical_requests r ON r.workspace_id=c.workspace_id AND r.request_id=c.request_id
 CROSS JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
 LEFT JOIN observation_run_links l ON l.workspace_id=r.workspace_id AND l.request_id=r.request_id";

fn switch_filter(enabled: bool) -> &'static str {
    if enabled {
        " AND ((SELECT COUNT(DISTINCT model_id) FROM observation_attempt_models_v2 a WHERE a.workspace_id=r.workspace_id AND a.request_id=r.request_id)>1 OR EXISTS(SELECT 1 FROM turn_changes t WHERE t.request_id=r.request_id AND t.changed=1))"
    } else {
        ""
    }
}

fn prefix(enabled: bool) -> String {
    if enabled {
        super::turns::cte(4, "9223372036854775807")
    } else {
        String::new()
    }
}

pub(super) fn candidate_sql(only_switch: bool, exact_gram: bool) -> String {
    let with = if only_switch {
        format!("{}, matched_content AS (", prefix(true))
    } else {
        "WITH matched_content AS (".into()
    };
    // One complete encoded gram is exact for a folded needle of 1..=3 scalars.
    // Longer needles need substring verification before choosing any content.
    // Scan source identities and covering content metadata, not every folded
    // body. FTS membership precedes ready-source checks and optional body reads;
    // grouping lets request authority run once before session ranking.
    let exact = if exact_gram {
        ""
    } else {
        " AND instr(b.folded,?10)>0"
    };
    format!(
        "{with}
 SELECT c.workspace_id,c.request_id,MIN(c.rowid) AS content_rowid
 FROM observation_text_lookup_v1
 CROSS JOIN observation_text_blocks_v2 b ON b.rowid=observation_text_lookup_v1.rowid
 CROSS JOIN content_instances_v2 c INDEXED BY content_instances_v2_search_blob
   ON c.workspace_id=b.workspace AND c.content_blob_digest=b.digest
 WHERE observation_text_lookup_v1 MATCH ?11 AND b.workspace=?1
   AND EXISTS(SELECT 1 FROM ({READY_SOURCES}) ready WHERE ready.digest=b.digest)
   {exact} AND {CONTENT_FILTERS}
 GROUP BY c.workspace_id,c.request_id
 ), matched_requests AS (
 SELECT r.session_id,r.request_id,r.started_at_ms AS last_at,c.content_rowid
 FROM matched_content c {REQUEST_JOINS}
 WHERE {REQUEST_FILTERS}{}
 ), ranked AS (
 SELECT *,ROW_NUMBER() OVER(PARTITION BY session_id ORDER BY last_at DESC,request_id) AS pick
 FROM matched_requests
 ), page AS (
 SELECT * FROM ranked WHERE pick=1 AND (last_at<?12 OR (last_at=?12 AND session_id>?13))
 ORDER BY last_at DESC,session_id LIMIT ?14
 )
 SELECT p.session_id,p.request_id,c.message_instance_id,c.content_id,
   b.original_start,substr(b.offsets,(instr(b.folded,?10)-1)*8+5,4) AS original_offset,
   NULL AS link,p.last_at
 FROM page p
 CROSS JOIN content_instances_v2 c ON c.rowid=p.content_rowid
 CROSS JOIN observation_text_blocks_v2 b ON b.rowid=(
   SELECT first.rowid FROM observation_text_blocks_v2 first
   WHERE first.workspace=c.workspace_id AND first.digest=c.content_blob_digest
     AND instr(first.folded,?10)>0 ORDER BY first.ordinal LIMIT 1)
 ORDER BY p.last_at DESC,p.session_id",
        switch_filter(only_switch)
    )
}

fn candidates_for(
    db: &Connection,
    only_switch: bool,
    needle: &str,
    expression: &str,
    workspace: &str,
) -> Result<String, ObservationV2Error> {
    let cardinality:u64=db.query_row("SELECT COUNT(*) FROM (SELECT b.rowid FROM observation_text_lookup_v1 CROSS JOIN observation_text_blocks_v2 b ON b.rowid=observation_text_lookup_v1.rowid WHERE observation_text_lookup_v1 MATCH ?1 AND b.workspace=?2 LIMIT 257)",params![expression,workspace],|r|r.get(0))?;
    if cardinality <= 256 {
        return Ok(candidate_sql(only_switch, needle.chars().count() <= 3));
    }
    // Common grams should not multiply every repeated occurrence before
    // authorizing a request. A request-indexed existential read can stop at its
    // first verified match; sparse terms keep the source-driven existing path.
    let old = candidate_sql(only_switch, needle.chars().count() <= 3);
    let tail = old
        .split_once("), ranked AS (")
        .ok_or(ObservationV2Error::Unavailable)?
        .1;
    let with = if only_switch {
        format!("{}, ", prefix(true))
    } else {
        "WITH ".into()
    };
    let exact = if needle.chars().count() <= 3 {
        ""
    } else {
        " AND instr(b.folded,?10)>0"
    };
    Ok(format!("{with} matched_requests AS (
       SELECT r.session_id,r.request_id,r.started_at_ms AS last_at,
        (SELECT c.rowid FROM content_instances_v2 c INDEXED BY content_instances_v2_request
         WHERE c.request_id=r.request_id AND {CONTENT_FILTERS}
          AND EXISTS(SELECT 1 FROM observation_text_blocks_v2 b INDEXED BY observation_text_blocks_source
            WHERE b.workspace=c.workspace_id AND b.digest=c.content_blob_digest
              AND b.rowid IN(SELECT rowid FROM observation_text_lookup_v1 WHERE observation_text_lookup_v1 MATCH ?11){exact})
          AND EXISTS(SELECT 1 FROM observation_text_index_v2 i JOIN observation_text_lookup_sources_v1 k ON k.workspace=i.workspace AND k.digest=i.digest
            WHERE i.workspace=c.workspace_id AND i.digest=c.content_blob_digest AND i.state='ready') LIMIT 1) AS content_rowid
       FROM logical_requests r JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
       LEFT JOIN observation_run_links l ON l.workspace_id=r.workspace_id AND l.request_id=r.request_id
       WHERE r.workspace_id=?1 AND {REQUEST_FILTERS}{} AND content_rowid IS NOT NULL
      ), ranked AS ({tail}",switch_filter(only_switch)))
}

pub(super) fn index_partial_sql(only_switch: bool) -> String {
    // Repeated native history can put hundreds of thousands of occurrences in
    // large metadata pages. First deduplicate through the covering digest index,
    // then subtract ready sources before opening any occurrence metadata. A WHERE
    // predicate alone lets SQLite load those pages before checking readiness.
    // Only the remaining candidates undergo the full current visibility checks.
    let filters = all_filters();
    let with = if only_switch {
        format!("{}, ", prefix(true))
    } else {
        "WITH ".into()
    };
    format!(
        "{with} sources AS MATERIALIZED (
          SELECT DISTINCT content_blob_digest FROM content_instances_v2
          INDEXED BY content_instances_v2_live_blob WHERE workspace_id=?1 AND state='complete'
        ), unready_sources AS MATERIALIZED (
          SELECT content_blob_digest FROM sources WHERE content_blob_digest IS NULL
            OR content_blob_digest NOT IN({READY_SOURCES})
        ) SELECT EXISTS(
        SELECT 1 FROM unready_sources u
        CROSS JOIN content_instances_v2 c INDEXED BY content_instances_v2_search_blob
          ON c.workspace_id=?1 AND c.content_blob_digest IS u.content_blob_digest
        {REQUEST_JOINS} WHERE {filters}{})",
        switch_filter(only_switch)
    )
}

pub(super) fn live_read_sql(only_switch: bool) -> String {
    // Returned identities, not all workspace content, drive the current read.
    let filters = all_filters();
    format!("{} SELECT c.content_id,CASE WHEN l.conflicted=0 THEN l.body_json ELSE NULL END
        FROM json_each(?10) ids CROSS JOIN content_instances_v2 c ON c.workspace_id=?1 AND c.content_id=ids.value
        {REQUEST_JOINS} WHERE {filters}{}",prefix(only_switch),switch_filter(only_switch))
}

fn native_turn(body: Option<String>) -> Option<String> {
    body.and_then(|json| serde_json::from_str::<RunObservationLink>(&json).ok())
        .and_then(|link| link.native_turn_id)
}

/// Freeze matching physical members using this same verified literal-search
/// pipeline. No occurrence cursor, replacement text index or second search
/// semantics is introduced by the grouped analysis consumer.
pub(crate) fn analysis_matching_sessions(
    db: &Connection,
    reader: &ObservationReaderContext,
    q: &hiroute_domain::ObservationAnalysisQueryV1,
    range: &hiroute_domain::AnalysisTimeRangeV1,
) -> Result<(std::collections::BTreeSet<String>, bool), ObservationV2Error> {
    let needle = crate::text_index::fold(
        q.query
            .as_deref()
            .ok_or(ObservationV2Error::Invalid)?
            .trim(),
    )
    .0;
    let expression = crate::text_index::lookup::expression(&needle);
    let candidate = candidates_for(db, false, &needle, &expression, reader.workspace().as_str())?;
    let prefix = candidate
        .split_once("), ranked AS (")
        .ok_or(ObservationV2Error::Unavailable)?
        .0;
    let sql = format!("{prefix}) SELECT DISTINCT session_id FROM matched_requests");
    let runs: Option<String> = None;
    let optional: Option<String> = None;
    let args = params![
        reader.workspace().as_str(),
        range.effective_from_ms,
        range.effective_to_ms,
        runs,
        optional,
        q.agent_id,
        q.plan_id,
        optional,
        optional,
        needle,
        expression
    ];
    let sessions = db
        .prepare(&sql)?
        .query_map(args, |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let partial = db.query_row(
        &index_partial_sql(false),
        params![
            reader.workspace().as_str(),
            range.effective_from_ms,
            range.effective_to_ms,
            runs,
            optional,
            q.agent_id,
            q.plan_id,
            optional,
            optional
        ],
        |r| r.get(0),
    )?;
    Ok((sessions, partial))
}

impl crate::LocalObservationStore {
    pub fn search_observed_sessions(
        &self,
        reader: &ObservationReaderContext,
        query: &ObservationSearchQueryV2,
        now_ms: i64,
    ) -> Result<ObservationSessionSearchPageV1, ObservationV2Error> {
        reader.check(now_ms, true, true)?;
        let started = Instant::now();
        let keyword = query.keyword.trim();
        if keyword.is_empty()
            || keyword.chars().count() > 256
            || query.from_ms < 0
            || query.from_ms >= query.to_ms
            || query.limit == 0
            || query.limit > 200
            || [
                &query.session_id,
                &query.agent_id,
                &query.plan_id,
                &query.native_model,
                &query.outcome,
            ]
            .into_iter()
            .flatten()
            .any(|id| !identifier(id))
        {
            return Err(ObservationV2Error::Invalid);
        }
        let _permit = self.query_permit()?;
        let needle = crate::text_index::fold(keyword).0;
        let expression = crate::text_index::lookup::expression(&needle);
        let mut normalized = query.clone();
        normalized.keyword = needle.clone();
        normalized.cursor = None;
        let binding = CanonicalDigest::of(&(reader.live_scope_binding()?, normalized))
            .map_err(|_| ObservationV2Error::Invalid)?
            .to_string();
        let cursor = match &query.cursor {
            Some(encoded) => {
                let cursor: Cursor = self.decode_observation_cursor(encoded)?;
                if cursor.schema != "session-search/v1" || cursor.binding != binding {
                    return Err(ObservationV2Error::Stale);
                }
                cursor
            }
            None => Cursor {
                schema: "session-search/v1".into(),
                binding,
                last_at: i64::MAX,
                last_id: String::new(),
            },
        };
        let runs = reader
            .allowed_runs()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| ObservationV2Error::Invalid)?;
        let from = query.from_ms.max(
            now_ms
                .saturating_sub(crate::managed_text::RETENTION_MS)
                .saturating_add(1),
        );
        let mut connection =
            Connection::open_with_flags(&self.activity_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let _deadline = QueryDeadline::start_for(&connection, reader)?;
        let tx = connection.transaction()?;
        let mut hits = Vec::new();
        let mut last_at = 0;
        {
            let mut stmt = tx.prepare(&candidates_for(
                &tx,
                query.only_model_switch,
                &needle,
                &expression,
                reader.workspace().as_str(),
            )?)?;
            let mut rows = stmt.query(params![
                reader.workspace().as_str(),
                from,
                query.to_ms,
                runs,
                query.session_id,
                query.agent_id,
                query.plan_id,
                query.native_model,
                query.outcome,
                needle,
                expression,
                cursor.last_at,
                cursor.last_id,
                u64::from(query.limit) + 1
            ])?;
            while let Some(row) = rows.next()? {
                let offset: Vec<u8> = row.get(5)?;
                let offset = u32::from_le_bytes(
                    offset
                        .try_into()
                        .map_err(|_| ObservationV2Error::Unavailable)?,
                );
                let start: u64 = row.get(4)?;
                hits.push(ObservationTextAnchorV2 {
                    session_id: row.get(0)?,
                    request_id: row.get(1)?,
                    native_turn_id: native_turn(row.get(6)?),
                    message_occurrence_id: row.get(2)?,
                    content_id: row.get(3)?,
                    original_text_offset: start + u64::from(offset),
                    match_kind: "literal_text".into(),
                });
                if hits.len() <= usize::from(query.limit) {
                    last_at = row.get(7)?;
                }
            }
        }
        let more = hits.len() > usize::from(query.limit);
        hits.truncate(usize::from(query.limit));
        let next_cursor = if more {
            Some(
                self.encode_observation_cursor(&Cursor {
                    last_at,
                    last_id: hits
                        .last()
                        .ok_or(ObservationV2Error::Unavailable)?
                        .session_id
                        .clone(),
                    ..cursor
                })?,
            )
        } else {
            None
        };
        let index_partial = tx.query_row(
            &index_partial_sql(query.only_model_switch),
            params![
                reader.workspace().as_str(),
                from,
                query.to_ms,
                runs,
                query.session_id,
                query.agent_id,
                query.plan_id,
                query.native_model,
                query.outcome
            ],
            |r| r.get(0),
        )?;
        tx.commit()?;

        // Re-read just the returned identities, rather than rejecting every page
        // when unrelated append-only activity increments the global generation.
        if !hits.is_empty() {
            let identities =
                serde_json::to_string(&hits.iter().map(|h| &h.content_id).collect::<Vec<_>>())
                    .map_err(|_| ObservationV2Error::Invalid)?;
            let mut stmt = connection.prepare(&live_read_sql(query.only_model_switch))?;
            let live: HashMap<String, Option<String>> = stmt
                .query_map(
                    params![
                        reader.workspace().as_str(),
                        from,
                        query.to_ms,
                        runs,
                        query.session_id,
                        query.agent_id,
                        query.plan_id,
                        query.native_model,
                        query.outcome,
                        identities
                    ],
                    |r| Ok((r.get(0)?, native_turn(r.get(1)?))),
                )?
                .collect::<Result<_, _>>()?;
            hits.retain_mut(|hit| match live.get(&hit.content_id) {
                Some(turn) => {
                    hit.native_turn_id.clone_from(turn);
                    true
                }
                None => false,
            });
        }
        reader.check(
            now_ms.saturating_add(started.elapsed().as_millis() as i64),
            true,
            true,
        )?;
        Ok(ObservationSessionSearchPageV1 {
            hits,
            next_cursor,
            index_partial,
        })
    }
}
