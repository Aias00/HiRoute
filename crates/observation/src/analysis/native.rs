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
        let body = serde_json::to_string(e).map_err(|_| Error::Invalid)?;
        if body.len() > 32 * 1024 {
            return Err(Error::Invalid);
        }
        let mut db = self.connection.lock();
        if !schema::available(&db) {
            return Err(Error::Unavailable);
        }
        let tx = db.transaction()?;
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
        tx.execute(
            "INSERT INTO observation_native_events_v1 VALUES(?1,?2,?3)",
            params![e.workspace_id, e.event_id, body],
        )?;
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
    // Native evidence is retained only as opaque cut metadata after deletion.
    db.execute("DELETE FROM observation_native_events_v1 WHERE workspace_id=?1 AND (json_extract(body,'$.current_session_id')=?2 OR EXISTS(SELECT 1 FROM json_each(json_extract(body,'$.ids')) WHERE value=?2))",params![w,session])?;
    Ok(())
}
