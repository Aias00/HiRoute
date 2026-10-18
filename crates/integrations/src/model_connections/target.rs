use hiroute_application_api::ComputeCandidateTargetV2;
use hiroute_domain::{
    GatewayAuthenticationSemanticsV1, GatewayHeaderSemanticsV1, UpstreamProtocol,
};
use http::{HeaderName, HeaderValue, Uri};
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelConnectionBaseKindV1 {
    /// User-entered API root. `/` receives `/v1`; the selected protocol suffix is appended.
    ApiRoot,
    /// Claude-style native base URL; the native client appends `/v1/messages`.
    NativeMessagesBase,
    /// Codex/OpenAI-style native base URL; the native client appends protocol paths directly.
    NativeResponsesBase,
}

#[derive(Clone, Copy, Debug)]
pub struct ModelConnectionTargetInputV1<'a> {
    pub base_url: &'a str,
    pub base_kind: ModelConnectionBaseKindV1,
    pub protocol: UpstreamProtocol,
    pub request_path_override: Option<&'a str>,
    pub inventory_path_override: Option<&'a str>,
    pub protocol_profile_id: &'a str,
    pub protocol_profile_revision: u64,
    pub protocol_header_semantics: &'a GatewayHeaderSemanticsV1,
    pub authentication: &'a GatewayAuthenticationSemanticsV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedModelConnectionTargetV1 {
    pub candidate_target: ComputeCandidateTargetV2,
    pub inventory_path: Option<String>,
    host_header: String,
    directory_headers: Vec<(HeaderName, HeaderValue)>,
}

impl NormalizedModelConnectionTargetV1 {
    pub fn request_url(&self) -> String {
        self.url_for(&self.candidate_target.request_path)
    }

    pub fn inventory_url(&self, query: Option<&str>) -> String {
        let mut path = self
            .inventory_path
            .clone()
            .expect("directory probe requires a path");
        if let Some(query) = query {
            path.push('?');
            path.push_str(query);
        }
        self.url_for(&path)
    }

    pub fn host_header(&self) -> &str {
        &self.host_header
    }

    pub(crate) fn directory_headers(&self) -> &[(HeaderName, HeaderValue)] {
        &self.directory_headers
    }

