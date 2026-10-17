//! Service-owned signed catalog state. A catalog and its admission generation travel together.

#[cfg(feature = "integration-test-hooks")]
pub(crate) mod product_fixture;
mod store;
pub(crate) use store::*;

use arc_swap::ArcSwap;
use hiroute_domain::CanonicalDigest;
use hiroute_integrations::{
    TrustedReleaseCatalog,
    model_feed::{FeedCounts, FeedDownload, VerifiedFeed, admit, retrieve_feed, verify_envelope},
};
use serde::Serialize;
use std::{
    ops::Deref,
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub(crate) struct ReleaseCatalogSnapshot {
    pub catalog: TrustedReleaseCatalog,
    pub generation: u64,
    pub accepted_sequence: u64,
}
impl Deref for ReleaseCatalogSnapshot {
    type Target = TrustedReleaseCatalog;
    fn deref(&self) -> &Self::Target {
        &self.catalog
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ModelFeedStatus {
    pub retrieval_enabled: bool,
    pub held: Option<HeldPublication>,
    pub highest_accepted_sequence: Option<u64>,
    pub last_attempt: Option<FeedAttempt>,
    pub verification: String,
    pub counts: FeedCounts,
    pub disclosure_acknowledged: bool,
    pub next_automatic_attempt_at: Option<u64>,
    pub generation: u64,
}

struct MutableFeed {
    state: FeedState,
    counts: FeedCounts,
    refused: Option<String>,
    refreshing: bool,
    retrieval_epoch: u64,
    last_refresh_result: Option<Result<(), String>>,
    next_attempt: Option<u64>,
    failures: u32,
}

type FeedVerifier = fn(&[u8]) -> Result<VerifiedFeed, hiroute_integrations::model_feed::FeedError>;

pub(crate) struct ModelFeedService {
    verify: FeedVerifier,
    pub embedded: Arc<TrustedReleaseCatalog>,
    current: ArcSwap<ReleaseCatalogSnapshot>,
    store: FeedStore,
    mutable: Mutex<MutableFeed>,
    refresh_finished: Condvar,
    active: AtomicBool,
    client_version: String,
}

impl ModelFeedService {
    pub fn open(
        root: &Path,
        embedded: TrustedReleaseCatalog,
        minimum_generation: u64,
        minimum_sequence: u64,
    ) -> Result<Self, String> {
        #[cfg(feature = "integration-test-hooks")]
        if product_fixture::root().is_some() {
            return Self::open_with_verifier(
                root,
                embedded,
                minimum_generation,
                minimum_sequence,
                product_fixture::verify,
            );
        }
        Self::open_with_verifier(
            root,
            embedded,
            minimum_generation,
            minimum_sequence,
            verify_envelope,
        )
    }
    fn open_with_verifier(
        root: &Path,
        embedded: TrustedReleaseCatalog,
        minimum_generation: u64,
        minimum_sequence: u64,
        verify: FeedVerifier,
    ) -> Result<Self, String> {
        let store = FeedStore::open(root)?;
        let (mut state, mut refused) = match store.read_state() {
            Ok(state) => (state, None),
            Err(reason) => (
                FeedState {
                    retrieval_enabled: false,
                    generation: minimum_generation
                        .checked_add(1)
                        .ok_or("feed_generation_overflow")?,
                    ..Default::default()
                },
                Some(reason),
            ),
        };
        let client_version = env!("CARGO_PKG_VERSION").to_owned();
        let mut effective = embedded.clone();
        let mut counts = FeedCounts::default();
        if let Some(held) = &state.held {
            let restored = store
                .blob(held)
                .and_then(|bytes| verify(&bytes).map_err(|e| e.to_string()))
                .and_then(|verified| {
                    if verified.payload.sequence != held.sequence
                        || verified.payload_digest != held.payload_digest
                    {
                        return Err("feed_cache_identity_mismatch".into());
                    }
                    let admission = admit(&embedded, &verified.payload, &client_version)
                        .map_err(|e| e.to_string())?;
                    Ok((
                        embedded.with_feed(&admission).map_err(|e| e.to_string())?,
                        admission.counts,
                    ))
                });
            match restored {
                Ok((catalog, new_counts)) => {
                    effective = catalog;
                    counts = new_counts;
                }
                Err(_) => {
                    state.held = None;
                    state.etag = None;
                    state.generation = state
                        .generation
                        .max(minimum_generation)
                        .checked_add(1)
                        .ok_or("feed_generation_overflow")?;
                    state.last_attempt = Some(FeedAttempt {
                        at_ms: now_ms(),
                        result: "validation".into(),
                        reason: Some("discarded_invalid_cache".into()),
                    });
                    store.commit(&state)?;
                }
            }
        }
        if state.generation < minimum_generation
            || state.highest_accepted.as_ref().map_or(0, |h| h.sequence) < minimum_sequence
        {
            // A lost/rolled-back commit record cannot reset acceptance, nor block frozen Plans.
            // Preserve the directory and use embedded facts with feed mutations refused.
            effective = embedded.clone();
            counts = FeedCounts::default();
            state.held = None;
            state.etag = None;
            state.retrieval_enabled = false;
            state.generation = minimum_generation
                .checked_add(1)
                .ok_or("feed_generation_overflow")?;
            refused = Some("feed_state_rollback_or_missing".into());
        }
        let generation = state.generation;
        let next_attempt = state
            .retrieval_enabled
            .then(|| now_ms() + startup_delay_ms());
        Ok(Self {
            verify,
            embedded: Arc::new(embedded),
            current: ArcSwap::from_pointee(ReleaseCatalogSnapshot {
                catalog: effective,
                generation,
                accepted_sequence: state
                    .highest_accepted
                    .as_ref()
                    .map_or(0, |h| h.sequence)
                    .max(minimum_sequence),
            }),
            store,
            mutable: Mutex::new(MutableFeed {
                state,
                counts,
                refused,
                refreshing: false,
                retrieval_epoch: 0,
                last_refresh_result: None,
                next_attempt,
                failures: 0,
            }),
            refresh_finished: Condvar::new(),
            active: AtomicBool::new(true),
            client_version,
        })
    }
    pub fn current(&self) -> Arc<ReleaseCatalogSnapshot> {
        self.current.load_full()
    }
    pub fn available_catalog(&self) -> Option<Arc<ReleaseCatalogSnapshot>> {
        self.active.load(Ordering::Acquire).then(|| self.current())
    }
    pub fn automatic_attempt_due(&self) -> bool {
        self.mutable.lock().is_ok_and(|guard| {
            guard.refused.is_none()
                && !guard.refreshing
                && guard.state.retrieval_enabled
                && guard.next_attempt.is_some_and(|at| at <= now_ms())
        })
    }
    pub fn status(&self) -> Result<ModelFeedStatus, String> {
        let guard = self.mutable.lock().map_err(|_| "feed_lock")?;
        let state = &guard.state;
        Ok(ModelFeedStatus {
            retrieval_enabled: state.retrieval_enabled,
            held: state.held.clone(),
            highest_accepted_sequence: state.highest_accepted.as_ref().map(|h| h.sequence),
            last_attempt: state.last_attempt.clone(),
            verification: guard.refused.clone().unwrap_or_else(|| {
                if state.held.is_some() {
                    "verified".into()
                } else {
                    "none".into()
                }
            }),
            counts: guard.counts.clone(),
            disclosure_acknowledged: state.disclosure_acknowledged,
            next_automatic_attempt_at: guard.next_attempt,
            generation: state.generation,
        })
    }
    pub fn settings(
        &self,
        enabled: Option<bool>,
        acknowledged: Option<bool>,
    ) -> Result<(), String> {
        let mut guard = self.mutable.lock().map_err(|_| "feed_lock")?;
        check_mutable(&guard)?;
        let mut next = guard.state.clone();
        if let Some(value) = enabled {
            next.retrieval_enabled = value;
            guard.retrieval_epoch = guard.retrieval_epoch.wrapping_add(1);
        }
        if let Some(value) = acknowledged {
            next.disclosure_acknowledged = value;
        }
        self.store.commit(&next)?;
        guard.state = next;
        guard.next_attempt = guard
            .state
            .retrieval_enabled
            .then(|| now_ms() + startup_delay_ms());
        Ok(())
    }
    pub fn removal_digest(&self) -> Result<CanonicalDigest, String> {
        let state = &self.mutable.lock().map_err(|_| "feed_lock")?.state;
        CanonicalDigest::of(state).map_err(|_| "feed_digest".into())
    }
    pub fn remove(
        &self,
        expected: &CanonicalDigest,
        disable: bool,
        install: impl FnOnce(u64, u64) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut guard = self.mutable.lock().map_err(|_| "feed_lock")?;
        check_mutable(&guard)?;
        if &CanonicalDigest::of(&guard.state).map_err(|_| "feed_digest")? != expected {
            return Err("feed_preview_changed".into());
        }
        let mut next = guard.state.clone();
        next.held = None;
        next.etag = None;
        next.generation = next
            .generation
            .checked_add(1)
            .ok_or("feed_generation_overflow")?;
        if disable {
            next.retrieval_enabled = false;
        }
        self.store.commit(&next)?;
        guard.state = next;
        guard.counts = FeedCounts::default();
        guard.next_attempt = guard
            .state
            .retrieval_enabled
            .then(|| now_ms() + startup_delay_ms());
        self.active.store(false, Ordering::Release);
        if let Err(error) = install(
            guard.state.generation,
            guard
                .state
                .highest_accepted
                .as_ref()
                .map_or(0, |h| h.sequence),
        ) {
            guard.refused = Some("feed_activation_requires_restart".into());
            return Err(error);
        }
        self.current.store(Arc::new(ReleaseCatalogSnapshot {
            catalog: (*self.embedded).clone(),
            generation: guard.state.generation,
            accepted_sequence: guard
                .state
                .highest_accepted
                .as_ref()
                .map_or(0, |h| h.sequence),
        }));
        self.active.store(true, Ordering::Release);
        self.store.retire_unreferenced(None);
        Ok(())
    }
    pub fn refresh(
        &self,
        install: impl FnOnce(u64, u64) -> Result<(), String>,
    ) -> Result<(), String> {
        #[cfg(feature = "integration-test-hooks")]
        if let Some(root) = product_fixture::root() {
            return self.refresh_using(|_| product_fixture::download(&root), install);
        }
        self.refresh_using(|etag| retrieve_feed(&self.client_version, etag), install)
    }
    fn refresh_using(
        &self,
        mut retrieve: impl FnMut(
            Option<&str>,
        )
            -> Result<FeedDownload, hiroute_integrations::model_feed::FeedError>,
        install: impl FnOnce(u64, u64) -> Result<(), String>,
    ) -> Result<(), String> {
        let (generation, epoch, etag) = {
            let mut guard = self.mutable.lock().map_err(|_| "feed_lock")?;
            check_mutable(&guard)?;
            if guard.refreshing {
                while guard.refreshing {
                    guard = self.refresh_finished.wait(guard).map_err(|_| "feed_lock")?;
                }
                return guard.last_refresh_result.clone().unwrap_or(Ok(()));
            }
            guard.refreshing = true;
            (
                guard.state.generation,
                guard.retrieval_epoch,
                guard.state.held.as_ref().and(guard.state.etag.clone()),
            )
        };
        let fetched = retrieve(etag.as_deref()).and_then(|value| {
            if matches!(value, FeedDownload::NotModified) && etag.is_none() {
                retrieve(None)
            } else {
                Ok(value)
            }
        });
        let mut guard = self.mutable.lock().map_err(|_| "feed_lock")?;
        guard.refreshing = false;
        if generation != guard.state.generation || epoch != guard.retrieval_epoch {
            guard.last_refresh_result = Some(Err("feed_retrieval_superseded".into()));
            self.refresh_finished.notify_all();
            return Err("feed_retrieval_superseded".into());
        }
        let result = match fetched {
            Ok(FeedDownload::Content { bytes, etag }) => (self.verify)(&bytes)
                .map_err(|e| failure_code(e.kind).into())
                .and_then(|verified| self.apply(&mut guard, verified, &bytes, etag, install)),
            Ok(FeedDownload::NotModified) if guard.state.held.is_some() => Ok("not_modified"),
            Ok(FeedDownload::NotPublished) => Ok("not_published"),
            Ok(FeedDownload::NotModified) => Err("cacheless_not_modified".into()),
            Err(error) => Err(failure_code(error.kind).into()),
        };
        let (code, reason) = match &result {
            Ok(code) => ((*code).to_owned(), None),
            Err(reason) => (attempt_failure_code(reason).into(), Some(reason.clone())),
        };
        guard.state.last_attempt = Some(FeedAttempt {
            at_ms: now_ms(),
            result: code,
            reason,
        });
        if result.is_ok() {
            guard.failures = 0;
        } else {
            guard.failures = guard.failures.saturating_add(1);
        }
        let delay = if guard.failures == 0 {
            330 * 60 * 1000 + jitter_ms(60 * 60 * 1000)
        } else {
            (15 * 60 * 1000_u64).saturating_mul(1 << guard.failures.min(4).saturating_sub(1))
        };
        guard.next_attempt = guard.state.retrieval_enabled.then(|| now_ms() + delay);
        let result = self.store.commit(&guard.state).and(result.map(|_| ()));
        guard.last_refresh_result = Some(result.clone());
        self.refresh_finished.notify_all();
        result
    }
    fn apply(
        &self,
        guard: &mut MutableFeed,
        verified: VerifiedFeed,
        bytes: &[u8],
        etag: Option<String>,
        install: impl FnOnce(u64, u64) -> Result<(), String>,
    ) -> Result<&'static str, String> {
        if let Some(high) = &guard.state.highest_accepted {
            if verified.payload.sequence < high.sequence {
                return Err("older_sequence".into());
            }
            if verified.payload.sequence == high.sequence {
                if verified.payload_digest != high.payload_digest {
                    return Err("same_sequence_conflicting_digest".into());
                }
                if guard.state.held.is_some() {
                    return Ok("already_current");
                }
            }
        }
        let admission = admit(&self.embedded, &verified.payload, &self.client_version)
            .map_err(|e| e.to_string())?;
        let catalog = self
            .embedded
            .with_feed(&admission)
            .map_err(|e| e.to_string())?;
        let mut next = guard.state.clone();
        next.generation = next
            .generation
            .checked_add(1)
            .ok_or("feed_generation_overflow")?;
        next.highest_accepted = Some(HighestAccepted {
            sequence: verified.payload.sequence,
            payload_digest: verified.payload_digest.clone(),
        });
        next.held = Some(HeldPublication {
            sequence: verified.payload.sequence,
            payload_digest: verified.payload_digest,
            envelope_digest: verified.envelope_digest.clone(),
            retrieved_at_ms: now_ms(),
            issued_at: verified.payload.issued_at,
        });
        next.etag = etag;
        next.last_success_at_ms = Some(now_ms());
        self.store.persist_blob(bytes, &verified.envelope_digest)?;
        self.store.commit(&next)?;
        guard.state = next;
        guard.counts = admission.counts;
        self.active.store(false, Ordering::Release);
        if let Err(error) = install(
            guard.state.generation,
            guard
                .state
                .highest_accepted
                .as_ref()
                .map_or(0, |h| h.sequence),
        ) {
            guard.refused = Some("feed_activation_requires_restart".into());
            return Err(error);
        }
        self.current.store(Arc::new(ReleaseCatalogSnapshot {
            catalog,
            generation: guard.state.generation,
            accepted_sequence: guard
                .state
                .highest_accepted
                .as_ref()
                .map_or(0, |h| h.sequence),
        }));
        self.active.store(true, Ordering::Release);
        self.store.retire_unreferenced(guard.state.held.as_ref());
        Ok("ok")
    }
}

fn check_mutable(guard: &MutableFeed) -> Result<(), String> {
    guard
        .refused
        .as_ref()
        .map_or(Ok(()), |reason| Err(reason.clone()))
}
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn jitter_ms(bound: u64) -> u64 {
    let mut bytes = [0; 8];
    let _ = getrandom::fill(&mut bytes);
    u64::from_le_bytes(bytes) % (bound + 1)
}
fn startup_delay_ms() -> u64 {
    2 * 60 * 1000 + jitter_ms(8 * 60 * 1000)
}

fn failure_code(kind: hiroute_integrations::model_feed::FeedFailureKind) -> &'static str {
    use hiroute_integrations::model_feed::FeedFailureKind::*;
    match kind {
        Network => "network",
        Size => "size",
        Signature => "signature",
        Schema => "schema",
        Validation => "validation",
        OlderSequence => "older_sequence",
    }
}
fn attempt_failure_code(reason: &str) -> &'static str {
    match reason {
        "network" => "network",
        "size" => "size",
        "signature" => "signature",
        "schema" => "schema",
        "older_sequence" => "older_sequence",
        _ => "validation",
    }
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
