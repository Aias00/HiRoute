//! Aggregate the complete retained input before ranking. Parsing numeric token
//! columns never reparses per-attempt quote graphs or changes their rounding.
use super::{comparison::RequestCost, scope::Scope, selection};
use crate::query_v2::ObservationV2Error as Error;
use hiroute_domain::*;
use rusqlite::Connection;
use std::collections::BTreeMap;

pub(super) struct Report {
    pub rows: Vec<AnalysisCostRowV1>,
    pub savings: Option<AnalysisSavingsV1>,
    pub choices: Vec<AnalysisChoiceV1>,
}

pub(super) fn evaluate(
    db: &Connection,
    s: &Scope,
    q: &ObservationAnalysisQueryV1,
    prices: Option<&AnalysisPricesV1>,
    reader: &ObservationReaderContext,
    now: i64,
) -> Result<Report, Error> {
    let basis = s.basis.unwrap_or_default();
    let mut choices = Vec::new();
    let mut currency = s.currency.clone();
    let mut kind = s.valuation_kind;
    let recorded = if basis == AnalysisCostBasisV1::Recorded {
        Some(recorded_rows(db, s, q, reader, now)?)
    } else {
        None
    };
    if let Some(recorded) = &recorded {
        let values = recorded
            .iter()
            .filter_map(|r| Some((r.currency.as_ref()?.clone(), r.kind.as_ref()?.clone())))
            .collect::<std::collections::BTreeSet<_>>();
        let currencies = values
            .iter()
            .filter(|(_, k)| kind.is_none_or(|v| kind_name(v) == k))
            .map(|(c, _)| c.clone())
            .collect::<std::collections::BTreeSet<_>>();
        if currency.is_none() && currencies.len() == 1 {
            currency = currencies.first().cloned()
        }
        if currency.is_none() && currencies.len() > 1 {
            choices.push(AnalysisChoiceV1 {
                field: "currency".into(),
                values: currencies.into_iter().collect(),
                reason: "Recorded currencies cannot be added together.".into(),
            })
        }
        let kinds = values
            .iter()
            .filter(|(c, _)| currency.as_ref().is_none_or(|v| v == c))
            .map(|(_, k)| k.clone())
            .collect::<std::collections::BTreeSet<_>>();
        if kind.is_none() && kinds.len() == 1 {
            kind = parse_kind(kinds.first().unwrap())
        }
        if kind.is_none() && kinds.len() > 1 {
            choices.push(AnalysisChoiceV1 {
                field: "valuation_kind".into(),
                values: kinds.into_iter().collect(),
                reason: "Choose one recorded valuation kind.".into(),
            })
        }
    } else if let Some(p) = prices {
        let currencies = p
            .prices
            .iter()
            .map(|p| p.quote.currency.clone())
            .collect::<std::collections::BTreeSet<_>>();
        if currency.is_none() && currencies.len() == 1 {
            currency = currencies.first().cloned()
        }
        if currency.is_none() {
            choices.push(AnalysisChoiceV1 {
                field: "currency".into(),
                values: currencies.into_iter().collect(),
                reason: "Choose an available saved currency.".into(),
            })
        }
        kind = Some(PriceValuationKindV1::ApiEquivalent);
    }
    if !choices.is_empty() {
        return Ok(Report {
            rows: vec![],
            savings: None,
            choices,
        });
    }
    let currency = currency.unwrap_or_else(|| "unknown".into());
    let kind = kind.unwrap_or(PriceValuationKindV1::UsageEstimate);
    let mut requests = BTreeMap::<String, RequestCost>::new();
    let mut excluded = 0;
    if let Some(recorded) = recorded {
        for r in recorded {
            if r.traffic != "normal" {
                excluded += 1;
                continue;
            }
            let matched = r.currency.as_deref() == Some(&currency)
                && r.kind.as_deref() == Some(kind_name(kind));
            let amount = matched.then_some(r.amount).flatten();
            let coverage = matched.then_some(r.coverage).flatten();
            requests.insert(
                r.id.clone(),
                RequestCost {
                    id: r.id,
                    anchor: r.anchor,
                    attempts: r.attempts,
                    complete: r.terminal
                        && !r.partial
                        && !r.pending
                        && coverage.as_deref() == Some("complete")
                        && amount.is_some(),
                    known: amount,
                    known_seen: amount.is_some(),
                    pending: r.pending,
                    missing_price: u64::from(coverage.is_none()),
                    ..Default::default()
                },
            );
        }
    } else {
        let prices = prices.ok_or(Error::Unavailable)?;
        let reference = s.reference.as_ref().and_then(|id| {
            prices
                .prices
                .iter()
                .find(|p| &p.binding_id == id && p.quote.currency == currency)
        });
        if q.view == AnalysisViewV1::SavingsSummary && reference.is_none() {
            return Ok(Report {
                rows: vec![],
                savings: None,
                choices: vec![AnalysisChoiceV1 {
                    field: "reference_binding_id".into(),
                    values: prices
                        .prices
                        .iter()
                        .filter(|p| p.quote.currency == currency)
                        .map(|p| p.binding_id.clone())
                        .collect(),
                    reason: "Choose a reference from this captured price vector.".into(),
                }],
            });
        }
        let result = super::comparison::evaluate(
            db,
            selection::CTE,
            &selection::params(s, q),
            &prices.prices,
            reference,
            &currency,
            reader,
            now,
        )?;
        requests = result.requests;
        excluded = result.excluded;
    }
    let group = q.group_by.unwrap_or_default();
    let count = requests.len() as u64;
    let mut compared = 0u64;
    let mut actual = Some(0u64);
    let mut baseline = Some(0u64);
    let mut groups = BTreeMap::<String, AnalysisCostRowV1>::new();
    let mut known_groups = std::collections::BTreeSet::new();
    for mut c in requests.into_values() {
        if !c.known_seen {
            c.known = None;
        }
        if c.complete && c.reference.is_some() && c.known.is_some() {
            compared += 1;
            actual = actual.and_then(|sum| sum.checked_add(c.known?));
            baseline = baseline.and_then(|sum| sum.checked_add(c.reference?));
        }
        let id = if group == AnalysisCostGroupV1::Session {
            c.anchor.clone()
        } else {
            c.id.clone()
        };
        if c.known.is_some() {
            known_groups.insert(id.clone());
        }
        let row = groups.entry(id.clone()).or_insert(AnalysisCostRowV1 {
            id,
            session_id: Some(c.anchor.clone()),
            request_id: (group == AnalysisCostGroupV1::Request).then_some(c.id),
            amount_micros: Some(0),
            known_sum_micros: Some(0),
            currency: currency.clone(),
            valuation_kind: kind,
            coverage: ObservationMetricCoverageV2::Complete,
            request_count: 0,
            attempt_count: 0,
            missing_price_count: 0,
            missing_usage_count: 0,
            pending_request_count: 0,
            highest_known_only: false,
        });
        row.request_count += 1;
        row.attempt_count += c.attempts;
        row.missing_price_count += c.missing_price;
        row.missing_usage_count += c.missing_usage;
        row.pending_request_count += u64::from(c.pending);
        row.known_sum_micros = row
            .known_sum_micros
            .and_then(|sum| sum.checked_add(c.known.unwrap_or(0)));
        if !c.complete || c.known.is_none() {
            row.highest_known_only = true;
            row.amount_micros = None;
            row.coverage = ObservationMetricCoverageV2::Partial;
        } else {
            row.amount_micros = row.amount_micros.and_then(|sum| sum.checked_add(c.known?));
        }
        if row.known_sum_micros.is_none() {
            row.amount_micros = None;
            row.coverage = ObservationMetricCoverageV2::Unknown;
            row.highest_known_only = true;
        }
    }
    let mut rows: Vec<_> = groups
        .into_values()
        .map(|mut r| {
            if !known_groups.contains(&r.id) {
                r.known_sum_micros = None;
                r.amount_micros = None;
                r.coverage = ObservationMetricCoverageV2::Unknown;
                r.highest_known_only = true;
            }
            r
        })
        .collect();
    let ranking_partial = rows
        .iter()
        .any(|r| r.coverage != ObservationMetricCoverageV2::Complete);
    for r in &mut rows {
        r.highest_known_only |= ranking_partial;
    }
    rows.sort_by(|a, b| {
        b.known_sum_micros
            .cmp(&a.known_sum_micros)
            .then_with(|| a.id.cmp(&b.id))
    });
    let savings = if q.view == AnalysisViewV1::SavingsSummary
        && basis == AnalysisCostBasisV1::ApiEquivalent
    {
        let diff = baseline
            .zip(actual)
            .and_then(|(b, a)| i64::try_from(i128::from(b) - i128::from(a)).ok());
        Some(AnalysisSavingsV1{currency,reference_binding_id:s.reference.clone().ok_or(Error::Invalid)?,evaluated_at_ms:prices.ok_or(Error::Invalid)?.evaluated_at_ms,request_count:count,compared_request_count:compared,excluded_request_count:excluded,compared_routed_subtotal_micros:(compared>0).then_some(actual).flatten(),compared_reference_subtotal_micros:(compared>0).then_some(baseline).flatten(),estimated_savings_micros:(compared>0).then_some(diff).flatten(),savings_percent:diff.zip(baseline).filter(|(_,b)|*b>0&&compared>0).map(|(d,b)|d as f64*100.0/b as f64),coverage:if compared==count&&count>0&&!s.range.clipped_for_retention{ObservationMetricCoverageV2::Complete}else if compared>0{ObservationMetricCoverageV2::Partial}else{ObservationMetricCoverageV2::Unknown},decision_overhead:"unmeasured".into(),disclosure:"API-equivalent estimate at captured prices, including attempts and retries; paired complete requests only. Subscription cash, equal output and quality, and unmeasured decision overhead are not established. Missing values are not free.".into()})
    } else {
        None
    };
    Ok(Report {
        rows,
        savings,
        choices,
    })
}

