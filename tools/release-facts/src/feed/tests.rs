use super::*;

fn current() -> TrustedReleaseCatalog {
    load_catalog(|file| {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/release-facts/current/bundle")
                .join(file),
        )
        .map_err(|e| e.to_string())
    })
    .unwrap()
}
#[test]
fn model_feed_projection_keeps_embedded_models_editorial_only() {
    let current = current();
    let clients = vec![SupportedFeedClient {
        version: "0.3.2".into(),
        catalog: current.clone(),
    }];
    let payload = project(
        &current,
        &clients,
        3,
        "2026-10-18T00:00:00Z",
        "0123456789abcdef0123456789abcdef01234567",
        vec![],
        None,
    )
    .unwrap();
    assert!(payload.records.is_empty());
    assert_eq!(payload.sequence, 3);
    assert_eq!(
        project(
            &current,
            &clients,
            3,
            &payload.issued_at,
            &payload.source_revision,
            vec![],
            Some(&payload)
        )
        .unwrap(),
        payload
    );
}
#[test]
fn model_feed_bootstrap_checks_real_shared_metadata_and_is_repeatable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    build(&root, true).unwrap();
    build(&root, true).unwrap();
}
#[test]
fn model_feed_editorial_record_cannot_introduce_untrusted_links() {
    let current = current();
    let clients = vec![SupportedFeedClient {
        version: "0.3.2".into(),
        catalog: current.clone(),
    }];
    let featured = vec![FeaturedModel {
        id: "test-model-update".into(),
        kind: FeaturedKind::ModelUpdate,
        model_configuration_id: current.model_data().models[0]
            .model_configuration_id
            .clone(),
        headline: LocalizedFeedText {
            en: "Update".into(),
            zh: "更新".into(),
        },
        note: LocalizedFeedText {
            en: "Provider announcement".into(),
            zh: "供应商公告".into(),
        },
        links: vec!["https://elsewhere.example/redirect".into()],
        included_on: "2026-10-18".into(),
    }];
    assert!(
        project(
            &current,
            &clients,
            3,
            "2026-10-18T00:00:00Z",
            "0123456789abcdef0123456789abcdef01234567",
            featured,
            None
        )
        .is_err()
    );
}
