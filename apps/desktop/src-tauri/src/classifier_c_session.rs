use super::*;
use hiroute_application_api::{
    HistoricalApplicabilityTestRequestV1, HistoricalApplicabilityTestResultV1,
};

impl Session {
    pub async fn test_classifier_c(
        &mut self,
        request: HistoricalApplicabilityTestRequestV1,
    ) -> Result<HistoricalApplicabilityTestResultV1, DesktopFailure> {
        #[cfg(unix)]
        {
            if !request.validate() {
                return Err("CLASSIFIER_TEST_INVALID".into());
            }
            let status: ClientServiceStatusV1 = query(
                &self.client,
                "GetClientServiceStatus",
                &serde_json::json!({}),
            )
            .await?;
            let capability = self
                .resident
                .register_classifier_diagnostic(&request, &status.revisions)?;
            let envelope: MachineEnvelopeV2<Value> = self
                .client
                .call_typed(LocalControlWireRequestV2 {
                    schema_version: LOCAL_CONTROL_SCHEMA_V2,
                    request_id: crate::random_id()?,
                    operation_id: "TestClassifierDecision".into(),
                    payload: serde_json::to_value(request)
                        .map_err(|_| "CLASSIFIER_TEST_INVALID")?,
                    protected_grant: Some(ProtectedClientGrantV2 {
                        principal_kind: PrincipalKind::Desktop,
                        capability: capability.to_string(),
                    }),
                })
                .await?;
            if envelope.error.is_some() {
                return Err(DesktopFailure::backend(envelope));
            }
            serde_json::from_value(envelope.data.ok_or("RESPONSE_DATA_MISSING")?)
                .map_err(|_| "RESPONSE_DATA_INVALID".into())
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            Err("TRUSTED_AUTHORITY_UNAVAILABLE".into())
        }
    }
}
