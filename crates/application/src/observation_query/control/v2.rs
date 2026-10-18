use super::*;
use hiroute_application_api::ObservationReadIntentV3;
use hiroute_domain::ObservationReaderContext;
impl ObservationControl {
    pub(super) fn read_v2(
        &self,
        grant: Option<&ProtectedClientGrantV2>,
        payload: Value,
        operation: &str,
    ) -> Result<Value, ObservationQueryError> {
        let (request, version) = hiroute_application_api::decode_observation_read(payload)
            .map_err(|_| ObservationQueryError::InvalidQuery)?;
        let wire = hiroute_application_api::observation_wire_value(&request, version)
            .map_err(|_| ObservationQueryError::InvalidQuery)?;
        if request.operation() != operation {
            return Err(ObservationQueryError::InvalidQuery);
        }
        let now = self.clock.now_ms().map_err(map_control)?;
        let generation = self
            .state
            .snapshot(&self.workspace_id)
            .map_err(map_control)?
            .revisions
            .target;
        let content = matches!(
            request.intent,
            ObservationReadIntentV3::Catalog(_)
                | ObservationReadIntentV3::Ancestry(_)
                | ObservationReadIntentV3::Content(_)
                | ObservationReadIntentV3::Search(_)
                | ObservationReadIntentV3::SessionSearch(_)
        );
        let search = matches!(
            request.intent,
            ObservationReadIntentV3::Search(_) | ObservationReadIntentV3::SessionSearch(_)
        );
        if grant.is_some() || content {
            self.protected_principal(grant, request.protected_operation(), &wire)?;
        }
        // A new short-lived context follows actual grant validation on EVERY
        // page. Its expiry is not a cursor scope: renewing this same local-user
        // scope does not authorize a broader filter or bypass grant validation.
        let reader = ObservationReaderContext::local_user(
            self.workspace_id.clone(),
            "protected-local-observer".into(),
            generation,
            now.saturating_add(1000),
            content,
            search,
        )?;
        let reader = if version == hiroute_application_api::ObservationWireVersion::LegacyV2 {
            reader.with_legacy_query_v2()
        } else {
            reader
        };
        let legacy_fields = match &request.intent {
            ObservationReadIntentV3::Facts(_) => Some("facts"),
            ObservationReadIntentV3::PlanQuality(_) => Some("quality"),
            _ => None,
        };
        let mut result = match request.intent {
            ObservationReadIntentV3::Status(_) => {
                serde_json::to_value(self.service.observed_maintenance_status(&reader, now)?)
            }
            ObservationReadIntentV3::HomeValue(q) => {
                let from_ms = match q
                    .period
                    .unwrap_or(hiroute_application_api::ValuePeriodV1::Today)
                {
                    hiroute_application_api::ValuePeriodV1::Today => {
                        self.clock.local_day_start_ms(now).map_err(map_control)?
                    }
                    hiroute_application_api::ValuePeriodV1::SevenDays => {
                        // Session details cannot use day archives. Their default window
                        // begins at the first retained millisecond, not one millisecond
                        // before it (which would mark every fresh session partial).
                        now.saturating_sub(7 * DAY_MILLIS)
                            .saturating_add(i64::from(q.session_id.is_some()))
                    }
                    hiroute_application_api::ValuePeriodV1::ThirtyDays => {
                        now.saturating_sub(30 * DAY_MILLIS)
                    }
                };
                serde_json::to_value(self.service.observed_value_totals(
                    &reader,
                    &hiroute_domain::ObservationValueQueryV2 {
                        from_ms,
                        to_ms: now,
                        session_id: q.session_id,
                        plan_id: None,
                        currency: q.currency,
                    },
                    now,
                )?)
            }
            ObservationReadIntentV3::Sessions(q) => {
                serde_json::to_value(self.service.observed_sessions(&reader, &q, now)?)
            }
            ObservationReadIntentV3::Timeline(q) => {
                serde_json::to_value(self.service.observed_timeline(&reader, &q, now)?)
            }
            ObservationReadIntentV3::Catalog(q) => {
                serde_json::to_value(self.service.observed_catalog(&reader, &q, now)?)
            }
            ObservationReadIntentV3::Ancestry(q) => {
                serde_json::to_value(self.service.observed_ancestry(&reader, &q, now)?)
            }
            ObservationReadIntentV3::Content(q) => {
                serde_json::to_value(self.service.observed_content_page(&reader, &q, now)?)
            }
            ObservationReadIntentV3::Facts(q) => {
                serde_json::to_value(self.service.observed_facts(&reader, &q, now)?)
            }
            ObservationReadIntentV3::Search(q) => {
                serde_json::to_value(self.service.search_observed_text(&reader, &q, now)?)
            }
            ObservationReadIntentV3::SessionSearch(q) => {
                serde_json::to_value(self.service.search_observed_sessions(&reader, &q, now)?)
            }
            ObservationReadIntentV3::Value(q) => {
                serde_json::to_value(self.service.observed_value_totals(&reader, &q, now)?)
            }
            ObservationReadIntentV3::ValueReport(q) => {
                if q.from_ms.is_some() && q.period.is_some() {
                    return Err(ObservationQueryError::InvalidQuery);
                }
                let to_ms = q.to_ms.unwrap_or(now);
                let from_ms = match q.from_ms {
                    Some(from_ms) => from_ms,
                    None => match q
                        .period
                        .unwrap_or(hiroute_application_api::ValuePeriodV1::SevenDays)
                    {
                        hiroute_application_api::ValuePeriodV1::Today => {
                            self.clock.local_day_start_ms(to_ms).map_err(map_control)?
                        }
                        hiroute_application_api::ValuePeriodV1::SevenDays => to_ms
                            .saturating_sub(7 * DAY_MILLIS)
                            .saturating_add(i64::from(q.session_id.is_some())),
                        hiroute_application_api::ValuePeriodV1::ThirtyDays => {
                            to_ms.saturating_sub(30 * DAY_MILLIS)
                        }
                    },
                };
                if from_ms >= to_ms {
                    return Err(ObservationQueryError::InvalidQuery);
                }
                serde_json::to_value(self.service.observed_value_report(
                    &reader,
                    &hiroute_domain::ObservationValueQueryV2 {
                        from_ms,
                        to_ms,
                        session_id: q.session_id.map(|id| id.to_string()),
                        plan_id: q.agent_plan_id.map(|id| id.as_str().to_owned()),
                        currency: q.currency,
                    },
                    q.group_by,
                    now,
                )?)
            }
            ObservationReadIntentV3::PlanQuality(q) => serde_json::to_value(
                self.service
                    .observed_plan_quality_samples(&reader, &q, now)?,
            ),
        }
        .map_err(|_| ObservationQueryError::Corrupt)?;
        if version == hiroute_application_api::ObservationWireVersion::LegacyV2
            && legacy_fields.is_some()
        {
            if let Some(object) = result.as_object_mut() {
                object.remove("schema");
                object.remove("routing_observations");
            }
        }
        if serde_json::to_vec(&result)
            .map_err(|_| ObservationQueryError::Corrupt)?
            .len()
            > 1024 * 1024
        {
            return Err(ObservationQueryError::Unavailable);
        }
        Ok(result)
    }
}

