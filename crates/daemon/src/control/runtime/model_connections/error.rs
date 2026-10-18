//! Closed connection-check failures; no URL, credential or provider text crosses control.

use hiroute_application::control::ComputeManagementControlError;
use hiroute_integrations::{ModelConnectionTargetError, NativeModelConnectionErrorV1};

pub(super) fn map_check_error(
    error: NativeModelConnectionErrorV1,
) -> ComputeManagementControlError {
    match error {
        NativeModelConnectionErrorV1::Target(error) => map_target_error(error),
        NativeModelConnectionErrorV1::InvalidDraft => ComputeManagementControlError::Invalid,
        NativeModelConnectionErrorV1::Candidate(error) => super::map_port(error),
        NativeModelConnectionErrorV1::RandomUnavailable => {
            ComputeManagementControlError::Unavailable
        }
    }
}

pub(super) fn map_target_error(error: ModelConnectionTargetError) -> ComputeManagementControlError {
    match error {
        ModelConnectionTargetError::InvalidUrl => {
            ComputeManagementControlError::InvalidModelConnectionUrl
        }
        ModelConnectionTargetError::UnsafePath => {
            ComputeManagementControlError::UnsafeModelConnectionPath
        }
        ModelConnectionTargetError::UnsafeAuthenticationHeader => {
            ComputeManagementControlError::UnsafeModelConnectionAuthenticationHeader
        }
        ModelConnectionTargetError::UnsafeProtocolHeader => {
            ComputeManagementControlError::UnsafeModelConnectionProtocolHeader
        }
    }
}
