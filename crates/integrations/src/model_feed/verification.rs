use base64::{Engine, engine::general_purpose::STANDARD};
use hiroute_domain::CanonicalDigest;
use serde::{Deserialize, Serialize};

use super::{
    FeedError, FeedFailureKind, FeedPayload, MAX_MODEL_FEED_BYTES, MODEL_FEED_ENVELOPE_SCHEMA,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedPublicKey {
    pub key_id: String,
    pub ed25519: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedPublicKeys {
    pub keys: Vec<FeedPublicKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: String,
    payload: String,
    signatures: Vec<Signature>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    key_id: String,
    ed25519: String,
}

#[derive(Clone, Debug)]
pub struct VerifiedFeed {
    pub payload: FeedPayload,
    pub payload_bytes: Vec<u8>,
    pub payload_digest: CanonicalDigest,
    pub envelope_digest: CanonicalDigest,
}

pub fn verify_envelope(bytes: &[u8]) -> Result<VerifiedFeed, FeedError> {
    let keys: FeedPublicKeys = serde_json::from_slice(include_bytes!(
        "../../../../assets/model-feed/public-keys.json"
    ))
    .map_err(|_| FeedError::new(FeedFailureKind::Signature, "invalid compiled key set"))?;
    verify(bytes, &keys)
}

#[cfg(any(test, feature = "test-support"))]
pub fn verify_envelope_with_keys(
    bytes: &[u8],
    keys_json: &[u8],
) -> Result<VerifiedFeed, FeedError> {
    let keys = serde_json::from_slice(keys_json)
        .map_err(|_| FeedError::new(FeedFailureKind::Signature, "invalid test key set"))?;
    verify(bytes, &keys)
}

fn verify(bytes: &[u8], keys: &FeedPublicKeys) -> Result<VerifiedFeed, FeedError> {
    if bytes.len() > MAX_MODEL_FEED_BYTES {
        return Err(FeedError::new(
            FeedFailureKind::Size,
            "envelope exceeds bound",
        ));
    }
    let envelope: Envelope = serde_json::from_slice(bytes)
        .map_err(|_| FeedError::new(FeedFailureKind::Schema, "invalid envelope"))?;
    if envelope.schema != MODEL_FEED_ENVELOPE_SCHEMA
        || !(1..=2).contains(&envelope.signatures.len())
    {
        return Err(FeedError::new(
            FeedFailureKind::Schema,
            "unsupported envelope schema or signature count",
        ));
    }
    let payload = STANDARD
        .decode(&envelope.payload)
        .map_err(|_| FeedError::new(FeedFailureKind::Schema, "invalid payload encoding"))?;
    let trusted = envelope.signatures.iter().any(|signature| {
        keys.keys
            .iter()
            .filter(|key| key.key_id == signature.key_id)
            .any(|key| {
                let (Ok(public), Ok(sig)) = (
                    STANDARD.decode(&key.ed25519),
                    STANDARD.decode(&signature.ed25519),
                ) else {
                    return false;
                };
                public.len() == 32
                    && sig.len() == 64
                    && ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public)
                        .verify(&payload, &sig)
                        .is_ok()
            })
    });
    if !trusted {
        return Err(FeedError::new(
            FeedFailureKind::Signature,
            "no trusted signature verifies",
        ));
    }
    // Authentication precedes payload interpretation; unsigned facts never reach admission.
    let parsed: FeedPayload = serde_json::from_slice(&payload)
        .map_err(|_| FeedError::new(FeedFailureKind::Schema, "invalid payload contract"))?;
    parsed.validate()?;
    Ok(VerifiedFeed {
        payload: parsed,
        payload_digest: CanonicalDigest::of_bytes(&payload),
        envelope_digest: CanonicalDigest::of_bytes(bytes),
        payload_bytes: payload,
    })
}
