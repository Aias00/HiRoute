use hiroute_domain::CanonicalDigest;
use hiroute_integrations::model_feed::MAX_MODEL_FEED_BYTES;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub const STATE_SCHEMA: &str = "hiroute.model-feed.state/v1";
const MAX_STATE_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HighestAccepted {
    pub sequence: u64,
    pub payload_digest: CanonicalDigest,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeldPublication {
    pub sequence: u64,
    pub payload_digest: CanonicalDigest,
    pub envelope_digest: CanonicalDigest,
    pub retrieved_at_ms: u64,
    pub issued_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedAttempt {
    pub at_ms: u64,
    pub result: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedState {
    pub schema: String,
    #[serde(default = "enabled")]
    pub retrieval_enabled: bool,
    pub disclosure_acknowledged: bool,
    pub highest_accepted: Option<HighestAccepted>,
    pub generation: u64,
    pub held: Option<HeldPublication>,
    pub last_attempt: Option<FeedAttempt>,
    pub last_success_at_ms: Option<u64>,
    pub etag: Option<String>,
}

fn enabled() -> bool {
    true
}
impl Default for FeedState {
    fn default() -> Self {
        Self {
            schema: STATE_SCHEMA.into(),
            retrieval_enabled: true,
            disclosure_acknowledged: false,
            highest_accepted: None,
            generation: 0,
            held: None,
            last_attempt: None,
            last_success_at_ms: None,
            etag: None,
        }
    }
}

pub struct FeedStore {
    root: PathBuf,
}
impl FeedStore {
    pub fn open(storage_root: &Path) -> Result<Self, String> {
        let root = storage_root.join("model-feed");
        private_directory(&root)?;
        private_directory(&root.join("envelopes"))?;
        Ok(Self { root })
    }
    pub fn read_state(&self) -> Result<FeedState, String> {
        let bytes = match bounded_read(&self.root.join("state.json"), MAX_STATE_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(FeedState::default());
            }
            Err(_) => return Err("feed_state_unreadable".into()),
        };
        let state: FeedState =
            serde_json::from_slice(&bytes).map_err(|_| "feed_state_invalid_or_newer_schema")?;
        if state.schema != STATE_SCHEMA {
            return Err("feed_state_newer_schema".into());
        }
        if let Some(highest) = &state.highest_accepted
            && (highest.sequence == 0
                || CanonicalDigest::parse(highest.payload_digest.as_str()).is_err())
        {
            return Err("feed_state_invalid".into());
        }
        if let Some(held) = &state.held
            && (CanonicalDigest::parse(held.envelope_digest.as_str()).is_err()
                || !state.highest_accepted.as_ref().is_some_and(|high| {
                    high.sequence == held.sequence && high.payload_digest == held.payload_digest
                }))
        {
            return Err("feed_state_invalid".into());
        }
        Ok(state)
    }
    pub fn commit(&self, state: &FeedState) -> Result<(), String> {
        let bytes = serde_json::to_vec(state).map_err(|_| "feed_state_encoding")?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err("feed_state_too_large".into());
        }
        hiroute_platform::durable::write_private_atomic(&self.root.join("state.json"), &bytes)
            .map_err(|_| "feed_state_commit_failed".into())
    }
    fn blob_path(&self, digest: &CanonicalDigest) -> Result<PathBuf, String> {
        CanonicalDigest::parse(digest.as_str()).map_err(|_| "feed_blob_invalid_digest")?;
        Ok(self
            .root
            .join("envelopes")
            .join(format!("{}.json", &digest.as_str()[7..])))
    }
    pub fn blob(&self, held: &HeldPublication) -> Result<Vec<u8>, String> {
        let bytes = bounded_read(
            &self.blob_path(&held.envelope_digest)?,
            MAX_MODEL_FEED_BYTES,
        )
        .map_err(|_| "feed_blob_unreadable")?;
        if CanonicalDigest::of_bytes(&bytes) != held.envelope_digest {
            return Err("feed_blob_digest_mismatch".into());
        }
        Ok(bytes)
    }
    pub fn persist_blob(&self, bytes: &[u8], digest: &CanonicalDigest) -> Result<(), String> {
        if bytes.len() > MAX_MODEL_FEED_BYTES || &CanonicalDigest::of_bytes(bytes) != digest {
            return Err("feed_blob_invalid".into());
        }
        let path = self.blob_path(digest)?;
        match bounded_read(&path, MAX_MODEL_FEED_BYTES) {
            Ok(existing) if existing == bytes => return Ok(()),
            Ok(_) => return Err("feed_blob_conflict".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("feed_blob_unreadable".into()),
        }
        hiroute_platform::durable::write_private_atomic(&path, bytes)
            .map_err(|_| "feed_blob_write_failed".into())
    }
    pub fn retire_unreferenced(&self, held: Option<&HeldPublication>) {
        let keep = held.and_then(|held| self.blob_path(&held.envelope_digest).ok());
        if let Ok(entries) = std::fs::read_dir(self.root.join("envelopes")) {
            for entry in entries.flatten().take(8192) {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.is_ascii()
                    && name.len() == 69
                    && name.ends_with(".json")
                    && name[..64].bytes().all(|b| b.is_ascii_hexdigit())
                    && Some(entry.path()) != keep
                    && entry.file_type().is_ok_and(|t| t.is_file())
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

fn private_directory(path: &Path) -> Result<(), String> {
    hiroute_platform::private_fs::prepare_storage_directory(path)
        .map_err(|_| "feed_directory_unavailable".into())
}

fn bounded_read(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut file = hiroute_platform::private_fs::open_private_file(path)?;
    hiroute_platform::private_fs::verify_single_link(&file)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(std::io::Error::other("feed file exceeds bound"));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::other("feed file exceeds bound"));
    }
    Ok(bytes)
}
