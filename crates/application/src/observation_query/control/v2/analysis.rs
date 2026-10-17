use super::*;
use hiroute_application_api::PriceTargetLocatorV1;
use hiroute_domain::*;
use std::collections::BTreeSet;

impl ObservationControl {
    pub(super) fn analysis_read(
        &self,
        reader: &ObservationReaderContext,
        mut q: ObservationAnalysisQueryV1,
        range: Option<&AnalysisTimeRangeV1>,
        now: i64,
    ) -> Result<ObservationAnalysisPageV1, ObservationQueryError> {
        let mut page = self
            .service
            .observation_analysis(reader, &q, range, None, now)?;
        let Some(required) = page.price_capture_request.take() else {
            return Ok(page);
        };
        reader.check(now, false, false)?;
        let facts = self
            .prices
            .as_ref()
            .ok_or(ObservationQueryError::Unavailable)?
            .price_control_facts()
            .map_err(map_control)?;
        if facts.source_bindings.len() > 4096 {
            return Err(ObservationQueryError::Unavailable);
        }
        let currencies: BTreeSet<_> = facts
            .entries
            .iter()
            .filter(|e| e.target.valuation_kind == PriceValuationKindV1::ApiEquivalent)
            .map(|e| e.target.currency.clone())
            .collect();
        let currency = required.currency.or_else(|| {
            (currencies.len() == 1)
                .then(|| currencies.first().cloned())
                .flatten()
        });
        let Some(currency) = currency else {
            page.choices.push(AnalysisChoiceV1{field:"currency".into(),values:currencies.into_iter().collect(),reason:"Choose a saved API-equivalent currency; add source-specific API prices first if no currency is available.".into()});
            return Ok(page);
        };
        let mut prices = Vec::with_capacity(facts.source_bindings.len());
        for binding in &facts.source_bindings {
            let (target, _, _) = crate::prices::resolve_price_target(
                &facts,
                &PriceTargetLocatorV1::Binding {
                    binding_id: binding.binding_id.clone(),
                },
                &currency,
                PriceValuationKindV1::ApiEquivalent,
            )
            .map_err(|_| ObservationQueryError::InvalidQuery)?;
            let quote = facts
                .effective
                .freeze_price(
                    &target,
                    facts.evaluated_at,
                    PriceBillingContextV1::StandardTokens,
                )
                .map_err(|_| ObservationQueryError::InvalidQuery)?;
            prices.push(ApiEquivalentPriceV1 {
                binding_id: binding.binding_id.clone(),
                execution_model_configuration_id: binding.execution_model_configuration_id.clone(),
                quote,
            });
        }
        let reference = required
            .reference_binding_id
            .or_else(|| self.sole_primary(required.plan_id.as_deref()));
        let capture = AnalysisPricesV1 {
            generation_ref: facts
                .effective
                .generation_ref()
                .map(|g| g.id.clone())
                .unwrap_or_else(|| "unavailable".into()),
            evaluated_at_ms: facts.evaluated_at.saturating_mul(1000),
            prices,
            reference_binding_id: reference,
        };
        q.analysis_scope = page.analysis_scope;
        q.cursor = None;
        q.time = Default::default();
        q.currency = Some(currency);
        self.service
            .observation_analysis(reader, &q, None, Some(&capture), now)
    }

    fn sole_primary(&self, plan_id: Option<&str>) -> Option<String> {
        let publication = self
            .routing
            .as_ref()?
            .routing_compilation_snapshot(&self.workspace_id)
            .ok()?
            .active_publication?;
        let plans: Vec<_> = publication
            .plans
            .iter()
            .filter(|p| plan_id.is_none_or(|id| p.body.identity.agent_plan_id.as_str() == id))
            .collect();
        if plans.len() != 1 {
            return None;
        }
        let bindings: BTreeSet<_> = plans[0]
            .body
            .materialized
            .attempt_owned
            .groups
            .iter()
            .filter(|g| g.group_id == MaterializedGroupId::Primary)
            .flat_map(|g| g.candidates.iter().map(|c| c.binding_id.clone()))
            .collect();
        (bindings.len() == 1)
            .then(|| bindings.first().cloned())
            .flatten()
    }
}
