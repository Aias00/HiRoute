use super::{members, schema};
use crate::{LocalObservationStore, query_v2::ObservationV2Error as Error};
use hiroute_domain::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const LIFETIME_MS: i64 = 3_600_000;

/// Expired scopes retain neither membership maps nor captured price leases.
/// The ordinary bounded writer cycle owns cleanup, outside read deadlines.
pub(super) fn expire(db: &Connection, now: i64) -> rusqlite::Result<usize> {
    let ids = {
        let mut stmt = db.prepare("SELECT id FROM observation_analysis_scopes_v1 WHERE expires_ms<=?1 ORDER BY expires_ms LIMIT 8")?;
        stmt.query_map([now], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    for id in &ids {
        db.execute(
            "DELETE FROM observation_analysis_members_v1 WHERE scope_id=?1",
            [id],
        )?;
        db.execute(
            "DELETE FROM observation_analysis_scopes_v1 WHERE id=?1",
            [id],
        )?;
    }
    db.execute("DELETE FROM observation_analysis_prices_v1 WHERE id IN (SELECT id FROM observation_analysis_prices_v1 WHERE expires_ms<=?1 ORDER BY expires_ms LIMIT 8)",[now])?;
    Ok(ids.len())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Scope {
    pub id: String,
    pub binding: String,
    pub workspace: String,
    pub created_ms: i64,
    pub expires_ms: i64,
    pub range: AnalysisTimeRangeV1,
    pub r: u64,
    pub e: Option<u64>,
    pub p: Option<String>,
    pub basis: Option<AnalysisCostBasisV1>,
    pub currency: Option<String>,
    pub valuation_kind: Option<PriceValuationKindV1>,
    pub reference: Option<String>,
    pub plan: Option<String>,
    pub agent: Option<String>,
    pub initial_session: Option<String>,
    pub initial_request: Option<String>,
    pub keyword_digest: Option<String>,
    pub used_edges: Vec<u64>,
    pub used_roots: Vec<(String, String)>,
    pub source_count: u64,
    pub source_digest: String,
    pub source_watermark: i64,
    pub index_partial: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Token {
    pub schema: String,
    pub scope: String,
    pub binding: String,
    pub page: Option<Page>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    pub query_digest: String,
    pub offset: u64,
}

pub(super) fn token(store: &LocalObservationStore, s: &Scope) -> Result<String, Error> {
    store.encode_observation_cursor(&Token {
        schema: "analysis-scope/v1".into(),
        scope: s.id.clone(),
        binding: s.binding.clone(),
        page: None,
    })
}
pub(super) fn save(db: &Connection, s: &Scope) -> Result<(), Error> {
    let body = serde_json::to_string(s).map_err(|_| Error::Unavailable)?;
    db.execute("INSERT INTO observation_analysis_scopes_v1 VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![s.id,s.workspace,body,s.expires_ms])?;
    Ok(())
}
pub(super) fn load(
    store: &LocalObservationStore,
    db: &Connection,
    reader: &ObservationReaderContext,
    q: &ObservationAnalysisQueryV1,
    now: i64,
) -> Result<(Scope, u64), Error> {
    let encoded = q
        .cursor
        .as_ref()
        .or(q.analysis_scope.as_ref())
        .ok_or(Error::Invalid)?;
    let decoded: Token = store.decode_observation_cursor(encoded)?;
    if q.cursor.is_some() != decoded.page.is_some() {
        return Err(Error::Invalid);
    }
    if let Some(encoded_scope) = &q.analysis_scope {
        let scope: Token = store.decode_observation_cursor(encoded_scope)?;
        if scope.schema != decoded.schema
            || scope.scope != decoded.scope
            || scope.binding != decoded.binding
            || scope.page.is_some()
        {
            return Err(Error::Invalid);
        }
    }
    if decoded.schema != "analysis-scope/v1"
        || decoded.binding != reader.analysis_scope_binding()?
    {
        return Err(Error::Stale);
    }
    let body: Option<String> = db
        .query_row(
            "SELECT body FROM observation_analysis_scopes_v1 WHERE id=?1 AND workspace_id=?2",
            params![decoded.scope, reader.workspace().as_str()],
            |r| r.get(0),
        )
        .optional()?;
    let s: Scope =
        serde_json::from_str(&body.ok_or(Error::Stale)?).map_err(|_| Error::Unavailable)?;
    if s.expires_ms <= now || s.binding != decoded.binding {
        return Err(Error::Stale);
    }
    if !q.time.is_empty()
        || q.plan_id
            .as_ref()
            .is_some_and(|p| Some(p) != s.plan.as_ref())
        || q.agent_id
            .as_ref()
            .is_some_and(|a| Some(a) != s.agent.as_ref())
        || q.basis.is_some_and(|b| s.basis.is_some_and(|old| b != old))
        || q.currency
            .as_ref()
            .is_some_and(|c| s.currency.as_ref().is_some_and(|old| c != old))
        || q.valuation_kind
            .is_some_and(|k| s.valuation_kind.is_some_and(|old| k != old))
        || q.reference_binding_id
            .as_ref()
            .is_some_and(|r| s.reference.as_ref().is_some_and(|old| r != old))
    {
        return Err(Error::Invalid);
    }
    if let Some(keyword) = &q.query
        && Some(digest(&crate::text_index::fold(keyword.trim()).0)?) != s.keyword_digest
    {
        return Err(Error::Invalid);
    }
    if let Some(selected) = &s.initial_session
        && let Some(requested) = &q.session_id
    {
        let same:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM observation_analysis_members_v1 a JOIN observation_analysis_members_v1 b ON b.scope_id=a.scope_id AND b.anchor_id=a.anchor_id WHERE a.scope_id=?1 AND a.session_id=?2 AND (b.session_id=?3 OR b.anchor_id=?3))",params![s.id,selected,requested],|r|r.get(0))?;
        if !same {
            return Err(Error::Invalid);
        }
    }
    if q.request_id
        .as_ref()
        .is_some_and(|id| s.initial_request.as_ref().is_some_and(|old| id != old))
    {
        return Err(Error::Invalid);
    }
    let offset = if let Some(page) = decoded.page {
        if page.query_digest != page_digest(q, &s)? {
            return Err(Error::Invalid);
        }
        page.offset
    } else {
        0
    };
    check_relations(db, &s)?;
    Ok((s, offset))
}
pub(super) fn digest(value: &impl Serialize) -> Result<String, Error> {
    CanonicalDigest::of(value)
        .map(|d| d.to_string())
        .map_err(|_| Error::Invalid)
}
pub(super) fn page_digest(q: &ObservationAnalysisQueryV1, s: &Scope) -> Result<String, Error> {
    let mut q = q.clone();
    q.cursor = None;
    q.analysis_scope = None;
    q.time = Default::default();
    q.plan_id = q.plan_id.or_else(|| s.plan.clone());
    q.agent_id = q.agent_id.or_else(|| s.agent.clone());
    q.basis = q.basis.or(s.basis);
    q.currency = q.currency.or_else(|| s.currency.clone());
    q.valuation_kind = q.valuation_kind.or(s.valuation_kind);
    q.reference_binding_id = q.reference_binding_id.or_else(|| s.reference.clone());
    q.group_by = Some(q.group_by.unwrap_or_default());
    digest(&q)
}
pub(super) fn next(
    store: &LocalObservationStore,
    s: &Scope,
    q: &ObservationAnalysisQueryV1,
    offset: u64,
) -> Result<String, Error> {
    store.encode_observation_cursor(&Token {
        schema: "analysis-scope/v1".into(),
        scope: s.id.clone(),
        binding: s.binding.clone(),
        page: Some(Page {
            query_digest: page_digest(q, s)?,
            offset,
        }),
    })
}

pub(super) fn create(
    db: &Connection,
    reader: &ObservationReaderContext,
    q: &ObservationAnalysisQueryV1,
    mut range: AnalysisTimeRangeV1,
    now: i64,
) -> Result<Scope, Error> {
    let expiry = now.checked_add(LIFETIME_MS).ok_or(Error::Invalid)?;
    range.effective_from_ms = range.requested_from_ms.max(
        expiry
            .saturating_sub(crate::managed_text::RETENTION_MS)
            .saturating_add(1),
    );
    range.clipped_for_retention = range.effective_from_ms != range.requested_from_ms;
    if range.effective_from_ms >= range.effective_to_ms {
        range.effective_from_ms = range.effective_to_ms;
        range.clipped_for_retention = true;
    }
    let r = schema::get(db, &format!("relation_revision/{}", reader.workspace()))?;
    let e = schema::get(db, "numeric_revision")?;
    let binding = reader.analysis_scope_binding()?;
    let nonce = schema::advance(db, "scope_sequence")?;
    let id = digest(&("analysis-scope/v1", &binding, now, &range, r, e, q, nonce))?;
    let mut stmt=db.prepare("SELECT DISTINCT r.session_id FROM logical_requests r JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
      LEFT JOIN valuation_requests_v2 v ON v.workspace_id=r.workspace_id AND v.request_id=r.request_id
      WHERE r.workspace_id=?1 AND r.started_at_ms>=?2 AND r.started_at_ms<?3 AND (?4 IS NULL OR v.plan_id=?4) AND (?5 IS NULL OR s.agent_id=?5) AND (?6 IS NULL OR r.request_id=?6)")?;
    let mut physical = stmt
        .query_map(
            params![
                reader.workspace().as_str(),
                range.effective_from_ms,
                range.effective_to_ms,
                q.plan_id,
                q.agent_id,
                q.request_id
            ],
            |r| r.get::<_, String>(0),
        )?
        .collect::<Result<BTreeSet<_>, _>>()?;
    let stable_from = expiry
        .saturating_sub(crate::managed_text::RETENTION_MS)
        .saturating_add(1);
    if let Some(session) = &q.session_id {
        let known:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM logical_requests WHERE workspace_id=?1 AND session_id=?2 AND started_at_ms>=?3)",params![reader.workspace().as_str(),session,stable_from],|r|r.get(0))?;
        if known {
            physical.insert(session.clone());
        }
    }
    let (mut all, _) =
        members::resolve(db, reader.workspace().as_str(), r, &physical, stable_from)?;
    let anchors = all
        .iter()
        .map(|m| m.anchor.clone())
        .collect::<BTreeSet<_>>();
    let encoded = serde_json::to_string(&anchors).map_err(|_| Error::Invalid)?;
    let mut stmt=db.prepare("SELECT p.session_id,(SELECT root.proof_id FROM observation_native_roots_v1 root
       JOIN logical_requests source ON source.workspace_id=root.workspace_id AND source.request_id=root.request_id
       WHERE root.workspace_id=p.workspace_id AND root.session_id=p.session_id AND source.started_at_ms>=?3
       ORDER BY root.observed_ms DESC,root.proof_id LIMIT 1)
      FROM observation_native_presence_v1 p WHERE p.workspace_id=?1 AND p.session_id IN(SELECT value FROM json_each(?2))")?;
    let roots = stmt
        .query_map(
            params![reader.workspace().as_str(), encoded, stable_from],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )?
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    for m in &mut all {
        if m.gap.is_none() && roots.get(&m.anchor) == Some(&None) {
            m.gap = Some(
                if m.predecessor.is_some() {
                    "ancestor_predecessor_unknown"
                } else {
                    "predecessor_unknown"
                }
                .into(),
            );
        }
    }
    let matching = if q.query.is_some() {
        Some(crate::query_v2::analysis_matching_sessions(
            db, reader, q, &range,
        )?)
    } else {
        None
    };
    let matching_anchors: Option<BTreeSet<_>> = matching.as_ref().map(|(ids, _)| {
        all.iter()
            .filter(|m| ids.contains(&m.id))
            .map(|m| m.anchor.clone())
            .collect()
    });
    let anchor = q
        .session_id
        .as_ref()
        .map(|id| {
            all.iter()
                .find(|m| &m.id == id || &m.anchor == id)
                .map(|m| m.anchor.clone())
                .ok_or(Error::Invalid)
        })
        .transpose()?;
    let selected: Vec<_> = all
        .into_iter()
        .filter(|m| anchor.as_ref().is_none_or(|a| a == &m.anchor))
        .filter(|m| {
            matching_anchors
                .as_ref()
                .is_none_or(|ids| ids.contains(&m.anchor))
        })
        .collect();
    let used_edges = selected
        .iter()
        .flat_map(|m| m.edge_versions.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let selected_anchors = selected
        .iter()
        .map(|m| m.anchor.as_str())
        .collect::<BTreeSet<_>>();
    let used_roots = roots
        .into_iter()
        .filter(|(root, _)| selected_anchors.contains(root.as_str()))
        .filter_map(|(root, proof)| proof.map(|proof| (root, proof)))
        .collect();
    {
        let mut insert =
            db.prepare("INSERT INTO observation_analysis_members_v1 VALUES(?1,?2,?3,?4,?5)")?;
        for m in selected {
            insert.execute(params![id, m.id, m.anchor, m.predecessor, m.gap])?;
        }
    }
    let source_watermark = db.query_row(
        "SELECT COALESCE(MAX(rowid),0) FROM logical_requests",
        [],
        |r| r.get(0),
    )?;
    let mut s = Scope {
        id,
        binding,
        workspace: reader.workspace().to_string(),
        created_ms: now,
        expires_ms: expiry,
        range,
        r,
        e: None,
        p: None,
        basis: q.basis,
        currency: q.currency.clone(),
        valuation_kind: q.valuation_kind,
        reference: q.reference_binding_id.clone(),
        plan: q.plan_id.clone(),
        agent: q.agent_id.clone(),
        initial_session: q.session_id.clone(),
        initial_request: q.request_id.clone(),
        keyword_digest: q
            .query
            .as_ref()
            .map(|k| digest(&crate::text_index::fold(k.trim()).0))
            .transpose()?,
        used_edges,
        used_roots,
        source_count: 0,
        source_digest: String::new(),
        source_watermark,
        index_partial: matching.is_some_and(|(_, partial)| partial),
    };
    (s.source_count, s.source_digest) = source_population(db, &s)?;
    save(db, &s)?;
    Ok(s)
}

fn source_population(db: &Connection, s: &Scope) -> Result<(u64, String), Error> {
    use sha2::{Digest, Sha256};
    let mut statement=db.prepare("SELECT r.request_id,r.session_id,r.started_at_ms FROM logical_requests r
      JOIN sessions sessions ON sessions.workspace_id=r.workspace_id AND sessions.session_id=r.session_id
      JOIN observation_analysis_members_v1 m ON m.scope_id=?1 AND m.session_id=r.session_id
      LEFT JOIN valuation_requests_v2 v ON v.workspace_id=r.workspace_id AND v.request_id=r.request_id
      WHERE r.workspace_id=?2 AND r.started_at_ms>=?3 AND r.started_at_ms<?4 AND (?5 IS NULL OR v.plan_id=?5) AND (?6 IS NULL OR sessions.agent_id=?6)
        AND (?7 IS NULL OR r.request_id=?7) AND r.rowid<=?8 ORDER BY r.request_id")?;
    let mut rows = statement.query(params![
        s.id,
        s.workspace,
        s.range.effective_from_ms,
        s.range.effective_to_ms,
        s.plan,
        s.agent,
        s.initial_request,
        s.source_watermark
    ])?;
    let mut count = 0;
    let mut hash = Sha256::new();
    while let Some(row) = rows.next()? {
        for column in [0, 1] {
            let rusqlite::types::ValueRef::Text(v) = row.get_ref(column)? else {
                return Err(Error::Unavailable);
            };
            hash.update((v.len() as u64).to_be_bytes());
            hash.update(v);
        }
        hash.update(row.get::<_, i64>(2)?.to_be_bytes());
        count += 1;
    }
    Ok((count, format!("{:x}", hash.finalize())))
}
pub(super) fn check_live(db: &Connection, s: &Scope) -> Result<(), Error> {
    check_relations(db, s)?;
    if source_population(db, s)? != (s.source_count, s.source_digest.clone()) {
        return Err(Error::Stale);
    }
    Ok(())
}
fn check_relations(db: &Connection, s: &Scope) -> Result<(), Error> {
    for (root, proof) in &s.used_roots {
        let present:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM observation_native_roots_v1 p JOIN logical_requests source ON source.workspace_id=p.workspace_id AND source.request_id=p.request_id WHERE p.workspace_id=?1 AND p.session_id=?2 AND p.proof_id=?3)",params![s.workspace,root,proof],|r|r.get(0))?;
        if !present {
            return Err(Error::Stale);
        }
    }
    for rev in &s.used_edges {
        let edge:Option<(String,String,String)>=db.query_row("SELECT parent_id,child_id,request_id FROM observation_native_relations_v1 WHERE workspace_id=?1 AND revision=?2",params![s.workspace,rev],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((parent, child, request)) = edge else {
            return Err(Error::Stale);
        };
        if !members::edge_usable(
            db,
            &s.workspace,
            &parent,
            &child,
            &request,
            s.expires_ms
                .saturating_sub(crate::managed_text::RETENTION_MS)
                .saturating_add(1),
        )? {
            return Err(Error::Stale);
        }
        let invalid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM observation_native_relations_v1 e JOIN observation_native_relations_v1 old
          ON old.workspace_id=e.workspace_id AND old.parent_id=e.parent_id AND old.child_id=e.child_id
          WHERE old.workspace_id=?1 AND old.revision=?2 AND e.revision>?2 AND e.active=0)",params![s.workspace,rev],|r|r.get(0))?;
        if invalid {
            return Err(Error::Stale);
        }
    }
    Ok(())
}
