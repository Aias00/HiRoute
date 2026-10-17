use super::*;
use hiroute_domain::{
    ObservationReaderContext, PlanTrustQueryV1, TrustPopulationQueryV1, TrustRevisionNameV1,
    TrustRevisionV1,
};

impl ObservationControl {
    pub(super) fn trust_suggestions(
        &self,
        reader: &ObservationReaderContext,
        now: i64,
    ) -> Result<Value, ObservationQueryError> {
        let started = std::time::Instant::now();
        let generation = self
            .state
            .snapshot(&self.workspace_id)
            .map_err(map_control)?
            .revisions;
        let ids = self
            .state
            .trust_plan_ids(&self.workspace_id)
            .map_err(map_control)?;
        let mut suggestions = Vec::new();
        for plan_id in ids {
            if started.elapsed() > std::time::Duration::from_secs(1) {
                return Err(ObservationQueryError::ScopeTooLarge);
            }
            let query = PlanTrustQueryV1 {
                plan_id: plan_id.clone(),
                branch_id: None,
                revision: Default::default(),
                from_ms: None,
                to_ms: None,
                group_by: Default::default(),
            };
            let report = self.trust_result(reader, &query, None, now)?;
            let branches: Vec<_> = report["branches"]
                .as_array()
                .ok_or(ObservationQueryError::Corrupt)?
                .iter()
                .filter(|b| b["suggestion"]["kind"] != "keep")
                .cloned()
                .collect();
            if !branches.is_empty() {
                suggestions.push(json!({"plan_id":plan_id,"active_revision":report["active_revision"],"branches":branches,"announcements":report["announcements"],"rows":report["rows"],"previous_routing":report["previous_routing"]}));
            }
        }
        if self
            .state
            .snapshot(&self.workspace_id)
            .map_err(map_control)?
            .revisions
            != generation
        {
            return Err(ObservationQueryError::StalePreview);
        }
        Ok(json!({"suggestions":suggestions}))
    }

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
        let tasks = if query.branch_id.is_none() {
            self.state
                .trust_task_population(
                    &self.workspace_id,
                    &TrustPopulationQueryV1 {
                        active_revision: context.active.content_revision,
                        plan_id: query.plan_id.clone(),
                        branch_id: None,
                        revision,
                        from_ms: Some(population.from_ms),
                        to_ms: Some(population.to_ms),
                    },
                    population.from_ms,
                    population.to_ms,
                )
                .map_err(map_control)?
        } else {
            Default::default()
        };
        if tasks.over_limit {
            return Err(ObservationQueryError::ScopeTooLarge);
        }
        let mut report =
            crate::trust::report::report(query, &context, &population, &runs, &tasks.tasks);
        let mut targets = Vec::new();
        if report.branches.iter().any(|branch| {
            matches!(
                branch.suggestion,
                hiroute_domain::TrustSuggestionV1::ChangeRegularModel
            )
        }) {
            targets = self
                .state
                .trust_recovery_targets(&query.plan_id)
                .map_err(map_control)?;
            if !targets.is_empty() {
                for branch in &mut report.branches {
                    if matches!(
                        branch.suggestion,
                        hiroute_domain::TrustSuggestionV1::ChangeRegularModel
                    ) {
                        branch.suggestion = crate::trust::suggestion::suggest(
                            branch.threshold_millis,
                            &branch.full_history,
                            &branch.upward_cohort,
                            true,
                        );
                    }
                }
            }
        }
        let mut value = serde_json::to_value(report).map_err(|_| ObservationQueryError::Corrupt)?;
        value["previous_routing"] =
            serde_json::to_value(targets).map_err(|_| ObservationQueryError::Corrupt)?;
        Ok(value)
    }
}
