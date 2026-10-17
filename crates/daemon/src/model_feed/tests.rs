use super::*;
use hiroute_integrations::model_feed::{FeedError, FeedPayload, MODEL_FEED_SCHEMA};

use base64::{Engine, engine::general_purpose::STANDARD};
use ring::signature::{Ed25519KeyPair, KeyPair};
fn test_key() -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[19; 32]).unwrap()
}
// Test-only trust root. Production open never accepts a runtime key/URL override.
fn verify_fixture(bytes: &[u8]) -> Result<VerifiedFeed, FeedError> {
    let keys=serde_json::to_vec(&serde_json::json!({"keys":[{"key_id":"daemon-test","ed25519":STANDARD.encode(test_key().public_key().as_ref())}]})).unwrap();
    hiroute_integrations::model_feed::verify_envelope_with_keys(bytes, &keys)
}
fn payload(sequence: u64) -> Vec<u8> {
    sign(&FeedPayload {
        schema: MODEL_FEED_SCHEMA.into(),
        sequence,
        issued_at: "2026-10-18T00:00:00Z".into(),
        source_revision: "0123456789abcdef0123456789abcdef01234567".into(),
        records: vec![],
        featured: vec![],
    })
}
fn sign(payload: &FeedPayload) -> Vec<u8> {
    let bytes = serde_json::to_vec(payload).unwrap();
    serde_json::to_vec(&serde_json::json!({"schema":hiroute_integrations::model_feed::MODEL_FEED_ENVELOPE_SCHEMA,"payload":STANDARD.encode(&bytes),"signatures":[{"key_id":"daemon-test","ed25519":STANDARD.encode(test_key().sign(&bytes).as_ref())}]})).unwrap()
}

#[test]
fn model_feed_restart_and_removal_preserve_captured_catalog_without_authorizing_new_use() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    let bytes = sign(&super::fixtures::publication(17));
    service
        .refresh_using(
            |_| {
                Ok(FeedDownload::Content {
                    bytes: bytes.clone(),
                    etag: None,
                })
            },
            |_, _| Ok(()),
        )
        .unwrap();
    let captured = service.current();
    assert!(
        captured
            .model_data()
            .model(super::fixtures::MODEL)
            .is_some()
    );
    drop(service);
    let restarted = open(root.path(), captured.generation);
    assert_eq!(
        restarted.current().model_data_provenance(),
        captured.model_data_provenance()
    );
    restarted
        .remove(&restarted.removal_digest().unwrap(), true, |_, _| Ok(()))
        .unwrap();
    assert!(
        restarted
            .current()
            .model_data()
            .model(super::fixtures::MODEL)
            .is_none()
    );
    assert!(
        captured
            .model_data()
            .model(super::fixtures::MODEL)
            .is_some()
    );
    let generation = restarted.current().generation;
    drop(restarted);
    let restarted = open(root.path(), generation);
    assert!(
        restarted
            .current()
            .model_data()
            .model(super::fixtures::MODEL)
            .is_none()
    );
    assert_eq!(
        restarted.status().unwrap().highest_accepted_sequence,
        Some(17)
    );
    restarted
        .refresh_using(
            |_| {
                Ok(FeedDownload::Content {
                    bytes: bytes.clone(),
                    etag: None,
                })
            },
            |_, _| Ok(()),
        )
        .unwrap();
    assert!(
        restarted
            .current()
            .model_data()
            .model(super::fixtures::MODEL)
            .is_some()
    );
}
fn open(path: &Path, min: u64) -> ModelFeedService {
    ModelFeedService::open_with_verifier(
        path,
        crate::release_catalog::current_fixture_catalog(),
        min,
        0,
        verify_fixture,
    )
    .unwrap()
}
fn fetch(service: &ModelFeedService, sequence: u64) -> Result<(), String> {
    service.refresh_using(
        |_| {
            Ok(FeedDownload::Content {
                bytes: payload(sequence),
                etag: Some("test-etag".into()),
            })
        },
        |_, _| Ok(()),
    )
}

#[test]
fn model_feed_delete_preserves_high_water_and_identical_publication_restores_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    fetch(&service, 10).unwrap();
    let old = service.current();
    service
        .remove(&service.removal_digest().unwrap(), true, |_, _| Ok(()))
        .unwrap();
    assert!(!service.status().unwrap().retrieval_enabled);
    assert!(service.status().unwrap().held.is_none());
    assert_eq!(old.generation, 1);
    assert_eq!(old.accepted_sequence, 10);
    assert_eq!(service.current().accepted_sequence, 10);
    let restarted = open(root.path(), 2);
    assert_eq!(
        restarted.status().unwrap().highest_accepted_sequence,
        Some(10)
    );
    assert!(fetch(&restarted, 9).is_err());
    assert_eq!(
        restarted.status().unwrap().last_attempt.unwrap().result,
        "older_sequence"
    );
    fetch(&restarted, 10).unwrap();
    assert_eq!(restarted.status().unwrap().held.unwrap().sequence, 10);
    assert_eq!(restarted.current().generation, 3);
}

