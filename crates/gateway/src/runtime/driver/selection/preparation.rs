use bytes::Bytes;
use hiroute_gateway_core::core::filter::LocalReply;
use hiroute_gateway_core::runtime::sse::SemanticProvenance;
use http::header::CONTENT_TYPE;
use http::{HeaderMap, HeaderValue, StatusCode};

/// Request-local summary of the closed preparation facts. Different causes cannot
/// be projected as if every candidate was rejected for one source or Key permission.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum PreparationFailure {
    SourceDisabled,
    CredentialDisabled,
    CredentialUnavailable,
    Other,
}

impl PreparationFailure {
    pub(super) fn from_error_class(error_class: Option<&str>) -> Self {
        match error_class {
            Some("source_disabled") => Self::SourceDisabled,
            Some("credential_disabled") => Self::CredentialDisabled,
            Some("credential_exhausted") => Self::CredentialUnavailable,
            _ => Self::Other,
        }
    }

    pub(crate) fn merge(self, next: Self) -> Self {
        if self == next { self } else { Self::Other }
    }

    pub(crate) fn code_phase(self) -> (&'static str, &'static str) {
        match self {
            Self::SourceDisabled => ("SOURCE_DISABLED", "credential_lease"),
            Self::CredentialDisabled => ("CREDENTIAL_DISABLED", "credential_lease"),
            Self::CredentialUnavailable => ("CREDENTIAL_UNAVAILABLE", "credential_lease"),
            Self::Other => ("CANDIDATE_PREPARATION_FAILED", "candidate_preparation"),
        }
    }

    pub(super) fn local_reply(self) -> LocalReply {
        let (code, phase) = self.code_phase();
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        LocalReply {
            status: StatusCode::BAD_GATEWAY,
            headers,
            body: Bytes::from(format!(
                "{{\"schema_version\":\"hiroute.gateway.error/v1\",\"code\":\"{code}\",\"phase\":\"{phase}\"}}"
            )),
            provenance: SemanticProvenance::NonSemantic,
        }
    }
}
