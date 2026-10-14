//! Grouped discovery is a V2 projection; legacy one-model references retain their meaning.
use super::*;
use hiroute_application_api::{
    COMPUTE_SCAN_RESULT_SCHEMA_V2, ComputeConnectionPointV2, ComputeDiscoveryRefV1,
    ComputeScanDiagnosticV2, ComputeScanRequestV2, ComputeScanResultV2,
};
use hiroute_integrations::{NativeApiConnectionPointV2, native_api_connection_points};

pub(super) fn point_discovery(point: &NativeApiConnectionPointV2) -> ComputeDiscoveryRefV1 {
    ComputeDiscoveryRefV1 {
        discovery_ref: format!("discovery/{}", &point.key_digest.as_str()[7..]),
        discovery_revision: point.revision(),
    }
}

impl LocalControlAdapter {
    pub(in crate::control::runtime) fn compute_point_scan(
        &self,
        request: &ComputeScanRequestV2,
    ) -> Result<ComputeScanResultV2, hiroute_application::control::ControlReadError> {
        use hiroute_application::control::ControlReadError;
        let catalog = self
            .release_catalog
            .as_ref()
            .ok_or(ControlReadError::Unavailable)?;
        let provenance = catalog
            .compute_catalog_provenance()
            .map_err(|_| ControlReadError::Corrupt)?;
        let mut connections = Vec::new();
        let mut diagnostics = Vec::new();
        for (agent, sources) in [
            ("agent_pi_default", self.scanner.pi_api_sources()),
            ("agent_dsh_default", self.scanner.dsh_api_sources()),
        ] {
            let sources = match sources {
                Ok(sources) => sources,
                Err(_) => {
                    diagnostics.push(ComputeScanDiagnosticV2 {
                        agent_id: agent.into(),
                        reason: "configuration_unavailable".into(),
                        native_provider_id: None,
                        model_id: None,
                    });
                    continue;
                }
            };
            for source in &sources {
                if source.point_binding.is_none() {
                    diagnostics.push(ComputeScanDiagnosticV2 {
                        agent_id: agent.into(),
                        reason: "native_endpoint_unavailable".into(),
                        native_provider_id: Some(source.provider_id.clone()),
                        model_id: Some(source.model_id.clone()),
                    });
                }
            }
            for point in native_api_connection_points(sources) {
                let first = point.sources.first().ok_or(ControlReadError::Corrupt)?;
                let importable = point
                    .sources
                    .iter()
                    .all(|s| s.supported_auth && s.protocol.is_some() && s.credential.is_some());
                connections.push(ComputeConnectionPointV2 {
                    point_id: format!("connection-point/{}", &point.key_digest.as_str()[7..]),
                    agent_id: agent.into(),
                    display_name: first.provider_id.clone(),
                    native_provider_id: Some(first.provider_id.clone()),
                    protocol: point.protocol(),
                    declared_model_ids: point.sources.iter().map(|s| s.model_id.clone()).collect(),
                    default_model_id: None,
                    configuration_state: if importable {
                        "static_api_with_protected_input"
                    } else {
                        "native_configuration_not_importable"
                    }
                    .into(),
                    discovery: importable.then(|| point_discovery(&point)),
                    actions_required: if importable {
                        vec![]
                    } else {
                        vec!["review_native_configuration".into()]
                    },
                });
            }
        }
        // Hermes and recognized registered APIs enter this same projection in their leaf wiring.
        if request.native_target.is_some() {
            diagnostics.push(ComputeScanDiagnosticV2 {
                agent_id: "agent_hermes_default".into(),
                reason: "native_producer_unavailable".into(),
                native_provider_id: None,
                model_id: None,
            });
        }
        connections.sort_by(|a, b| a.point_id.cmp(&b.point_id));
        Ok(ComputeScanResultV2 {
            schema: COMPUTE_SCAN_RESULT_SCHEMA_V2.into(),
            catalog: super::super::compute_routing::public_provenance(&provenance),
            connections,
            diagnostics,
        })
    }
}
