//! A bounded projection of existing facts. Cursors retain a snapshot while paging and
//! advance only after every changed turn has been delivered.
mod projection;
mod read;

use super::{ObservationReaderContext, ObservationV2Error, QueryDeadline, identifier};
use hiroute_domain::*;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
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
}
struct Projected {
    view: RoutingTimelineTurnV1,
    position: Position,
}

impl crate::LocalObservationStore {
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
                if cursor.schema != 1
                    || cursor.binding != binding
                    || cursor.visibility != visibility
                {
                    return Err(ObservationV2Error::Stale);
                }
                cursor
            }
            None => Cursor {
                schema: 1,
                binding,
                visibility,
                delivered: Position::default(),
                snapshot: None,
                after: None,
            },
        };
        let target = match cursor.snapshot {
            Some(value) => value,
            None => read::position(&tx)?,
        };
        let (requests, facts) = read::read(&tx, reader, &query.session_id, target, now_ms)?;
        let mut turns = projection::project(requests, &facts)?;
        if started.elapsed() > std::time::Duration::from_millis(500) {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        turns.sort_by(|a, b| {
            (a.view.started_at_ms, &a.view.turn_id).cmp(&(b.view.started_at_ms, &b.view.turn_id))
        });
        let mut changed: Vec<_> = turns
            .into_iter()
            .filter(|turn| {
                turn.position.newer_than(cursor.delivered)
                    && cursor.after.as_ref().is_none_or(|after| {
                        (turn.view.started_at_ms, &turn.view.turn_id) > (after.0, &after.1)
                    })
            })
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
        let turns = changed.into_iter().map(|turn| turn.view).collect();
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
        Ok(result)
    }
}