#[test]
fn model_feed_cacheless_304_retries_without_conditional_header() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    fetch(&service, 10).unwrap();
    service
        .remove(&service.removal_digest().unwrap(), false, |_, _| Ok(()))
        .unwrap();
    let mut calls = 0;
    service
        .refresh_using(
            |etag| {
                assert!(etag.is_none());
                calls += 1;
                if calls == 1 {
                    Ok(FeedDownload::NotModified)
                } else {
                    Ok(FeedDownload::Content {
                        bytes: payload(10),
                        etag: None,
                    })
                }
            },
            |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(calls, 2);
    assert!(service.status().unwrap().held.is_some());
}

#[test]
fn model_feed_corrupt_state_is_preserved_and_mutations_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    fetch(&service, 5).unwrap();
    drop(service);
    let path = root.path().join("model-feed/state.json");
    std::fs::write(&path, b"future schema").unwrap();
    let reopened = open(root.path(), 1);
    assert!(reopened.status().unwrap().held.is_none());
    assert!(reopened.settings(Some(true), None).is_err());
    assert!(fetch(&reopened, 6).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"future schema");
}

#[test]
fn model_feed_crash_before_state_commit_does_not_adopt_unreferenced_blob() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    fetch(&service, 5).unwrap();
    let bytes = payload(6);
    service
        .store
        .persist_blob(&bytes, &CanonicalDigest::of_bytes(&bytes))
        .unwrap();
    drop(service);
    let reopened = open(root.path(), 1);
    assert_eq!(reopened.status().unwrap().held.unwrap().sequence, 5);
}

#[test]
fn model_feed_crash_after_commit_recovers_catalog_and_generation() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    fetch(&service, 5).unwrap();
    assert!(
        service
            .refresh_using(
                |_| Ok(FeedDownload::Content {
                    bytes: payload(6),
                    etag: None
                }),
                |_, _| Err("injected activation interruption".into())
            )
            .is_err()
    );
    assert!(service.available_catalog().is_none());
    drop(service);
    let reopened = open(root.path(), 1);
    assert_eq!(reopened.current().generation, 2);
    assert_eq!(reopened.status().unwrap().held.unwrap().sequence, 6);
}

#[test]
fn model_feed_generation_is_captured_with_catalog_before_dependency_activation() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    let before = service.current();
    service
        .refresh_using(
            |_| {
                Ok(FeedDownload::Content {
                    bytes: payload(1),
                    etag: None,
                })
            },
            |generation, sequence| {
                assert_eq!(sequence, 1);
                assert_eq!(generation, 1);
                assert_eq!(service.current().generation, 0);
                assert!(service.available_catalog().is_none());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(before.generation, 0);
    assert_eq!(service.current().generation, 1);
}

#[test]
fn model_feed_disabling_then_reenabling_invalidates_in_flight_retrieval() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    let result = service.refresh_using(
        |_| {
            service.settings(Some(false), None).unwrap();
            service.settings(Some(true), None).unwrap();
            Ok(FeedDownload::Content {
                bytes: payload(1),
                etag: None,
            })
        },
        |_, _| panic!("superseded retrieval cannot activate"),
    );
    assert!(result.is_err());
    assert!(service.status().unwrap().held.is_none());
}

#[test]
fn model_feed_invalid_cache_retains_high_water_on_reopen() {
    let root = tempfile::tempdir().unwrap();
    let service = open(root.path(), 0);
    fetch(&service, 5).unwrap();
    let held = service.status().unwrap().held.unwrap();
    std::fs::remove_file(
        root.path()
            .join("model-feed/envelopes")
            .join(format!("{}.json", &held.envelope_digest.as_str()[7..])),
    )
    .unwrap();
    drop(service);
    let reopened = open(root.path(), 1);
    assert!(reopened.status().unwrap().held.is_none());
    assert_eq!(
        reopened.status().unwrap().highest_accepted_sequence,
        Some(5)
    );
    assert!(fetch(&reopened, 4).is_err());
    fetch(&reopened, 5).unwrap();
}
