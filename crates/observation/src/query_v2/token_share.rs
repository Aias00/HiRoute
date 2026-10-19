//! Frozen candidate attribution and retained, normalized attempt usage, in one read snapshot.
use super::ObservationV2Error as Error;
use crate::valuation::{RawUsage, UsageBucketsV2, normalize_semantics};
use hiroute_domain::*;
use rusqlite::{Transaction, named_params};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn aggregate(
    tx: &Transaction<'_>,
    reader: &ObservationReaderContext,
    query: &ObservationValueQueryV2,
    scope: &ObservationTokenShareReadV1,
    now: i64,
) -> Result<ObservationTokenShareV1, Error> {
    let retained = query.from_ms.max(
        now.saturating_sub(crate::managed_text::RETENTION_MS)
            .saturating_add(1),
    );
    let runs = reader
        .allowed_runs()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| Error::Invalid)?;
    // The small catalog excludes content-bearing facts before opening a carrier. Only
    // a matching executed candidate, binding, profile and frozen publication can classify
    // an attempt. Evaluated-but-unused candidates and current source metadata cannot.
    let sql = "WITH selected AS MATERIALIZED (
      SELECT r.*,v.plan_id FROM logical_requests r
      JOIN sessions s ON s.workspace_id=r.workspace_id AND s.session_id=r.session_id
      LEFT JOIN valuation_requests_v2 v ON v.workspace_id=r.workspace_id AND v.request_id=r.request_id
      LEFT JOIN observation_run_links l ON l.workspace_id=r.workspace_id AND l.request_id=r.request_id
      WHERE r.workspace_id=:workspace AND r.started_at_ms>=:from AND r.started_at_ms<:to
        AND r.traffic_kind='normal' AND (s.tombstone_reason IS NULL OR s.content_completeness='deleted')
        AND (:session IS NULL OR r.session_id=:session) AND (:plan IS NULL OR v.plan_id=:plan)
        AND (:runs IS NULL OR (l.conflicted=0 AND l.run_id IN(SELECT value FROM json_each(:runs))))
        AND (:agent IS NULL OR CASE WHEN l.conflicted=0 THEN json_extract(l.body_json,'$.harness_id')
          WHEN l.request_id IS NULL THEN s.agent_id END=:agent)
    ), frozen AS MATERIALIZED (
      SELECT e.request_id,e.sequence,p.body_json AS body FROM selected r
      JOIN execution_fact_events e ON e.workspace_id=r.workspace_id AND e.request_id=r.request_id
      LEFT JOIN observation_safe_facts_v2 f ON f.workspace_id=e.workspace_id AND f.request_id=e.request_id AND f.original_digest=e.envelope_digest
      JOIN observation_sensitive_payloads_v2 p ON p.id='fact:'||e.envelope_digest
      WHERE (f.body_json IS NULL OR json_extract(f.body_json,'$.event_kind') IN ('attempt_started','candidate_decision'))
        AND octet_length(p.body_json)<=65536
        AND json_extract(p.body_json,'$.fact.kind') IN ('attempt_started','candidate_decision')
    ), started AS MATERIALIZED (
      SELECT request_id,sequence,body,json_extract(body,'$.fact.ordinal') AS ordinal FROM frozen
      WHERE json_extract(body,'$.fact.kind')='attempt_started'
    ) SELECT r.request_id,r.session_id,i.ordinal,i.usage_json,i.pricing_json,
      CASE WHEN t.membership_conflict=0 THEN json_extract(q.branch_execution_json,'$.group') END,
      CASE WHEN t.membership_conflict=0 THEN t.agent_turn_id END,
      (SELECT json_extract(c.body,'$.fact.cost_class') FROM frozen c JOIN started a ON a.request_id=c.request_id
        WHERE a.request_id=r.request_id AND a.ordinal=i.ordinal AND c.sequence<=a.sequence
          AND json_extract(c.body,'$.fact.kind')='candidate_decision'
          AND json_extract(c.body,'$.fact.candidate_id')=json_extract(a.body,'$.fact.candidate_id')
          AND json_extract(c.body,'$.fact.stable_binding_id')=json_extract(a.body,'$.fact.stable_binding_id')
          AND json_extract(c.body,'$.fact.profile_digest')=json_extract(a.body,'$.fact.profile_digest')
          AND json_extract(c.body,'$.trust.gateway_publication_digest')=json_extract(a.body,'$.trust.gateway_publication_digest')
        ORDER BY c.sequence DESC LIMIT 1)
      FROM selected r
      LEFT JOIN valuation_attempt_inputs_v2 i ON i.workspace_id=r.workspace_id AND i.request_id=r.request_id
      LEFT JOIN observation_analysis_turns_v1 t ON t.workspace_id=r.workspace_id AND t.request_id=r.request_id
      LEFT JOIN plan_quality_segments q ON q.workspace_id=t.workspace_id AND q.session_id=r.session_id
        AND q.segment_id=t.segment_id AND q.plan_id=r.plan_id";
    let mut billing: BTreeMap<String, ObservationBillingTokensV1> =
        ["subscription", "paid", "free", "unknown"]
            .into_iter()
            .map(|key| (key.into(), Default::default()))
            .collect();
    let mut groups: BTreeMap<String, ObservationGroupTokensV1> = ["regular", "primary", "other"]
        .into_iter()
        .map(|key| (key.into(), Default::default()))
        .collect();
    let mut turns = BTreeSet::new();
    let mut unknown_usage = 0u64;
    let mut known_subscription = 0u64;
    let mut statement = tx.prepare(sql)?;
    let rows = statement.query_map(
        named_params! {":workspace":reader.workspace().as_str(),":from":retained,":to":query.to_ms,
        ":session":query.session_id,":plan":query.plan_id,":runs":runs,":agent":scope.agent_filter},
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<u32>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        },
    )?;
    for row in rows {
        let (_request, session, _ordinal, usage, pricing, group, turn, class) = row?;
        let class = match class.as_deref() {
            Some("subscription") => "subscription",
            Some("paid") => "paid",
            Some("free") => "free",
            _ => "unknown",
        };
        let group = match group.as_deref() {
            Some("regular") => "regular",
            Some("primary") => "primary",
            _ => "other",
        };
        let bucket = billing.get_mut(class).ok_or(Error::Unavailable)?;
        add(&mut bucket.requests, 1)?;
        let values = if let Some(body) = usage {
            if class == "subscription" {
                add(&mut known_subscription, 1)?;
            }
            let raw: RawUsage = serde_json::from_str(&body).map_err(|_| Error::Unavailable)?;
            let pricing: Option<ExecutionPricingEvidenceV1> = pricing
                .map(|body| serde_json::from_str(&body))
                .transpose()
                .map_err(|_| Error::Unavailable)?;
            normalize_semantics(pricing.map(|p| p.usage_semantics).unwrap_or_default(), &raw)
        } else {
            add(&mut unknown_usage, 1)?;
            UsageBucketsV2::default()
        };
        add_tokens(&mut bucket.tokens, &values)?;
        let group_bucket = groups.get_mut(group).ok_or(Error::Unavailable)?;
        add_tokens(&mut group_bucket.tokens, &values)?;
        if let Some(turn) = turn
            && turns.insert((session, turn, group.to_owned()))
        {
            add(&mut group_bucket.turns, 1)?;
        }
    }
    let headline = headline(&billing, known_subscription, unknown_usage)?;
    Ok(ObservationTokenShareV1 {
        basis: "input_uncached+cache_read+cache_write+output".into(),
        period: ObservationTokenSharePeriodV1 { kind:scope.kind,timezone:scope.timezone.clone(),start_ms:query.from_ms,end_ms:query.to_ms },
        agent_filter: scope.agent_filter.clone(), by_billing_class:billing,by_group:groups,
        unknown_usage_requests:unknown_usage,retention_boundary_partial:retained>query.from_ms,headline,
        disclosure: "Token counts across models are approximate, not quota, cost or savings. Only recorded, mutually exclusive components are counted; missing usage is excluded and unknown billing classes are separate. This view uses retained request details; expired details cannot be reconstructed from monetary archives.".into(),
    })
}

