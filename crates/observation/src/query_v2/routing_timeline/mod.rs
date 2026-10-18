//! A bounded projection of existing facts. Cursors retain a snapshot while paging and
//! advance only after every changed turn has been delivered.
pub(crate) mod cache;
mod projection;
mod read;

use super::{ObservationReaderContext, ObservationV2Error, QueryDeadline, identifier};
use hiroute_domain::*;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Position {
    facts: i64,
    values: i64,
    links: i64,
}
impl Position {
    fn newer_than(self, other: Self) -> bool {
        self.facts > other.facts || self.values > other.values || self.links > other.links
    }
    fn include(&mut self, other: Self) {
        self.facts = self.facts.max(other.facts);
        self.values = self.values.max(other.values);
        self.links = self.links.max(other.links);
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema: u8,
    binding: String,
    visibility: u64,
    delivered: Position,
    snapshot: Option<Position>,
    after: Option<(i64, String)>,
    expires_at: i64,
}
struct Fact {
    position: i64,
    event_id: String,
    request_id: String,
    value: ExecutionFactV1,
}
struct Request {
    view: RoutingTimelineRequestV1,
    position: Position,
    first_fact: i64,
    native_turn: Option<String>,
    native_session: Option<String>,
    run_id: Option<String>,
    decision: Option<RouteDecisionFactV1>,
    candidate: Option<(String, Option<String>)>,
    models: BTreeMap<u32, String>,
    partial: bool,
    body_bytes: usize,
    fact_count: usize,
    usage_count: usize,
}
#[derive(Clone)]
struct Projected {
    view: RoutingTimelineTurnV1,
    position: Position,
}

impl crate::LocalObservationStore {
    #[cfg(test)]
    pub(crate) fn timeline_work(&self) -> (usize, usize) {
        let cache = self.timeline_cache.lock();
        (
            cache.entries.values().map(|s| s.read_requests).sum(),
            cache.entries.values().map(|s| s.projected_turns).sum(),
        )
    }

    pub fn observed_routing_timeline(
        &self,
        reader: &ObservationReaderContext,
        query: &RoutingTimelineQueryV1,
        now_ms: i64,
    ) -> Result<RoutingTimelinePageV1, ObservationV2Error> {
        reader.check(now_ms, false, false)?;
        if !identifier(&query.session_id) || query.limit == 0 || query.limit > 200 {
            return Err(ObservationV2Error::Invalid);
        }
        let _permit = self.query_permit()?;
        let started = std::time::Instant::now();
        let mut connection =
            Connection::open_with_flags(&self.activity_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let _deadline = QueryDeadline::start_for(&connection, reader)?;
        let tx = connection.transaction()?;
        let visibility: u64 = tx.query_row("SELECT CAST(value AS INTEGER) FROM observation_meta WHERE key='query_visibility_generation'", [], |r| r.get(0))?;
        let binding = CanonicalDigest::of(&(reader.binding()?, &query.session_id, query.limit))
            .map_err(|_| ObservationV2Error::Invalid)?
            .to_string();
        let mut cursor = match &query.after_watermark {
            Some(value) => {
                let cursor: Cursor = self.decode_observation_cursor(value)?;
                if cursor.schema != 2
                    || cursor.binding != binding
                    || cursor.visibility != visibility
                    || now_ms >= cursor.expires_at
                {
                    return Err(ObservationV2Error::Stale);
                }
                cursor
            }
            None => Cursor {
                schema: 2,
                binding,
                visibility,
                delivered: Position::default(),
                snapshot: None,
                after: None,
                expires_at: i64::MAX,
            },
        };
        let target = match cursor.snapshot {
            Some(value) => value,
            None => read::position(&tx)?,
        };
        let mut caches = self
            .timeline_cache
            .try_lock()
            .ok_or(ObservationV2Error::Busy)?;
        let key = cursor.binding.clone();
        let cached = caches
            .entries
            .remove(&key)
            .filter(|c| c.visibility == visibility && now_ms < c.expires_at);
        let mut state = cached.unwrap_or_default();
        let frozen = cursor.snapshot.is_some();
        // A frozen page must never use the no-change shortcut: it still owes rows.
        if state.position != Some(target) || query.after_watermark.is_none() {
            let can_update = query.after_watermark.is_some()
                && !frozen
                && state.position.is_some_and(|p| p == cursor.delivered);
            let changed = if can_update {
                Some(read::changed_requests(
                    &tx,
                    reader,
                    &query.session_id,
                    cursor.delivered,
                    target,
                )?)
            } else {
                None
            };
            state.refresh(
                &tx,
                reader,
                &query.session_id,
                target,
                now_ms,
                changed.as_ref(),
            )?;
        }
        state.visibility = visibility;
        cursor.expires_at = state.expires_at;
        if started.elapsed() > std::time::Duration::from_millis(500) {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        let start = cursor.after.as_ref().map_or(0, |after| {
            state.turns.partition_point(|turn| {
                (turn.view.started_at_ms, &turn.view.turn_id) <= (after.0, &after.1)
            })
        });
        let mut changed: Vec<_> = state.turns[start..]
            .iter()
            .filter(|turn| turn.position.newer_than(cursor.delivered))
            .take(usize::from(query.limit) + 1)
            .cloned()
            .collect();
        let complete = changed.len() <= usize::from(query.limit);
        changed.truncate(usize::from(query.limit));
        if complete {
            cursor.delivered = target;
            cursor.snapshot = None;
            cursor.after = None;
        } else {
            let last = changed.last().ok_or(ObservationV2Error::Unavailable)?;
            cursor.snapshot = Some(target);
            cursor.after = Some((last.view.started_at_ms, last.view.turn_id.clone()));
        }
        let turns = changed.into_iter().map(|turn| turn.view.clone()).collect();
        let result = RoutingTimelinePageV1 {
            turns,
            watermark: self.encode_observation_cursor(&cursor)?,
            complete,
        };
        if serde_json::to_vec(&result)
            .map_err(|_| ObservationV2Error::Unavailable)?
            .len()
            > 1024 * 1024
        {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        tx.commit()?;
        super::check_visibility(&connection, visibility)?;
        caches.insert(key, state);
        Ok(result)
    }
}