impl ObservationControl {
    pub(crate) fn delete_preview(
        &self,
        grant: Option<&ProtectedClientGrantV2>,
        payload: Value,
    ) -> Result<Value, ObservationQueryError> {
        let request: hiroute_application_api::ObservationDeletePreviewRequestV2 =
            serde_json::from_value(payload).map_err(|_| ObservationQueryError::InvalidQuery)?;
        let principal = self.protected_principal(grant, "PreviewSessionDeletionV2", &request)?;
        serde_json::to_value(self.service.preview_session_deletion_v2(
            &principal,
            &request.spec,
            self.clock.now_ms().map_err(map_control)?,
        )?)
        .map_err(|_| ObservationQueryError::Corrupt)
    }
    pub(crate) fn delete_apply(
        &self,
        grant: Option<&ProtectedClientGrantV2>,
        payload: Value,
    ) -> Result<Value, ObservationQueryError> {
        let request: hiroute_application_api::ObservationDeleteApplyRequestV2 =
            serde_json::from_value(payload).map_err(|_| ObservationQueryError::InvalidQuery)?;
        let principal = self.protected_principal(grant, "ApplySessionDeletionV2", &request)?;
        serde_json::to_value(self.service.apply_session_deletion_v2(
            &principal,
            &request.preview,
            &request.accepted_digest,
            self.clock.now_ms().map_err(map_control)?,
        )?)
        .map_err(|_| ObservationQueryError::Corrupt)
    }
}
