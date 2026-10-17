//! Thin ordered request metadata first; hydrate only the selected page through
//! the existing receipt/content and PlanQuality owners.
use super::{scope, scope::Scope};
use crate::{LocalObservationStore, query_v2::ObservationV2Error as Error};
use hiroute_domain::*;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;

pub(super) struct Row {
    pub request: AnalysisRequestV1,
    pub anchor: String,
    pub predecessor: Option<String>,
    pub gap: Option<String>,
    pub turn: Option<String>,
    pub ordinal: Option<u64>,
    pub epoch: Option<String>,
    pub turn_partial: bool,
    pub terminal: bool,
}

pub(super) fn rows(
    db: &Connection,
    s: &Scope,
    q: &ObservationAnalysisQueryV1,
) -> Result<Vec<Row>, Error> {
    let sql="SELECT r.session_id,r.request_id,r.started_at_ms,r.finished_at_ms,r.outcome,r.receipt_id,
      m.anchor_id,m.predecessor,m.gap,s.agent_id,
      COALESCE(a.native_model,(SELECT model_id FROM observation_attempt_models_v2 am WHERE am.workspace_id=r.workspace_id AND am.request_id=r.request_id AND am.ordinal=v.accepted_ordinal)),
      a.model_id,a.reasoning_profile_id,COALESCE(v.terminal,0),
      (SELECT COUNT(DISTINCT model_id) FROM observation_attempt_models_v2 am WHERE am.workspace_id=r.workspace_id AND am.request_id=r.request_id),
      t.agent_turn_id,t.ordinal,t.producer_epoch,COALESCE(t.partial,1),
      (SELECT f.body_json FROM observation_safe_facts_v2 f WHERE f.workspace_id=r.workspace_id AND f.request_id=r.request_id ORDER BY f.rowid LIMIT 1)
      FROM logical_requests r JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
      JOIN observation_analysis_members_v1 m ON m.scope_id=?1 AND m.session_id=r.session_id
      LEFT JOIN valuation_requests_v2 v ON v.workspace_id=r.workspace_id AND v.request_id=r.request_id
      LEFT JOIN observation_analysis_requests_v1 a ON a.workspace_id=r.workspace_id AND a.request_id=r.request_id AND a.revision=(SELECT MAX(new.revision) FROM observation_analysis_requests_v1 new WHERE new.workspace_id=r.workspace_id AND new.request_id=r.request_id)
      LEFT JOIN observation_analysis_turns_v1 t ON t.workspace_id=r.workspace_id AND t.request_id=r.request_id
      WHERE r.workspace_id=?2 AND r.started_at_ms>=?3 AND r.started_at_ms<?4 AND r.rowid<=?5
       AND (?6 IS NULL OR v.plan_id=?6) AND (?7 IS NULL OR s.agent_id=?7) AND (?8 IS NULL OR r.request_id=?8)
       AND (?9 IS NULL OR m.anchor_id=?9 OR m.anchor_id=(SELECT anchor_id FROM observation_analysis_members_v1 WHERE scope_id=?1 AND session_id=?9))
      ORDER BY r.started_at_ms,r.request_id";
    let mut stmt = db.prepare(sql)?;
    Ok(stmt
        .query_map(
            params![
                s.id,
                s.workspace,
                s.range.effective_from_ms,
                s.range.effective_to_ms,
                s.source_watermark,
                s.plan,
                s.agent,
                s.initial_request,
                q.session_id
            ],
            |r| {
                let count: u32 = r.get(14)?;
                let model = AnalysisModelV1 {
                    native_model: r.get(10)?,
                    model_configuration_id: r.get(11)?,
                    reasoning_profile_id: r.get(12)?,
                };
                let safe: Option<String> = r.get(19)?;
                let context = safe
                    .and_then(|s| serde_json::from_str::<ObservationSafeFactV2>(&s).ok())
                    .and_then(|f| f.routing_context)
                    .filter(|c| c.valid())
                    .unwrap_or_else(ObservationRoutingContextV1::unavailable);
                Ok(Row {
                    request: AnalysisRequestV1 {
                        request: ObservedRequestSummary {
                            session_id: r.get(0)?,
                            request_id: r.get(1)?,
                            started_at_ms: r.get(2)?,
                            outcome: r.get(4)?,
                            receipt_id: r.get(5)?,
                            run_id: None,
                            parent_context_ref: None,
                            native_turn_id: r.get(15)?,
                            relation_conflicted: false,
                            routing_context: context,
                            attempted_model_count: count,
                            within_request_fallback: (count > 0).then_some(count > 1),
                            final_native_model: model.native_model.clone(),
                            between_turn_model_change: None,
                            previous_native_turn_id: None,
                            previous_turn_model: None,
                            turn_final_native_model: None,
                        },
                        model,
                        finished_at_ms: r.get(3)?,
                        content: vec![],
                        content_partial: false,
                    },
                    anchor: r.get(6)?,
                    predecessor: r.get(7)?,
                    gap: r.get(8)?,
                    turn: r.get(15)?,
                    ordinal: r.get(16)?,
                    epoch: r.get(17)?,
                    turn_partial: r.get(18)?,
                    terminal: r.get(13)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(super) fn conversations(rows: &[Row]) -> Vec<AnalysisConversationV1> {
    let mut groups = BTreeMap::<String, AnalysisConversationV1>::new();
    for r in rows {
        let at = r.request.request.started_at_ms;
        let c = groups
            .entry(r.anchor.clone())
            .or_insert(AnalysisConversationV1 {
                analysis_session_id: r.anchor.clone(),
                members: vec![],
                first_at_ms: at,
                last_at_ms: at,
                request_count: 0,
                lineage_partial: false,
            });
        c.first_at_ms = c.first_at_ms.min(at);
        c.last_at_ms = c.last_at_ms.max(at);
        c.request_count += 1;
        c.lineage_partial |= r.gap.is_some();
        if !c
            .members
            .iter()
            .any(|m| m.session_id == r.request.request.session_id)
        {
            c.members.push(AnalysisMemberV1 {
                session_id: r.request.request.session_id.clone(),
                predecessor_session_id: r.predecessor.clone(),
                gap_before: r.gap.clone(),
            });
        }
    }
    let mut result: Vec<_> = groups.into_values().collect();
    result.sort_by(|a, b| {
        b.last_at_ms
            .cmp(&a.last_at_ms)
            .then_with(|| a.analysis_session_id.cmp(&b.analysis_session_id))
    });
    result
}

pub(super) fn transitions(rows: &[Row]) -> Vec<AnalysisTransitionV1> {
    let mut previous = BTreeMap::<String, &Row>::new();
    let last_by_session: BTreeMap<_, _> = rows
        .iter()
        .map(|r| (r.request.request.session_id.as_str(), r))
        .collect();
    let mut seen_physical = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for r in rows {
        let own = &r.request.request;
        let first_physical = seen_physical.insert(own.session_id.clone());
        let boundary = first_physical && r.predecessor.is_some();
        let prev = if boundary {
            r.predecessor
                .as_deref()
                .and_then(|id| last_by_session.get(id).copied())
        } else {
            previous.get(&own.session_id).copied()
        };
        let known = r.terminal && r.request.model.native_model.is_some();
        let endpoints = prev.is_some_and(|p| {
            p.terminal
                && p.request
                    .finished_at_ms
                    .is_some_and(|at| at < own.started_at_ms)
                && p.request.model.native_model.is_some()
        });
        let changed = if known && endpoints {
            prev.map(|p| p.request.model.native_model != r.request.model.native_model)
        } else {
            None
        };
        let reasoning = prev.and_then(|p| {
            p.request
                .model
                .reasoning_profile_id
                .as_ref()
                .zip(r.request.model.reasoning_profile_id.as_ref())
                .map(|(a, b)| a != b)
        });
        let adjacent = prev.is_some_and(|p| {
            p.epoch.is_some()
                && p.epoch == r.epoch
                && p.ordinal
                    .zip(r.ordinal)
                    .is_some_and(|(a, b)| a.checked_add(1) == Some(b))
                && p.turn != r.turn
                && !p.turn_partial
                && !r.turn_partial
                && endpoints
        });
        let kind = if boundary {
            AnalysisTransitionKindV1::ContinuationBoundary
        } else if !r.terminal {
            AnalysisTransitionKindV1::Unknown
        } else if prev.is_none() && r.ordinal == Some(0) && !r.turn_partial {
            AnalysisTransitionKindV1::InitialSelection
        } else if adjacent && changed == Some(true) {
            AnalysisTransitionKindV1::BetweenTurnChange
        } else if adjacent && reasoning == Some(true) {
            AnalysisTransitionKindV1::ReasoningChange
        } else if adjacent && changed == Some(false) {
            AnalysisTransitionKindV1::Unchanged
        } else if prev.is_some_and(|p| p.turn.is_some() && p.turn == r.turn)
            && changed == Some(false)
        {
            AnalysisTransitionKindV1::Unchanged
        } else {
            AnalysisTransitionKindV1::Unknown
        };
        let completeness = if !r.terminal || prev.is_some_and(|p| !p.terminal) {
            "pending"
        } else if boundary || !adjacent && prev.is_some() {
            "turn_context_reset"
        } else if known && endpoints {
            "complete"
        } else {
            "unknown"
        };
        let transition = AnalysisTransitionV1 {
            kind,
            session_id: own.session_id.clone(),
            request_id: own.request_id.clone(),
            previous_session_id: prev.map(|p| p.request.request.session_id.clone()),
            previous_request_id: prev.map(|p| p.request.request.request_id.clone()),
            at_ms: own.started_at_ms,
            previous_model: prev.map(|p| p.request.model.clone()),
            model: r.request.model.clone(),
            model_changed: changed,
            reasoning_changed: reasoning,
            completeness: completeness.into(),
            receipt_id: own.receipt_id.clone(),
            reason: None,
        };
        result.push(transition.clone());
        if own.within_request_fallback == Some(true) {
            result.push(AnalysisTransitionV1 {
                kind: AnalysisTransitionKindV1::WithinRequestFallback,
                previous_session_id: None,
                previous_request_id: None,
                previous_model: None,
                model_changed: Some(true),
                ..transition
            });
        }
        previous.insert(own.session_id.clone(), r);
    }
    result
}

pub(super) fn content(
    store: &LocalObservationStore,
    db: &Connection,
    reader: &ObservationReaderContext,
    rows: &mut [AnalysisRequestV1],
    now: i64,
) -> Result<(), Error> {
    let mut remaining: usize = 128 * 1024;
    for r in rows {
        let mut stmt=db.prepare("SELECT content_id,message_instance_id,direction,content_kind,canonical_media_type,state FROM content_instances_v2 WHERE workspace_id=?1 AND request_id=?2 AND content_kind NOT IN('provider_state','reasoning_delta','reasoning_finished') ORDER BY rowid LIMIT 17")?;
        let parts = stmt
            .query_map(
                params![reader.workspace().as_str(), r.request.request_id],
                |p| {
                    Ok((
                        p.get::<_, String>(0)?,
                        p.get::<_, String>(1)?,
                        p.get::<_, String>(2)?,
                        p.get::<_, String>(3)?,
                        p.get::<_, String>(4)?,
                        p.get::<_, String>(5)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        r.content_partial = parts.len() > 16;
        for (id, message, direction, kind, media, state) in parts.into_iter().take(16) {
            let mut c = AnalysisContentV1 {
                content_id: id,
                message_occurrence_id: message,
                direction,
                kind,
                media_type: media,
                text: None,
                next_cursor: None,
                completeness: state,
            };
            if remaining >= 128 * 1024 && c.completeness == "complete" {
                let query = ObservationContentQueryV2 {
                    request_id: LogicalRequestId::parse(&r.request.request_id)
                        .map_err(|_| Error::Invalid)?,
                    content_id: c.content_id.clone(),
                    cursor: None,
                    anchor_offset: None,
                };
                match store.observed_content_page_admitted(reader, &query, now, true) {
                    Ok(page) => {
                        let text = page.chunks.into_iter().map(|p| p.text).collect::<String>();
                        remaining = remaining.saturating_sub(text.len());
                        c.text = Some(text);
                        c.next_cursor = page.next_cursor;
                    }
                    Err(Error::Unavailable) => {
                        c.completeness = "unavailable".into();
                        r.content_partial = true;
                    }
                    Err(e) => return Err(e),
                }
            } else {
                r.content_partial = true;
            }
            r.content.push(c);
        }
    }
    Ok(())
}

pub(super) fn paginate<T: Clone>(
    store: &LocalObservationStore,
    s: &Scope,
    q: &ObservationAnalysisQueryV1,
    values: &[T],
    offset: u64,
) -> Result<(Vec<T>, Option<String>), Error> {
    let start = usize::try_from(offset)
        .map_err(|_| Error::Invalid)?
        .min(values.len());
    let end = (start + usize::from(q.limit)).min(values.len());
    Ok((
        values[start..end].to_vec(),
        if end < values.len() {
            Some(scope::next(store, s, q, end as u64)?)
        } else {
            None
        },
    ))
}
