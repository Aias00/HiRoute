use super::*;
impl ApplicationService {
    pub(super) fn test_classifier_decision_c(
        &self,
        request: LocalControlRequestV2,
    ) -> MachineEnvelopeV2<Value> {
        use hiroute_application_api::HistoricalApplicabilityTestRequestV1;
        let Some(ports) = self.ports.as_ref() else {
            return failed(ErrorCode::DaemonUnavailable, request.request_id);
        };
        let Ok(test) =
            serde_json::from_value::<HistoricalApplicabilityTestRequestV1>(request.payload)
        else {
            return failed(ErrorCode::InvalidArguments, request.request_id);
        };
        if !test.validate() {
            return failed(ErrorCode::InvalidArguments, request.request_id);
        }
        let Ok(digest) = CanonicalDigest::of(&test) else {
            return failed(ErrorCode::InvalidArguments, request.request_id);
        };
        let revisions = match ports.control.snapshot(&WorkspaceId::default()) {
            Ok(s) => s.revisions,
            Err(e) => return failed(map_control_error(e), request.request_id),
        };
        if let Some(grant) = request.protected_grant.as_ref()
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
        let service = ports.routing.as_ref().and_then(|p| {
            p.decision_service(
                &WorkspaceId::default(),
                &test.service_id,
                test.service_revision,
            )
            .ok()
            .flatten()
        });
        let Some(service) = service else {
            return failed(ErrorCode::InvalidArguments, request.request_id);
        };
        let Some(diagnostic) = ports.classifier_diagnostic.as_deref() else {
            return failed(ErrorCode::GatewayUnavailable, request.request_id);
        };
        match diagnostic.test_classifier_c(&test, &service) {
            Ok(result) => succeeded(result, request.request_id),
            Err(_) => failed(ErrorCode::GatewayUnavailable, request.request_id),
        }
    }
}
