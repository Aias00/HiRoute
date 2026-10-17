use super::*;
use rusqlite::{Transaction, named_params};

pub(super) fn position(tx: &Transaction<'_>) -> Result<Position, ObservationV2Error> {
    Ok(Position {
        facts: tx.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM execution_fact_events",
            [],
            |r| r.get(0),
        )?,
        values: tx.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM valuation_records_v2",
            [],
            |r| r.get(0),
        )?,
        links: tx.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM observation_link_events",
            [],
            |r| r.get(0),
        )?,
    })
}

pub(super) fn read(
    tx: &Transaction<'_>,
    reader: &ObservationReaderContext,
    session: &str,
    target: Position,
    now: i64,
) -> Result<(BTreeMap<String, Request>, Vec<Fact>), ObservationV2Error> {
    let runs = reader
        .allowed_runs()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| ObservationV2Error::Invalid)?;
    let retained = now
        .saturating_sub(crate::managed_text::RETENTION_MS)
        .saturating_add(1)
        .max(0);
    let mut requests = BTreeMap::new();
    let mut statement = tx.prepare("SELECT r.request_id,r.started_at_ms,r.finished_at_ms,r.outcome,CASE WHEN length(CAST(l.body_json AS BLOB))<=1048576 THEN l.body_json END,l.conflicted,
        COALESCE((SELECT MAX(e.rowid) FROM observation_link_events e WHERE e.workspace_id=r.workspace_id AND e.request_id=r.request_id AND e.rowid<=:links),0),
        (SELECT CASE WHEN length(CAST(v.body_json AS BLOB))<=1048576 THEN v.body_json END FROM valuation_records_v2 v WHERE v.workspace_id=r.workspace_id AND v.request_id=r.request_id AND v.rowid<=:values ORDER BY v.revision DESC LIMIT 1),
        COALESCE((SELECT MAX(v.rowid) FROM valuation_records_v2 v WHERE v.workspace_id=r.workspace_id AND v.request_id=r.request_id AND v.rowid<=:values),0),
        r.traffic_kind, l.run_id,
        COALESCE(length(CAST(l.body_json AS BLOB)),0)>1048576 OR COALESCE((SELECT length(CAST(v.body_json AS BLOB)) FROM valuation_records_v2 v WHERE v.workspace_id=r.workspace_id AND v.request_id=r.request_id AND v.rowid<=:values ORDER BY v.revision DESC LIMIT 1),0)>1048576
        FROM logical_requests r JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
        LEFT JOIN observation_run_links l ON l.workspace_id=r.workspace_id AND l.request_id=r.request_id
        WHERE r.workspace_id=:workspace AND r.session_id=:session AND r.started_at_ms>=:retained
          AND (s.tombstone_reason IS NULL OR EXISTS(SELECT 1 FROM observation_tombstones t WHERE t.workspace_id=s.workspace_id AND t.session_id=s.session_id AND t.delete_scope='content_only'))
          AND (:runs IS NULL OR (l.conflicted=0 AND l.run_id IN(SELECT value FROM json_each(:runs))))
          AND EXISTS(SELECT 1 FROM execution_fact_events e WHERE e.workspace_id=r.workspace_id AND e.request_id=r.request_id AND e.rowid<=:facts)
        ORDER BY r.started_at_ms,r.request_id LIMIT 10001")?;
    let mut bytes = 0usize;
    let rows = statement.query_map(named_params! { ":workspace":reader.workspace().as_str(), ":session":session, ":retained":retained, ":runs":runs, ":facts":target.facts, ":values":target.values, ":links":target.links }, |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<bool>>(5)?,r.get::<_,i64>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,i64>(8)?,r.get::<_,String>(9)?,r.get::<_,Option<String>>(10)?,r.get::<_,bool>(11)?)))?;
    for row in rows {
        let (
            id,
            start,
            finish,
            outcome,
            link,
            conflicted,
            link_position,
            valuation,
            value_position,
            traffic,
            recorded_run,
            oversized,
        ) = row?;
        if oversized {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        bytes += link.as_ref().map_or(0, String::len) + valuation.as_ref().map_or(0, String::len);
        if bytes > 16 * 1024 * 1024 || requests.len() >= 10_000 {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        let link = link
            .and_then(|s| serde_json::from_str::<RunObservationLink>(&s).ok())
            .filter(|l| {
                conflicted == Some(false)
                    && l.workspace_id == *reader.workspace()
                    && l.request_id.as_str() == id
                    && recorded_run.as_deref() == Some(l.run_id.as_str())
                    && ObservationRoutingContextV1::recorded(
                        l.plan_id.clone(),
                        l.plan_revision.clone(),
                        None,
                    )
                    .valid()
            });
        let costs = if traffic == "normal" {
            costs(valuation.as_deref())?
        } else {
            vec![]
        };
        requests.insert(
            id.clone(),
            Request {
                view: RoutingTimelineRequestV1 {
                    request_id: id,
                    started_at_ms: start,
                    duration_ms: finish.and_then(|end| u64::try_from(end - start).ok()),
                    native_model: None,
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_read_share: None,
                    costs,
                    outcome,
                },
                position: Position {
                    facts: 0,
                    values: value_position,
                    links: link_position,
                },
                first_fact: i64::MAX,
                native_turn: link.as_ref().and_then(|l| l.native_turn_id.clone()),
                native_session: link.as_ref().and_then(|l| l.native_session_id.clone()),
                run_id: link.map(|l| l.run_id.to_string()),
                decision: None,
                candidate: None,
                models: BTreeMap::new(),
                partial: conflicted == Some(true),
            },
        );
    }
    let mut statement = tx.prepare("SELECT e.rowid,e.request_id,e.event_id,CASE WHEN length(CAST(p.body_json AS BLOB))<=1048576 THEN json_extract(p.body_json,'$.fact') END,COALESCE(length(CAST(p.body_json AS BLOB)),0)>1048576
        FROM execution_fact_events e LEFT JOIN observation_sensitive_payloads_v2 p ON p.id='fact:'||e.envelope_digest
        WHERE e.workspace_id=?1 AND e.session_id=?2 AND e.rowid<=?3 ORDER BY e.rowid LIMIT 50001")?;
    let rows = statement.query_map(
        rusqlite::params![reader.workspace().as_str(), session, target.facts],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, bool>(4)?,
            ))
        },
    )?;
    let mut facts = Vec::new();
    for (index, row) in rows.enumerate() {
        let (position, request_id, event_id, value, oversized) = row?;
        if index >= 50_000 {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        let Some(request) = requests.get_mut(&request_id) else {
            continue;
        };
        if oversized {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        request.position.facts = position;
        request.first_fact = request.first_fact.min(position);
        let Some(value) = value else {
            request.partial = true;
            continue;
        };
        bytes += value.len();
        if bytes > 32 * 1024 * 1024 {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        facts.push(Fact {
            position,
            event_id,
            request_id,
            value: serde_json::from_str(&value).map_err(|_| ObservationV2Error::Unavailable)?,
        });
    }
    // Reuse the valuation owner's cumulative/delta normalization. Unknown usage
    // remains unknown rather than silently turning a partial sum into a total.
    let mut statement = tx.prepare("SELECT i.request_id,CASE WHEN length(CAST(i.usage_json AS BLOB))<=65536 THEN i.usage_json END,COALESCE(length(CAST(i.usage_json AS BLOB)),0)>65536 FROM valuation_attempt_inputs_v2 i JOIN logical_requests r ON r.workspace_id=i.workspace_id AND r.request_id=i.request_id WHERE r.workspace_id=?1 AND r.session_id=?2 ORDER BY i.request_id,i.ordinal LIMIT 50001")?;
    let rows = statement.query_map(
        rusqlite::params![reader.workspace().as_str(), session],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, bool>(2)?,
            ))
        },
    )?;
    let mut usage: BTreeMap<String, [Option<u64>; 3]> = BTreeMap::new();
    for (index, row) in rows.enumerate() {
        if index >= 50_000 {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        let (id, body, oversized) = row?;
        if !requests.contains_key(&id) {
            continue;
        }
        if oversized {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        bytes += body.as_ref().map_or(0, String::len);
        if bytes > 32 * 1024 * 1024 {
            return Err(ObservationV2Error::ScopeTooLarge);
        }
        let value = body.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        let sums = usage.entry(id).or_insert([Some(0); 3]);
        for (sum, key) in sums.iter_mut().zip(["input", "output", "read"]) {
            *sum = sum
                .zip(value.as_ref().and_then(|v| v.get(key)?.as_u64()))
                .and_then(|(a, b)| a.checked_add(b));
        }
    }
    for (id, [input, output, read]) in usage {
        let request = requests
            .get_mut(&id)
            .ok_or(ObservationV2Error::Unavailable)?;
        request.view.input_tokens = input;
        request.view.output_tokens = output;
        request.view.cache_read_tokens = read;
        request.view.cache_read_share = input
            .zip(read)
            .filter(|(i, r)| *i > 0 && r <= i)
            .map(|(i, r)| r as f64 / i as f64);
    }
    Ok((requests, facts))
}

fn costs(value: Option<&str>) -> Result<Vec<RoutingTimelineCostV1>, ObservationV2Error> {
    let Some(value) = value else {
        return Ok(vec![unknown(1)]);
    };
    let record: crate::valuation::ValuationRecordV2 =
        serde_json::from_str(value).map_err(|_| ObservationV2Error::Unavailable)?;
    let mut result = Vec::new();
    for attempt in record.attempts {
        result.push(RoutingTimelineCostV1 {
            model_configuration_id: Some(match attempt.quote.exact_target.model_identity {
                PriceModelIdentityV1::CatalogModel(id) | PriceModelIdentityV1::LocalModel(id) => id,
            }),
            currency: Some(attempt.currency),
            valuation_kind: Some(
                serde_json::to_value(attempt.valuation_kind)
                    .map_err(|_| ObservationV2Error::Unavailable)?
                    .as_str()
                    .ok_or(ObservationV2Error::Unavailable)?
                    .into(),
            ),
            known_micros: attempt.known_sum_micros,
            unknown_contributions: u64::from(
                attempt.coverage != crate::valuation::MetricCoverage::Complete,
            ),
        });
    }
    if record.unknown_attempt_count > 0 {
        result.push(unknown(record.unknown_attempt_count));
    }
    if result.is_empty() || record.facts_partial || !record.terminal_observed {
        result.push(unknown(1));
    }
    Ok(result)
}
fn unknown(count: u64) -> RoutingTimelineCostV1 {
    RoutingTimelineCostV1 {
        model_configuration_id: None,
        currency: None,
        valuation_kind: None,
        known_micros: None,
        unknown_contributions: count,
    }
}
