//! Authenticated HMAC-only native evidence. Edge/node ordering, quarantine and
//! deletion are shared by every analysis facet; arrival never renews a proof.
use super::schema;
use crate::{LocalObservationStore, query_v2::ObservationV2Error as Error};
use hiroute_domain::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::cmp::Ordering;

#[derive(Clone)]
struct Prior {
    active: bool,
    observation: NativeProofObservationV1,
}

fn order(a: &NativeProofObservationV1, b: &NativeProofObservationV1) -> Option<Ordering> {
    if a.observer_id == b.observer_id {
        Some(a.sequence.cmp(&b.sequence))
    } else {
        match a.observed_at_ms.cmp(&b.observed_at_ms) {
            Ordering::Equal => None,
            order => Some(order),
        }
    }
}
fn valid_stamp(s: &NativeProofObservationV1) -> bool {
    !s.observer_id.is_empty()
        && s.observer_id.len() <= 128
        && s.sequence > 0
        && s.observed_at_ms >= 0
        && s.age_ms <= 3_600_000
}
fn stamp_key(w: &str, parent: &str, child: &str) -> Result<String, Error> {
    Ok(format!(
        "edge-stamp/{}",
        CanonicalDigest::of(&(w, parent, child)).map_err(|_| Error::Invalid)?
    ))
}
fn latest(db: &Connection, w: &str, parent: &str, child: &str) -> Result<Option<Prior>, Error> {
    let row:Option<(bool,String,String,i64)>=db.query_row("SELECT active,observer_id,sequence,observed_ms FROM observation_native_relations_v1 WHERE workspace_id=?1 AND parent_id=?2 AND child_id=?3 ORDER BY revision DESC LIMIT 1",params![w,parent,child],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let mut prior = row
        .map(|(active, id, seq, at)| {
            Ok::<_, Error>(Prior {
                active,
                observation: NativeProofObservationV1 {
                    observer_id: id,
                    sequence: seq.parse().map_err(|_| Error::Unavailable)?,
                    observed_at_ms: at,
                    age_ms: 0,
                },
            })
        })
        .transpose()?;
    if let Some(p) = &mut prior {
        let extra: Option<String> = db
            .query_row(
                "SELECT value FROM observation_analysis_meta_v1 WHERE key=?1",
                [stamp_key(w, parent, child)?],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(body) = extra {
            p.observation = serde_json::from_str(&body).map_err(|_| Error::Unavailable)?;
        }
    }
    Ok(prior)
}
fn put_edge(
    db: &Connection,
    e: &NativeContinuationEvidenceV1,
    parent: &str,
    child: &str,
    mut active: bool,
    stamp: &NativeProofObservationV1,
) -> Result<(), Error> {
    if let Some(old) = latest(db, &e.workspace_id, parent, child)? {
        match order(stamp, &old.observation) {
            Some(Ordering::Less | Ordering::Equal) => return Ok(()),
            None => {
                if !old.active {
                    return Ok(());
                }
                active = false;
            }
            Some(Ordering::Greater) => {}
        }
        if old.active == active {
            // Keep the latest *observed* stamp without mutating the immutable
            // R row or refreshing its source provenance on a cached replay.
            let body = serde_json::to_string(stamp).map_err(|_| Error::Invalid)?;
            db.execute("INSERT INTO observation_analysis_meta_v1 VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![stamp_key(&e.workspace_id,parent,child)?,body])?;
            return Ok(());
        }
    }
    let revision = schema::advance(db, &format!("relation_revision/{}", e.workspace_id))?;
    let proof = CanonicalDigest::of(&(&stamp.observer_id, stamp.sequence, stamp.observed_at_ms))
        .map_err(|_| Error::Invalid)?
        .to_string();
    db.execute(
        "INSERT INTO observation_native_relations_v1 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            e.workspace_id,
            revision,
            parent,
            child,
            active,
            stamp.observer_id,
            stamp.sequence.to_string(),
            stamp.observed_at_ms,
            e.ingress_at_ms,
            e.request_id,
            proof
        ],
    )?;
    let body = serde_json::to_string(stamp).map_err(|_| Error::Invalid)?;
    db.execute("INSERT INTO observation_analysis_meta_v1 VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![stamp_key(&e.workspace_id,parent,child)?,body])?;
    Ok(())
}

fn current_state(
    db: &Connection,
    e: &NativeContinuationEvidenceV1,
    state: &NativeCurrentStateV1,
) -> Result<(), Error> {
    let prior:Option<(String,String,i64)>=db.query_row("SELECT observer_id,sequence,observed_ms FROM observation_native_states_v1 WHERE workspace_id=?1 AND session_id=?2",params![e.workspace_id,e.current_session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((id, seq, at)) = prior {
        let old = NativeProofObservationV1 {
            observer_id: id,
            sequence: seq.parse().map_err(|_| Error::Unavailable)?,
            observed_at_ms: at,
            age_ms: 0,
        };
        if order(&state.observation, &old) != Some(Ordering::Greater) {
            return Ok(());
        }
    }
    let name = match state.value {
        NativeSessionStateV1::Open => "open",
        NativeSessionStateV1::CompressionClosed => "compression_closed",
        NativeSessionStateV1::OtherClosed => "other_closed",
    };
    db.execute("INSERT INTO observation_native_states_v1 VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(workspace_id,session_id) DO UPDATE SET state=excluded.state,observer_id=excluded.observer_id,sequence=excluded.sequence,observed_ms=excluded.observed_ms",params![e.workspace_id,e.current_session_id,name,state.observation.observer_id,state.observation.sequence.to_string(),state.observation.observed_at_ms])?;
    if state.value != NativeSessionStateV1::CompressionClosed {
        for (parent, child) in incident(db, &e.workspace_id, &e.current_session_id, true)? {
            put_edge(db, e, &parent, &child, false, &state.observation)?;
        }
    }
    Ok(())
}
fn incident(
    db: &Connection,
    w: &str,
    id: &str,
    outgoing: bool,
) -> Result<Vec<(String, String)>, Error> {
    let mut stmt=db.prepare("SELECT e.parent_id,e.child_id FROM observation_native_relations_v1 e WHERE e.workspace_id=?1
      AND (e.parent_id=?2 OR (?3=0 AND e.child_id=?2)) AND e.active=1
      AND e.revision=(SELECT MAX(new.revision) FROM observation_native_relations_v1 new WHERE new.workspace_id=e.workspace_id AND new.parent_id=e.parent_id AND new.child_id=e.child_id)")?;
    Ok(stmt
        .query_map(params![w, id, outgoing], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?)
}

fn state_allows(
    db: &Connection,
    e: &NativeContinuationEvidenceV1,
    parent: &str,
) -> Result<bool, Error> {
    let prior:Option<(String,String,String,i64)>=db.query_row("SELECT state,observer_id,sequence,observed_ms FROM observation_native_states_v1 WHERE workspace_id=?1 AND session_id=?2",params![e.workspace_id,parent],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let Some((state, id, seq, at)) = prior else {
        return Ok(true);
    };
    if state == "compression_closed" {
        return Ok(true);
    }
    let stamp = NativeProofObservationV1 {
        observer_id: id,
        sequence: seq.parse().map_err(|_| Error::Unavailable)?,
        observed_at_ms: at,
        age_ms: 0,
    };
    Ok(order(&e.proof_observation, &stamp) == Some(Ordering::Greater))
}

fn retained(
    db: &Connection,
    input: &NativeContinuationEvidenceV1,
) -> Result<Option<NativeContinuationEvidenceV1>, Error> {
    let mut e = input.clone();
    let mut ids = e.ids.clone();
    if let Some(n) = &e.invalid_edges {
        for pair in &n.edges {
            ids.extend(pair.iter().cloned());
        }
    }
    let json = serde_json::to_string(&ids).map_err(|_| Error::Invalid)?;
    let mut stmt=db.prepare("SELECT c.session_id,c.through_ms FROM observation_analysis_cuts_v1 c WHERE c.workspace_id=?1 AND c.session_id IN(SELECT value FROM json_each(?2))")?;
    let cuts = stmt
        .query_map(params![e.workspace_id, json], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    if cuts
        .get(&e.current_session_id)
        .is_some_and(|through| e.ingress_at_ms <= *through)
    {
        return Ok(None);
    }
    if let Some(index) = e.ids.iter().rposition(|id| cuts.contains_key(id)) {
        let start = (index + 1).min(e.ids.len() - 1);
        e.ids = e.ids[start..].to_vec();
        e.complete = false;
    }
    if let Some(n) = &mut e.invalid_edges {
        n.edges
            .retain(|[p, c]| !cuts.contains_key(p) && !cuts.contains_key(c));
        if n.edges.is_empty() {
            e.invalid_edges = None;
        }
    }
    Ok(Some(e))
}
fn store_event(db: &Connection, e: &NativeContinuationEvidenceV1, body: &str) -> Result<(), Error> {
    db.execute("INSERT INTO observation_native_events_v1 VALUES(?1,?2,?3) ON CONFLICT(workspace_id,event_id) DO UPDATE SET body=excluded.body",params![e.workspace_id,e.event_id,body])?;
    db.execute(
        "DELETE FROM observation_native_event_members_v1 WHERE workspace_id=?1 AND event_id=?2",
        params![e.workspace_id, e.event_id],
    )?;
    let mut ids = e
        .ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(n) = &e.invalid_edges {
        for [p, c] in &n.edges {
            ids.insert(p.clone());
            ids.insert(c.clone());
        }
    }
    for id in ids {
        db.execute(
            "INSERT INTO observation_native_event_members_v1 VALUES(?1,?2,?3)",
            params![e.workspace_id, id, e.event_id],
        )?;
    }
    Ok(())
}

impl LocalObservationStore {
    pub fn ingest_native_continuation(
        &self,
        e: &NativeContinuationEvidenceV1,
    ) -> Result<(), Error> {
        if e.schema_version != NATIVE_CONTINUATION_SCHEMA_V1
            || WorkspaceId::parse(&e.workspace_id).is_err()
            || LogicalRequestId::parse(&e.request_id).is_err()
            || e.ids.is_empty()
            || e.ids.len() > 32
            || e.ids.last() != Some(&e.current_session_id)
            || !valid_stamp(&e.proof_observation)
            || e.ingress_at_ms < 0
            || e.ids.iter().any(|id| SessionId::parse(id).is_err())
            || e.ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != e.ids.len()
        {
            return Err(Error::Invalid);
        }
        if e.current_state
            .as_ref()
            .is_some_and(|s| !valid_stamp(&s.observation) || s.observation.age_ms > 1000)
            || e.invalid_edges
                .as_ref()
                .is_some_and(|n| !valid_stamp(&n.observation) || n.edges.len() > 31)
        {
            return Err(Error::Invalid);
        }
        if serde_json::to_vec(e).map_err(|_| Error::Invalid)?.len() > 32 * 1024 {
            return Err(Error::Invalid);
        }
        let mut db = self.connection.lock();
        if !schema::available(&db) {
            return Err(Error::Unavailable);
        }
        let tx = db.transaction()?;
        let Some(e) = retained(&tx, e)? else {
            return Ok(());
        };
        let e = &e;
        let body = serde_json::to_string(e).map_err(|_| Error::Invalid)?;
        let previous:Option<String>=tx.query_row("SELECT body FROM observation_native_events_v1 WHERE workspace_id=?1 AND event_id=?2",params![e.workspace_id,e.event_id],|r|r.get(0)).optional()?;
        if let Some(previous) = previous {
            if previous == body {
                return Ok(());
            }
            return Err(Error::RelationshipConflict);
        }
        if let Some(state) = &e.current_state {
            current_state(&tx, e, state)?;
        }
        if let Some(negative) = &e.invalid_edges {
            for [parent, child] in &negative.edges {
                put_edge(&tx, e, parent, child, false, &negative.observation)?;
            }
        }
        for pair in e.ids.windows(2) {
            let (parent, child) = (&pair[0], &pair[1]);
            if !state_allows(&tx, e, parent)? {
                continue;
            }
            let mut competitors = incident(&tx, &e.workspace_id, parent, true)?;
            competitors.extend(
                incident(&tx, &e.workspace_id, child, false)?
                    .into_iter()
                    .filter(|(p, c)| c == child && p != parent),
            );
            competitors.retain(|(p, c)| p != parent || c != child);
            if !competitors.is_empty() {
                for (p, c) in competitors {
                    put_edge(&tx, e, &p, &c, false, &e.proof_observation)?;
                }
                put_edge(&tx, e, parent, child, false, &e.proof_observation)?;
            } else {
                put_edge(&tx, e, parent, child, true, &e.proof_observation)?;
            }
        }
        store_event(&tx, e, &body)?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn full_delete(
    db: &Connection,
    w: &str,
    session: &str,
    through: i64,
) -> rusqlite::Result<()> {
    if !schema::available(db) {
        return Ok(());
    }
    for table in [
        "observation_analysis_attempts_v1",
        "observation_analysis_recorded_v1",
        "observation_analysis_requests_v1",
        "observation_analysis_models_v1",
        "observation_analysis_reasoning_v1",
        "observation_analysis_turns_v1",
    ] {
        db.execute(&format!("DELETE FROM {table} WHERE workspace_id=?1 AND request_id IN(SELECT request_id FROM logical_requests WHERE workspace_id=?1 AND session_id=?2)"),params![w,session])?;
    }
    db.execute("INSERT INTO observation_analysis_cuts_v1 VALUES(?1,?2,?3,'full') ON CONFLICT(workspace_id,session_id,kind) DO UPDATE SET through_ms=MAX(through_ms,excluded.through_ms)",params![w,session,through])?;
    let removed=db.execute("DELETE FROM observation_native_relations_v1 WHERE workspace_id=?1 AND (parent_id=?2 OR child_id=?2)",params![w,session])?;
    if removed > 0 {
        schema::advance(db, &format!("relation_revision/{w}"))?;
    }
    db.execute(
        "DELETE FROM observation_native_states_v1 WHERE workspace_id=?1 AND session_id=?2",
        params![w, session],
    )?;
    db.execute("DELETE FROM observation_analysis_members_v1 WHERE session_id=?2 AND scope_id IN(SELECT id FROM observation_analysis_scopes_v1 WHERE workspace_id=?1)",params![w,session])?;
    // Preserve a still-proven suffix after the deleted bridge, while dropping
    // its prefix and reverse references. This is retained observation evidence,
    // never a replacement canonical execution/receipt payload.
    while redact_events(db, w, Some(session), 200).map_err(|_| rusqlite::Error::InvalidQuery)?
        == 200
    {}
    Ok(())
}

pub(super) fn redact_events(
    db: &Connection,
    w: &str,
    session: Option<&str>,
    limit: usize,
) -> Result<usize, Error> {
    let mut stmt=db.prepare("SELECT DISTINCT e.event_id,e.body FROM observation_native_events_v1 e
      JOIN observation_native_event_members_v1 m ON m.workspace_id=e.workspace_id AND m.event_id=e.event_id
      JOIN observation_analysis_cuts_v1 c ON c.workspace_id=m.workspace_id AND c.session_id=m.session_id
      WHERE e.workspace_id=?1 AND (?2 IS NULL OR m.session_id=?2)
        AND (m.session_id!=json_extract(e.body,'$.current_session_id') OR json_extract(e.body,'$.ingress_at_ms')<=c.through_ms OR json_array_length(json_extract(e.body,'$.ids'))>1 OR json_array_length(json_extract(e.body,'$.invalid_edges.edges'))>0)
      ORDER BY e.event_id LIMIT ?3")?;
    let events = stmt
        .query_map(params![w, session, limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut count = 0;
    for (id, body) in events {
        let e: NativeContinuationEvidenceV1 =
            serde_json::from_str(&body).map_err(|_| Error::Unavailable)?;
        if let Some(retained) = retained(db, &e)? {
            let next = serde_json::to_string(&retained).map_err(|_| Error::Unavailable)?;
            if next != body {
                store_event(db, &retained, &next)?;
                count += 1;
            }
        } else {
            db.execute(
                "DELETE FROM observation_native_events_v1 WHERE workspace_id=?1 AND event_id=?2",
                params![w, id],
            )?;
            db.execute("DELETE FROM observation_native_event_members_v1 WHERE workspace_id=?1 AND event_id=?2",params![w,id])?;
            count += 1;
        }
    }
    Ok(count)
}