    fn url_for(&self, path: &str) -> String {
        format!(
            "{}://{}{}",
            self.candidate_target.scheme, self.host_header, path
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ModelConnectionTargetError {
    #[error("model connection URL is invalid")]
    InvalidUrl,
    #[error("model connection URL contains an unsafe path")]
    UnsafePath,
    #[error("model connection authentication header is unsafe")]
    UnsafeAuthenticationHeader,
    #[error("model connection protocol profile headers are unsupported or unsafe")]
    UnsafeProtocolHeader,
}

/// Validate the configured target without DNS or I/O. The bounded transport resolves
/// its hostname when explicitly checking or executing a request, for HTTP and HTTPS alike.
pub fn normalize_model_connection_target(
    input: ModelConnectionTargetInputV1<'_>,
) -> Result<NormalizedModelConnectionTargetV1, ModelConnectionTargetError> {
    normalize_target(input)
}

/// Pure scan projection. It never resolves hostnames or opens a socket.
pub fn project_model_connection_target(
    input: ModelConnectionTargetInputV1<'_>,
) -> Result<NormalizedModelConnectionTargetV1, ModelConnectionTargetError> {
    normalize_target(input)
}

fn normalize_target(
    input: ModelConnectionTargetInputV1<'_>,
) -> Result<NormalizedModelConnectionTargetV1, ModelConnectionTargetError> {
    let ModelConnectionTargetInputV1 {
        base_url,
        base_kind,
        protocol,
        request_path_override,
        inventory_path_override,
        protocol_profile_id,
        protocol_profile_revision,
        protocol_header_semantics,
        authentication,
    } = input;
    if base_url.trim() != base_url
        || base_url.contains('#')
        || protocol_profile_id.trim().is_empty()
        || protocol_profile_revision == 0
    {
        return Err(ModelConnectionTargetError::InvalidUrl);
    }
    validate_authentication_header(authentication)?;
    let directory_headers =
        validated_directory_headers(protocol, authentication, protocol_header_semantics)?;
    let uri: Uri = base_url
        .parse()
        .map_err(|_| ModelConnectionTargetError::InvalidUrl)?;
    let scheme = uri
        .scheme_str()
        .ok_or(ModelConnectionTargetError::InvalidUrl)?
        .to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https")
        || uri
            .path_and_query()
            .is_some_and(|value| value.query().is_some())
    {
        return Err(ModelConnectionTargetError::InvalidUrl);
    }
    let authority = uri
        .authority()
        .ok_or(ModelConnectionTargetError::InvalidUrl)?;
    if authority.as_str().contains('@') {
        return Err(ModelConnectionTargetError::InvalidUrl);
    }
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err(ModelConnectionTargetError::InvalidUrl);
    }
    let port = authority
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    if port == 0 {
        return Err(ModelConnectionTargetError::InvalidUrl);
    }
    let base_path = normalize_base_path(uri.path())?;
    let (request_path, _) = paths(base_kind, protocol, &base_path);
    let request_path = request_path_override
        .map(validate_override_path)
        .transpose()?
        .unwrap_or(request_path);
    let inventory_path = inventory_path_override
        .map(validate_override_path)
        .transpose()?;

    let host_header = authority_for(&host, port, &scheme);
    Ok(NormalizedModelConnectionTargetV1 {
        candidate_target: ComputeCandidateTargetV2 {
            scheme,
            authority: host,
            port,
            request_path,
            upstream_protocol: protocol,
            protocol_profile_id: protocol_profile_id.to_owned(),
            protocol_profile_revision,
        },
        inventory_path,
        host_header,
        directory_headers,
    })
}

fn validated_directory_headers(
    protocol: UpstreamProtocol,
    authentication: &GatewayAuthenticationSemanticsV1,
    semantics: &GatewayHeaderSemanticsV1,
) -> Result<Vec<(HeaderName, HeaderValue)>, ModelConnectionTargetError> {
    let authentication_header = match authentication {
        GatewayAuthenticationSemanticsV1::ApiKeyHeader { header } => Some(
            HeaderName::from_bytes(header.as_bytes())
                .map_err(|_| ModelConnectionTargetError::UnsafeAuthenticationHeader)?,
        ),
        GatewayAuthenticationSemanticsV1::Bearer | GatewayAuthenticationSemanticsV1::None => None,
    };
    let mut headers = Vec::new();
    for (raw_name, raw_value) in &semantics.required_headers {
        let name = HeaderName::from_bytes(raw_name.as_bytes())
            .map_err(|_| ModelConnectionTargetError::UnsafeProtocolHeader)?;
        if protocol != UpstreamProtocol::Messages
            || name.as_str() != "anthropic-version"
            || authentication_header.as_ref() == Some(&name)
            || headers.iter().any(|(current, _)| current == name)
            || raw_value.is_empty()
            || raw_value.trim() != raw_value
            || raw_value.len() > 256
        {
            return Err(ModelConnectionTargetError::UnsafeProtocolHeader);
        }
        let value = HeaderValue::from_bytes(raw_value.as_bytes())
            .map_err(|_| ModelConnectionTargetError::UnsafeProtocolHeader)?;
        headers.push((name, value));
    }
    Ok(headers)
}

pub fn validate_authentication_header(
    authentication: &GatewayAuthenticationSemanticsV1,
) -> Result<(), ModelConnectionTargetError> {
    let GatewayAuthenticationSemanticsV1::ApiKeyHeader { header } = authentication else {
        return Ok(());
    };
    let parsed = HeaderName::from_bytes(header.as_bytes())
        .map_err(|_| ModelConnectionTargetError::UnsafeAuthenticationHeader)?;
    let name = parsed.as_str();
    const DENIED: &[&str] = &[
        "authorization",
        "connection",
        "content-length",
        "cookie",
        "host",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "transfer-encoding",
        "upgrade",
    ];
    if DENIED.contains(&name) {
        Err(ModelConnectionTargetError::UnsafeAuthenticationHeader)
    } else {
        Ok(())
    }
}

fn normalize_base_path(path: &str) -> Result<String, ModelConnectionTargetError> {
    validate_path(path)?;
    let trimmed = path.trim_end_matches('/');
    Ok(if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.to_owned()
    })
}

fn validate_override_path(path: &str) -> Result<String, ModelConnectionTargetError> {
    validate_path(path)?;
    if path == "/" {
        return Err(ModelConnectionTargetError::UnsafePath);
    }
    Ok(path.to_owned())
}

fn validate_path(path: &str) -> Result<(), ModelConnectionTargetError> {
    if !path.starts_with('/')
        || path.contains(['?', '#', '\\', '%'])
        || (path.len() > 1 && path.contains("//"))
        || path.split('/').any(|segment| matches!(segment, "." | ".."))
    {
        Err(ModelConnectionTargetError::UnsafePath)
    } else {
        Ok(())
    }
}

fn paths(
    kind: ModelConnectionBaseKindV1,
    protocol: UpstreamProtocol,
    base_path: &str,
) -> (String, String) {
    match kind {
        ModelConnectionBaseKindV1::ApiRoot => {
            let root = if base_path.is_empty() {
                "/v1"
            } else {
                base_path
            };
            (
                append(root, protocol_suffix(protocol)),
                append(root, "models"),
            )
        }
        ModelConnectionBaseKindV1::NativeMessagesBase => (
            append(base_path, "v1/messages"),
            append(base_path, "v1/models"),
        ),
        ModelConnectionBaseKindV1::NativeResponsesBase => (
            append(base_path, protocol_suffix(protocol)),
            append(base_path, "models"),
        ),
    }
}

fn protocol_suffix(protocol: UpstreamProtocol) -> &'static str {
    match protocol {
        UpstreamProtocol::Responses => "responses",
        UpstreamProtocol::Messages => "messages",
        UpstreamProtocol::ChatCompletions => "chat/completions",
    }
}

