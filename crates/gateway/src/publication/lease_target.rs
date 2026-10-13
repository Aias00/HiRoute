use std::sync::Arc;

pub(crate) fn for_request(
    execution: &crate::server::publication::SealedCandidateExecutionV1,
    profile: &crate::server::core_runtime::profiles::CandidateProtocolProfile,
) -> Result<hiroute_domain::GatewayOperationalTargetV1, Arc<str>> {
    if execution.connector_runtime == hiroute_domain::ConnectorRuntimeKind::BuiltinNative {
        if let Some(target) = &profile.native_target {
            let domain_profile: hiroute_domain::GatewayCandidateProtocolProfileV1 =
                serde_json::from_value(
                    serde_json::to_value(profile).map_err(|_| Arc::from("protocol_failed"))?,
                )
                .map_err(|_| Arc::from("protocol_failed"))?;
            if !target.validate_for(&domain_profile) {
                return Err(Arc::from("protocol_failed"));
            }
            return Ok(target.operational_target.clone());
        }
        execution
            .operational_target
            .for_protocol_path(&profile.connector.request_path)
            .ok_or_else(|| Arc::from("protocol_failed"))
    } else {
        Ok(execution.operational_target.clone())
    }
}