fn add(total: &mut u64, value: u64) -> Result<(), Error> {
    *total = total.checked_add(value).ok_or(Error::Unavailable)?;
    Ok(())
}
fn add_tokens(
    total: &mut ObservationTokenComponentsV1,
    usage: &UsageBucketsV2,
) -> Result<(), Error> {
    for (target, value) in [
        (&mut total.input, usage.input_uncached),
        (&mut total.cache_read, usage.cache_read),
        (&mut total.cache_write, usage.cache_write),
        (&mut total.output, usage.output),
    ] {
        if let Some(value) = value {
            add(target, value)?;
            add(&mut total.total, value)?;
        }
    }
    Ok(())
}

fn headline(
    classes: &BTreeMap<String, ObservationBillingTokensV1>,
    known_subscription: u64,
    unknown_usage_requests: u64,
) -> Result<ObservationTokenShareHeadlineV1, Error> {
    use ObservationTokenShareHeadlineV1::*;
    let count = classes
        .values()
        .try_fold(0u64, |n, v| n.checked_add(v.requests))
        .ok_or(Error::Unavailable)?;
    if count == 0 {
        return Ok(NoRoutedRequests);
    }
    let subscription = &classes["subscription"];
    let unknown = &classes["unknown"];
    if subscription.requests == 0 && unknown.requests == 0 {
        return Ok(NoSubscriptionInPeriod);
    }
    if subscription.requests > 0 && known_subscription == 0 {
        return Ok(SubscriptionUsageUnknown {
            unknown_usage_requests,
        });
    }
    let outside = classes["paid"]
        .tokens
        .total
        .checked_add(classes["free"].tokens.total)
        .ok_or(Error::Unavailable)?;
    let known = outside
        .checked_add(subscription.tokens.total)
        .ok_or(Error::Unavailable)?;
    if known == 0 {
        return Ok(NoComparableUsage);
    }
    Ok(Share {
        outside_subscription_ratio_millis: ((u128::from(outside) * 1000 + u128::from(known) / 2)
            / u128::from(known)) as u32,
        known_classified_tokens: known,
        unknown_usage_requests,
    })
}
