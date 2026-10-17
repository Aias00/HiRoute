//! Read existing retained publications for assessment-only rollback, without extending retention.
use super::*;
use hiroute_domain::{AssessmentRollbackOptionV1, HistoricalAssessmentSourceV1};
use std::collections::BTreeMap;

impl ControlStore {
    pub fn assessment_rollback_sources(
        &self,
        current: &PlanExecutionRef,
    ) -> Result<Vec<AssessmentRollbackOptionV1>, PlanVersionError> {
        let current_version = self.lookup_exact_plan_version(current)?;
        let mut needed: BTreeMap<_, _> = current_version
            .configuration
            .assessment_scopes()
            .keys()
            .map(|k| (k.clone(), None))
            .collect();
        let connection = self.connection.borrow();
        let mut statement = connection.prepare("SELECT content_digest,content_revision FROM plan_versions WHERE workspace_id=?1 AND plan_id=?2 AND content_revision<?3 AND state='published' ORDER BY content_revision DESC LIMIT 512").map_err(storage)?;
        let rows = statement
            .query_map(
                params![
                    current.workspace_id.as_str(),
                    current.plan_id.as_str(),
                    current.content_revision
                ],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)),
            )
            .map_err(storage)?;
        for row in rows {
            let (digest, revision) = row.map_err(storage)?;
            let reference = PlanExecutionRef {
                workspace_id: current.workspace_id.clone(),
                plan_id: current.plan_id.clone(),
                content_revision: revision,
                content_digest: hiroute_domain::CanonicalDigest::parse(digest)
                    .map_err(|_| PlanVersionError::Invalid)?,
            };
            let version = lookup_in(&connection, &reference)?;
            if version.configuration.native_protocol().is_some() {
                continue;
            }
            for (scope, judgment) in version.configuration.assessment_scopes() {
                if let Some(value) = needed.get_mut(&scope)
                    && value.is_none()
                {
                    *value = Some(AssessmentRollbackOptionV1 {
                        source: HistoricalAssessmentSourceV1 {
                            plan: reference.clone(),
                            scope,
                        },
                        instructions: judgment.competence.instructions.clone(),
                        criteria: judgment.competence.criteria.clone(),
                    });
                }
            }
            if needed.values().all(Option::is_some) {
                break;
            }
        }
        Ok(needed.into_values().flatten().collect())
    }
}
