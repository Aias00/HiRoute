//! Aggregate the complete retained input before ranking. Parsing numeric token
//! columns never reparses per-attempt quote graphs or changes their rounding.
use super::{
    projection::{PricingIdentity, model_key},
    scope::Scope,
    selection,
};
use crate::{
    query_v2::ObservationV2Error as Error,
    valuation::{
        MetricCoverage, RawUsage, ValuationMissingReason, amount_only, reference_usage_semantics,
    },
};
use hiroute_domain::*;
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
struct RequestCost {
    id: String,
    anchor: String,
    attempts: u64,
    complete: bool,
    known: Option<u64>,
    known_seen: bool,
    missing_price: u64,
    missing_usage: u64,
    pending: bool,
    reference: Option<u64>,
}

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
    if basis == AnalysisCostBasisV1::Recorded {
        let mut stmt=db.prepare(&format!("{} SELECT DISTINCT v.currency,v.valuation_kind FROM selected a
          JOIN observation_analysis_recorded_v1 v ON v.workspace_id=a.workspace_id AND v.request_id=a.request_id AND v.revision=a.revision ORDER BY v.currency,v.valuation_kind",selection::CTE))?;
        let values = stmt
            .query_map(&selection::params(s, q)[..], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
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
    if basis == AnalysisCostBasisV1::Recorded {
        let sql=format!("{} SELECT a.request_id,a.anchor_id,a.terminal,a.partial,a.recorded_pending,a.traffic,v.amount,v.coverage,
          (SELECT COUNT(*) FROM observation_analysis_attempts_v1 i WHERE i.workspace_id=a.workspace_id AND i.request_id=a.request_id AND i.revision=a.revision)
          FROM selected a LEFT JOIN observation_analysis_recorded_v1 v ON v.workspace_id=a.workspace_id AND v.request_id=a.request_id AND v.revision=a.revision AND v.currency=:currency AND v.valuation_kind=:kind ORDER BY a.request_id",selection::CTE);
        let name = kind_name(kind);
        let mut args = selection::params(s, q);
        args.extend([
            (":currency", &currency as &dyn rusqlite::ToSql),
            (":kind", &name),
        ]);
        let mut stmt = db.prepare(&sql)?;
        let mut rows = stmt.query(&args[..])?;
        while let Some(r) = rows.next()? {
            if r.get::<_, String>(5)? != "normal" {
                excluded += 1;
                continue;
            }
            let id: String = r.get(0)?;
            let terminal: bool = r.get(2)?;
            let partial: bool = r.get(3)?;
            let pending: bool = r.get(4)?;
            let amount: Option<String> = r.get(6)?;
            let coverage: Option<String> = r.get(7)?;
            let amount = amount.and_then(|v| v.parse::<u64>().ok());
            requests.insert(
                id.clone(),
                RequestCost {
                    id,
                    anchor: r.get(1)?,
                    attempts: r.get(8)?,
                    complete: terminal
                        && !partial
                        && !pending
                        && coverage.as_deref() == Some("complete")
                        && amount.is_some(),
                    known: amount,
                    known_seen: amount.is_some(),
                    pending,
                    missing_price: u64::from(coverage.is_none()),
                    ..Default::default()
                },
            );
            if requests.len() % 512 == 0 {
                reader.check(now, false, false)?;
            }
        }
    } else {
        let prices = prices.ok_or(Error::Unavailable)?;
        let index = price_index(prices, &currency);
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
        let identities = {
            let mut stmt = db.prepare("SELECT key,body FROM observation_analysis_pricing_v1")?;
            let mut map = HashMap::new();
            for row in
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            {
                let (k, v) = row?;
                if let Ok(v) = serde_json::from_str::<PricingIdentity>(&v) {
                    map.insert(k, v);
                }
            }
            map
        };
        let matches: HashMap<_, _> = identities
            .iter()
            .map(|(key, id)| {
                let p = if id.standard_tokens {
                    index
                        .get(&(
                            id.source_id.as_str(),
                            id.source_digest.as_str(),
                            id.model_id.as_str(),
                        ))
                        .copied()
                } else {
                    None
                };
                (key.as_str(), p)
            })
            .collect();
        let sql=format!("{} SELECT a.request_id,a.anchor_id,a.terminal,a.partial,a.accepted_ordinal,a.traffic,i.ordinal,i.pricing_key,i.input_tokens,i.output_tokens,i.cache_read,i.cache_write,i.reasoning
          FROM selected a LEFT JOIN observation_analysis_attempts_v1 i ON i.workspace_id=a.workspace_id AND i.request_id=a.request_id AND i.revision=a.revision ORDER BY a.request_id,i.ordinal",selection::CTE);
        let mut stmt = db.prepare(&sql)?;
        let mut rows = stmt.query(&selection::params(s, q)[..])?;
        let mut n = 0u64;
        let mut last_excluded = String::new();
        while let Some(r) = rows.next()? {
            n += 1;
            if n % 512 == 0 {
                reader.check(now, false, false)?;
            }
            let id: String = r.get(0)?;
            if r.get::<_, String>(5)? != "normal" {
                if last_excluded != id {
                    excluded += 1;
                    last_excluded = id
                }
                continue;
            }
            let c = requests.entry(id.clone()).or_insert(RequestCost {
                id,
                anchor: r.get(1)?,
                complete: r.get::<_, bool>(2)? && !r.get::<_, bool>(3)?,
                known: Some(0),
                ..Default::default()
            });
            let ordinal: Option<u64> = r.get(6)?;
            c.attempts += u64::from(ordinal.is_some());
            let key: Option<String> = r.get(7)?;
            let identity = key.as_ref().and_then(|k| identities.get(k));
            let price = key
                .as_deref()
                .and_then(|k| matches.get(k).copied().flatten());
            let number = |column| -> rusqlite::Result<Option<u64>> {
                Ok(r.get::<_, Option<String>>(column)?
                    .and_then(|s| s.parse().ok()))
            };
            let raw = RawUsage {
                input: number(8)?,
                output: number(9)?,
                read: number(10)?,
                write: number(11)?,
                reasoning: number(12)?,
            };
            if let Some((p, i)) = price.zip(identity) {
                let amount = amount_only(
                    &p.quote,
                    &reference_usage_semantics(i.semantics, &raw, &p.quote),
                );
                if amount.coverage != MetricCoverage::Complete {
                    c.complete = false;
                    c.missing_usage += u64::from(
                        amount
                            .missing
                            .contains(&Some(ValuationMissingReason::UsageUnknown)),
                    );
                    c.missing_price += u64::from(
                        amount
                            .missing
                            .contains(&Some(ValuationMissingReason::PriceUnknown)),
                    );
                }
                if let Some(v) = amount.known_sum_micros {
                    c.known_seen = true;
                    c.known = c.known.and_then(|sum| sum.checked_add(v));
                } else {
                    c.complete = false;
                }
            } else {
                c.complete = false;
                c.missing_price += 1;
            }
            let accepted: Option<u64> = r.get(4)?;
            if ordinal.is_some() && ordinal == accepted {
                c.reference = reference.zip(identity).and_then(|(p, i)| {
                    let a = amount_only(
                        &p.quote,
                        &reference_usage_semantics(i.semantics, &raw, &p.quote),
                    );
                    (a.coverage == MetricCoverage::Complete)
                        .then_some(a.known_sum_micros)
                        .flatten()
                });
            }
        }
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
fn price_index<'a>(
    p: &'a AnalysisPricesV1,
    currency: &str,
) -> HashMap<(&'a str, &'a str, &'a str), &'a ApiEquivalentPriceV1> {
    let mut map = HashMap::new();
    for p in &p.prices {
        if p.quote.currency != currency {
            continue;
        }
        let target = &p.quote.exact_target;
        map.entry((
            target.source_id.as_str(),
            target.source_identity_digest.as_str(),
            model_key(&target.model_identity),
        ))
        .or_insert(p);
        if let Some(id) = &p.execution_model_configuration_id {
            map.entry((
                target.source_id.as_str(),
                target.source_identity_digest.as_str(),
                id.as_str(),
            ))
            .or_insert(p);
        }
    }
    map
}
