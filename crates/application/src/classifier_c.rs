//! One current diagnostic business path; v1 is accepted only by its wire adapter.
use super::*;
use hiroute_application_api::*;
impl ApplicationService {
    pub(super) fn test_classifier_current(
        &self,
        request: LocalControlRequestV2,
    ) -> MachineEnvelopeV2<Value> {
        if request
            .protected_grant
            .as_ref()
            .is_some_and(|g| g.principal_kind.is_collaboration())
            || (request.protected_grant.is_none()
                && !request
                    .principal
                    .capabilities
                    .iter()
                    .any(|c| c == "same-os-user:query-preview"))
        {
            return failed(ErrorCode::CapabilityDenied, request.request_id);
        }
        let Some(ports) = &self.ports else {
            return failed(ErrorCode::DaemonUnavailable, request.request_id);
        };
        let legacy = request.payload.get("schema").and_then(Value::as_str)
            == Some(CLASSIFIER_DECISION_TEST_SCHEMA_V1);
        let (test, auth) = if legacy {
            let Ok(old) =
                serde_json::from_value::<ClassifierDecisionTestRequestV1>(request.payload)
            else {
                return failed(ErrorCode::InvalidArguments, request.request_id);
            };
            if !old.validate() {
                return failed(ErrorCode::InvalidArguments, request.request_id);
            }
            let Ok(auth) = serde_json::to_value(&old) else {
                return failed(ErrorCode::InvalidArguments, request.request_id);
            };
            (ClassifierDecisionTestRequestV2::value(old.classifier), auth)
        } else {
            let Ok(test) =
                serde_json::from_value::<ClassifierDecisionTestRequestV2>(request.payload)
            else {
                return failed(ErrorCode::InvalidArguments, request.request_id);
            };
            let Ok(auth) = serde_json::to_value(&test) else {
                return failed(ErrorCode::InvalidArguments, request.request_id);
            };
            (test, auth)
        };
        if !test.validate() {
            return failed(ErrorCode::InvalidArguments, request.request_id);
        }
        let Ok(digest) = CanonicalDigest::of(&auth) else {
            return failed(ErrorCode::InvalidArguments, request.request_id);
        };
        let revisions = match ports.control.snapshot(&WorkspaceId::default()) {
            Ok(s) => s.revisions,
            Err(e) => return failed(map_control_error(e), request.request_id),
        };
        if let Some(grant) = &request.protected_grant
            && let Err(e) = ports.control.validate_protected_capability(
                &grant.capability,
                &WorkspaceId::default(),
                grant.principal_kind,
                "TestClassifierDecision",
                &digest,
                &revisions,
            )
        {
            return failed(map_control_error(e), request.request_id);
        }
        let Some(diagnostic) = ports.classifier_diagnostic.as_ref() else {
            return failed(ErrorCode::GatewayUnavailable, request.request_id);
        };
        let service = match &test.target {
            ClassifierDecisionTestTargetV2::Saved {
                service_id,
                service_revision,
                ..
            } => {
                let Some(service) = ports.routing.as_ref().and_then(|p| {
                    p.decision_service(&WorkspaceId::default(), service_id, *service_revision)
                        .ok()
                        .flatten()
                }) else {
                    return failed(ErrorCode::InvalidArguments, request.request_id);
                };
                Some(service)
            }
            _ => None,
        };
        if let Some(probe) = test.capability() {
            return match diagnostic
                .test_classifier_c(&probe, service.as_ref().expect("saved target"))
            {
                Ok(result) => succeeded(result, request.request_id),
                Err(_) => failed(ErrorCode::GatewayUnavailable, request.request_id),
            };
        }
        let classifier = match test.target {
            ClassifierDecisionTestTargetV2::Value { classifier } => classifier,
            ClassifierDecisionTestTargetV2::Saved { .. } => {
                hiroute_domain::ComplexityClassifierModeV1::DecisionService {
                    service: Box::new(service.clone().expect("saved target")),
                }
            }
        };
        let result = match diagnostic
            .test_classifier_decision(&ClassifierConnectivityProbeV1 { classifier })
        {
            Ok(r) => r,
            Err(_) => return failed(ErrorCode::GatewayUnavailable, request.request_id),
        };
        if legacy {
            return succeeded(
                ClassifierDecisionTestResultV1 {
                    schema: CLASSIFIER_DECISION_TEST_RESULT_SCHEMA_V1.into(),
                    outcome: result.outcome,
                    branch_id: result.branch_id,
                    duration_millis: result.duration_millis,
                    failure_code: result.failure_code,
                },
                request.request_id,
            );
        }
        let passed = result.outcome == ClassifierDecisionTestOutcomeV1::Passed;
        let tested_at = match ports.clock.now_ms() {
            Ok(value) => value.max(0) as u64,
            Err(_) => return failed(ErrorCode::DaemonUnavailable, request.request_id),
        };
        let mut current = ClassifierDecisionTestResultV2 {
            schema: CLASSIFIER_DECISION_TEST_RESULT_SCHEMA_V2.into(),
            state: if passed {
                CCapabilityStateV1::Verified
            } else {
                CCapabilityStateV1::TestFailed
            },
            verified_shapes: if passed {
                vec!["connectivity".into()]
            } else {
                vec![]
            },
            branch_id: result.branch_id,
            failure_code: result.failure_code.clone(),
            reason: result
                .failure_code
                .unwrap_or_else(|| "connectivity_complete".into()),
            service_id: None,
            service_revision: None,
            endpoint_digest: None,
            model: None,
            protocol_digest: None,
            shape: None,
            question_count: 2,
            tested_at_unix_ms: tested_at,
            duration_millis: result.duration_millis.unwrap_or(0),
        };
        if let Some(service) = service {
            current.endpoint_digest = CanonicalDigest::of(&service.connection.transport().0).ok();
            current.model = match service.connection {
                hiroute_domain::DecisionConnectionV1::SystemOne { model, .. } => Some(model),
                _ => None,
            };
            current.service_id = Some(service.id);
            current.service_revision = Some(service.revision);
        }
        succeeded(current, request.request_id)
    }
}
