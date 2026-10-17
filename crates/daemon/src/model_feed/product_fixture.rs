//! Test-binary-only external provider/transport fixtures for the installed product journey.
//! Normal daemon builds have no runtime trust-root, URL or inventory override.
use base64::{Engine, engine::general_purpose::STANDARD};
use hiroute_integrations::{TrustedReleaseCatalog, model_feed::*};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::{path::PathBuf, sync::Arc};

pub(crate) fn root() -> Option<PathBuf> {
    std::env::var_os("HIROUTE_TEST_MODEL_FEED_ROOT").map(PathBuf::from)
}
fn key() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[19; 32]).expect("fixture key")
}
pub(crate) fn verify(bytes: &[u8]) -> Result<VerifiedFeed, FeedError> {
    let keys = serde_json::to_vec(&serde_json::json!({"keys":[{
        "key_id":"installed-product-test", "ed25519":STANDARD.encode(key().public_key().as_ref())
    }]}))
    .expect("fixture keys");
    verify_envelope_with_keys(bytes, &keys)
}
pub(crate) fn download(root: &std::path::Path) -> Result<FeedDownload, FeedError> {
    let bytes = std::fs::read(root.join("publication.json"))
        .map_err(|_| FeedError::new(FeedFailureKind::Network, "fixture publication unavailable"))?;
    let envelope = serde_json::to_vec(&serde_json::json!({
        "schema":MODEL_FEED_ENVELOPE_SCHEMA, "payload":STANDARD.encode(&bytes),
        "signatures":[{"key_id":"installed-product-test", "ed25519":STANDARD.encode(key().sign(&bytes).as_ref())}]
    })).expect("fixture envelope");
    Ok(FeedDownload::Content {
        bytes: envelope,
        etag: None,
    })
}
pub(crate) fn catalog(catalog: TrustedReleaseCatalog) -> Result<TrustedReleaseCatalog, String> {
    // A later client embedding exactly these model facts is a separate restart case.
    let Some(root) = root().filter(|root| root.join("absorbed.json").is_file()) else {
        return Ok(catalog);
    };
    let payload: FeedPayload = serde_json::from_slice(
        &std::fs::read(root.join("absorbed.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let admission =
        admit(&catalog, &payload, env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    let effective = catalog.with_feed(&admission).map_err(|e| e.to_string())?;
    // Model a complete later client bundle, not a second overlay on an already overlaid
    // catalog (which the production admission boundary correctly refuses).
    let mut data = catalog.current_release_model_data().clone();
    data.data = effective.model_data().clone();
    data.rating_snapshot.models = effective.native_reasoning().to_vec();
    data.rating_snapshot.model_catalog_digest =
        hiroute_domain::CanonicalDigest::of(&data.data.models).map_err(|e| e.to_string())?;
    data.rating_snapshot.digest = data
        .rating_snapshot
        .computed_digest()
        .map_err(|e| e.to_string())?;
    for record in &payload.records {
        if let FeedRecord::Model { record, .. } = record {
            data.metadata_catalog
                .canonical_models
                .push(record.canonical.clone());
        }
    }
    data.metadata_catalog
        .canonical_models
        .sort_by(|a, b| a.model_key.cmp(&b.model_key));
    let registry = serde_json::to_vec(catalog.registry()).map_err(|e| e.to_string())?;
    let model_data = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
    let mut manifest = catalog.release_facts_manifest().clone();
    manifest.connector_registry_digest = hiroute_domain::CanonicalDigest::of_bytes(&registry);
    manifest.model_data_digest = hiroute_domain::CanonicalDigest::of_bytes(&model_data);
    manifest.cross_reference_digest = data
        .cross_reference_digest(catalog.registry())
        .map_err(|e| e.to_string())?;
    let manifest = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    TrustedReleaseCatalog::load_bundled_release_facts(&manifest, &manifest, &registry, &model_data)
        .map_err(|e| e.to_string())
}
pub(crate) fn directory_transport() -> Arc<dyn hiroute_integrations::ModelDirectoryTransportV1> {
    if root().is_some() {
        Arc::new(Directory)
    } else {
        Arc::new(hiroute_integrations::ReqwestModelDirectoryTransportV1)
    }
}
struct Directory;
impl hiroute_integrations::ModelDirectoryTransportV1 for Directory {
    fn get(
        &self,
        target: &hiroute_integrations::NormalizedModelConnectionTargetV1,
        _query: Option<&str>,
        credential: Option<hiroute_integrations::ModelConnectionProbeCredentialV1<'_>>,
        _timeout: std::time::Duration,
        _response_limit: usize,
    ) -> Result<
        hiroute_integrations::ModelDirectoryHttpResponseV1,
        hiroute_integrations::ModelDirectoryTransportErrorV1,
    > {
        if target.host_header() != "dashscope.aliyuncs.com" || credential.is_none() {
            return Err(hiroute_integrations::ModelDirectoryTransportErrorV1::Unavailable);
        }
        Ok(hiroute_integrations::ModelDirectoryHttpResponseV1 {
            status: 200,
            body: br#"{"object":"list","data":[{"id":"feed-regression","object":"model"}]}"#
                .to_vec(),
            truncated: false,
        })
    }
}
