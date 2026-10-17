use super::*;
use hiroute_domain::{
    ObservationReaderContext, PlanTrustQueryV1, TrustPopulationQueryV1, TrustRevisionNameV1,
    TrustRevisionV1,
};

impl ObservationControl {
    pub(super) fn trust_result(
        &self,
        reader: &ObservationReaderContext,
        query: &PlanTrustQueryV1,
        thresholds: Option<&[u16]>,
        now: i64,
    ) -> Result<Value, ObservationQueryError> {
        let context = self
            .state
            .trust_context(&self.workspace_id, &query.plan_id)
            .map_err(map_control)?;
        let revision = match &query.revision {
            TrustRevisionV1::Number(n) => Some(*n),
            TrustRevisionV1::Name(TrustRevisionNameV1::Active) => {
                Some(context.active.content_revision)
            }
            TrustRevisionV1::Name(TrustRevisionNameV1::All) => None,
        };
        let population = self.service.observed_trust_population(
            reader,
            &TrustPopulationQueryV1 {
                active_revision: context.active.content_revision,
                plan_id: query.plan_id.clone(),
                branch_id: query.branch_id.clone(),
                revision,
                from_ms: query.from_ms,
                to_ms: query.to_ms,
            },
            now,
        )?;
        let latest = self
            .state
            .trust_context(&self.workspace_id, &query.plan_id)
            .map_err(map_control)?;
        if latest.generation != context.generation {
            return Err(ObservationQueryError::StalePreview);
        }
        if let Some(thresholds) = thresholds {
            return serde_json::to_value(crate::trust::replay::replay(
                &population.stages,
                thresholds,
            ))
            .map_err(|_| ObservationQueryError::Corrupt);
        }
        let ids = population
            .stages
            .iter()
            .chain(population.active_stages.iter())
            .filter_map(|s| s.sample.run_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let runs = self
            .state
            .trust_runs(&self.workspace_id, &ids)
            .map_err(map_control)?;
        serde_json::to_value(crate::trust::report::report(
            query,
            &context,
            &population,
            &runs,
        ))
        .map_err(|_| ObservationQueryError::Corrupt)
    }
}