fn append(base: &str, suffix: &str) -> String {
    if base.is_empty() {
        format!("/{suffix}")
    } else {
        format!("{base}/{suffix}")
    }
}

fn authority_for(host: &str, port: u16, scheme: &str) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    if (scheme == "https" && port == 443) || (scheme == "http" && port == 80) {
        host
    } else {
        format!("{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        base_url: &'a str,
        base_kind: ModelConnectionBaseKindV1,
        protocol: UpstreamProtocol,
        protocol_profile_id: &'a str,
        authentication: &'a GatewayAuthenticationSemanticsV1,
    ) -> ModelConnectionTargetInputV1<'a> {
        static EMPTY_HEADERS: std::sync::LazyLock<GatewayHeaderSemanticsV1> =
            std::sync::LazyLock::new(|| GatewayHeaderSemanticsV1 {
                content_type: "application/json".into(),
                required_headers: Vec::new(),
                forbidden_forward_headers: vec!["authorization".into(), "x-api-key".into()],
            });
        ModelConnectionTargetInputV1 {
            base_url,
            base_kind,
            protocol,
            request_path_override: None,
            inventory_path_override: None,
            protocol_profile_id,
            protocol_profile_revision: 1,
            protocol_header_semantics: &EMPTY_HEADERS,
            authentication,
        }
    }

    #[test]
    fn api_root_and_native_base_have_distinct_suffix_rules() {
        let api = normalize_model_connection_target(input(
            "https://example.test/v1/",
            ModelConnectionBaseKindV1::ApiRoot,
            UpstreamProtocol::Messages,
            "profile/messages",
            &GatewayAuthenticationSemanticsV1::None,
        ))
        .unwrap();
        assert_eq!(api.candidate_target.request_path, "/v1/messages");
        assert_eq!(api.inventory_path, None);

        let native = normalize_model_connection_target(input(
            "https://example.test/proxy",
            ModelConnectionBaseKindV1::NativeMessagesBase,
            UpstreamProtocol::Messages,
            "profile/messages",
            &GatewayAuthenticationSemanticsV1::Bearer,
        ))
        .unwrap();
        assert_eq!(native.candidate_target.request_path, "/proxy/v1/messages");
        assert_eq!(native.inventory_path, None);

        let chat = normalize_model_connection_target(input(
            "https://example.test/compatible",
            ModelConnectionBaseKindV1::ApiRoot,
            UpstreamProtocol::ChatCompletions,
            "profile/chat-completions",
            &GatewayAuthenticationSemanticsV1::Bearer,
        ))
        .unwrap();
        assert_eq!(
            chat.candidate_target.request_path,
            "/compatible/chat/completions"
        );
        assert_eq!(chat.inventory_path, None);
    }

    #[test]
    fn explicit_http_and_https_targets_preserve_the_host_without_dns() {
        for (base, authority, host, port) in [
            ("http://192.0.2.10/v1", "192.0.2.10", "192.0.2.10", 80),
            (
                "http://192.168.1.10:8123/v1",
                "192.168.1.10",
                "192.168.1.10:8123",
                8123,
            ),
            (
                "http://gateway.invalid:8123/v1",
                "gateway.invalid",
                "gateway.invalid:8123",
                8123,
            ),
            (
                "http://localhost:8123/v1",
                "localhost",
                "localhost:8123",
                8123,
            ),
            (
                "http://127.0.0.1:8123/v1",
                "127.0.0.1",
                "127.0.0.1:8123",
                8123,
            ),
            ("http://[::1]:8123/v1", "::1", "[::1]:8123", 8123),
            (
                "http://[2001:db8::10]:8123/v1",
                "2001:db8::10",
                "[2001:db8::10]:8123",
                8123,
            ),
            (
                "https://gateway.invalid/v1",
                "gateway.invalid",
                "gateway.invalid",
                443,
            ),
        ] {
            for authentication in [
                GatewayAuthenticationSemanticsV1::None,
                GatewayAuthenticationSemanticsV1::Bearer,
                GatewayAuthenticationSemanticsV1::ApiKeyHeader {
                    header: "x-api-key".into(),
                },
            ] {
                let draft = input(
                    base,
                    ModelConnectionBaseKindV1::ApiRoot,
                    UpstreamProtocol::Responses,
                    "profile/responses",
                    &authentication,
                );
                let target = normalize_model_connection_target(draft).unwrap();
                assert_eq!(target.candidate_target.authority, authority);
                assert_eq!(target.candidate_target.port, port);
                assert_eq!(target.host_header(), host);
                assert_eq!(target.request_url(), format!("{base}/responses"));
                assert_eq!(target.inventory_path, None);
                assert_eq!(project_model_connection_target(draft).unwrap(), target);
            }
        }
    }

    #[test]
    fn explicit_http_directory_uses_the_same_configured_authority() {
        let mut draft = input(
            "http://gateway.invalid:8123/proxy",
            ModelConnectionBaseKindV1::ApiRoot,
            UpstreamProtocol::Responses,
            "profile/responses",
            &GatewayAuthenticationSemanticsV1::Bearer,
        );
        draft.inventory_path_override = Some("/proxy/models");
        let target = normalize_model_connection_target(draft).unwrap();
        assert_eq!(
            target.inventory_url(None),
            "http://gateway.invalid:8123/proxy/models"
        );
        assert_eq!(
            target.request_url(),
            "http://gateway.invalid:8123/proxy/responses"
        );
        assert_eq!(target.host_header(), "gateway.invalid:8123");
    }

    #[test]
    fn unsafe_authentication_headers_fail_before_network() {
        assert_eq!(
            validate_authentication_header(&GatewayAuthenticationSemanticsV1::ApiKeyHeader {
                header: "Proxy-Authorization".into()
            }),
            Err(ModelConnectionTargetError::UnsafeAuthenticationHeader)
        );
    }

    #[test]
    fn http_does_not_allow_protocol_headers_to_inject_credentials() {
        let headers = GatewayHeaderSemanticsV1 {
            content_type: "application/json".into(),
            required_headers: vec![("authorization".into(), "Bearer must-not-leak".into())],
            forbidden_forward_headers: Vec::new(),
        };
        let mut draft = input(
            "http://gateway.invalid/v1",
            ModelConnectionBaseKindV1::ApiRoot,
            UpstreamProtocol::Responses,
            "profile/responses",
            &GatewayAuthenticationSemanticsV1::None,
        );
        draft.protocol_header_semantics = &headers;
        assert_eq!(
            normalize_model_connection_target(draft),
            Err(ModelConnectionTargetError::UnsafeProtocolHeader)
        );
    }

    #[test]
    fn embedded_secrets_and_ambiguous_paths_are_rejected() {
        for url in [
            "http://user:secret@gateway.invalid/v1",
            "http://gateway.invalid/v1?api_key=secret",
            "http://gateway.invalid/v1/%2e%2e/private",
            "ftp://gateway.invalid/v1",
            "https://user:secret@example.test/v1",
            "https://example.test/v1?api_key=secret",
            "https://example.test/v1/%2e%2e/private",
            "https://example.test/v1//models",
        ] {
            assert!(
                normalize_model_connection_target(input(
                    url,
                    ModelConnectionBaseKindV1::ApiRoot,
                    UpstreamProtocol::Responses,
                    "profile/responses",
                    &GatewayAuthenticationSemanticsV1::Bearer,
                ))
                .is_err(),
                "unsafe URL unexpectedly accepted: {url}"
            );
        }
        assert!(
            validate_authentication_header(&GatewayAuthenticationSemanticsV1::ApiKeyHeader {
                header: "bad\r\nheader".into(),
            })
            .is_err()
        );
    }
}