fn kind_name(k: PriceValuationKindV1) -> &'static str {
    match k {
        PriceValuationKindV1::UsageEstimate => "usage_estimate",
        PriceValuationKindV1::ApiEquivalent => "api_equivalent",
    }
}
fn parse_kind(s: &str) -> Option<PriceValuationKindV1> {
    match s {
        "usage_estimate" => Some(PriceValuationKindV1::UsageEstimate),
        "api_equivalent" => Some(PriceValuationKindV1::ApiEquivalent),
        _ => None,
    }
}

struct RecordedRow {
    id: String,
    anchor: String,
    terminal: bool,
    partial: bool,
    pending: bool,
    traffic: String,
    currency: Option<String>,
    kind: Option<String>,
    amount: Option<u64>,
    coverage: Option<String>,
    attempts: u64,
}

/// Resolve kinds and amounts in the same pass through the scoped source join.
/// Choice discovery must not spend the read budget evaluating that join twice.
fn recorded_rows(
    db: &Connection,
    s: &Scope,
    q: &ObservationAnalysisQueryV1,
    reader: &ObservationReaderContext,
    now: i64,
) -> Result<Vec<RecordedRow>, Error> {
    let sql = format!("{} SELECT a.request_id,a.anchor_id,a.terminal,a.partial,a.recorded_pending,a.traffic,
        v.currency,v.valuation_kind,v.amount,v.coverage,
        (SELECT COUNT(*) FROM observation_analysis_attempts_v1 i WHERE i.workspace_id=a.workspace_id AND i.request_id=a.request_id AND i.revision=a.revision)
        FROM selected a LEFT JOIN observation_analysis_recorded_v1 v ON v.workspace_id=a.workspace_id AND v.request_id=a.request_id AND v.revision=a.revision
          AND (:currency IS NULL OR v.currency=:currency) AND (:kind IS NULL OR v.valuation_kind=:kind)
        ORDER BY a.request_id", selection::CTE);
    let kind = s.valuation_kind.map(kind_name);
    let mut args = selection::params(s, q);
    args.extend([
        (":currency", &s.currency as &dyn rusqlite::ToSql),
        (":kind", &kind),
    ]);
    let mut stmt = db.prepare(&sql)?;
    let mut rows = stmt.query(&args[..])?;
    let mut result = Vec::new();
    while let Some(r) = rows.next()? {
        result.push(RecordedRow {
            id: r.get(0)?,
            anchor: r.get(1)?,
            terminal: r.get(2)?,
            partial: r.get(3)?,
            pending: r.get(4)?,
            traffic: r.get(5)?,
            currency: r.get(6)?,
            kind: r.get(7)?,
            amount: r.get::<_, Option<String>>(8)?.and_then(|s| s.parse().ok()),
            coverage: r.get(9)?,
            attempts: r.get(10)?,
        });
        if result.len().is_multiple_of(512) {
            reader.check(now, false, false)?;
        }
    }
    Ok(result)
}
