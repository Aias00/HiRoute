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

fn without_model(id: &str) -> TrustedReleaseCatalog {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut input =
        crate::read_compiler_input(&root.join("assets/release-facts/current/compiler-input.json"))
            .unwrap();
    let data = &mut input.model_data.data;
    data.models.retain(|m| m.model_configuration_id != id);
    data.model_endpoint_capabilities
        .retain(|c| c.model_configuration_id != id);
    data.price_rates.retain(|p| p.model_configuration_id != id);
    for offer in &mut data.offers {
        offer.model_configuration_ids.retain(|member| member != id);
    }
    data.offers
        .retain(|offer| !offer.model_configuration_ids.is_empty());
    let ratings = &mut input.model_data.rating_snapshot;
    ratings.models.retain(|m| m.model_configuration_id != id);
    ratings.records.retain(|r| r.model_configuration_id != id);
    ratings.model_catalog_digest = CanonicalDigest::of(&data.models).unwrap();
    ratings.digest = ratings.computed_digest().unwrap();
    let output = crate::compile_release_facts(&input).unwrap();
    load_catalog(|file| Ok(output.files[file].clone())).unwrap()
}

#[test]
fn model_feed_projection_closes_a_real_model_graph_and_preserves_first_inclusion() {
    let current = current();
    let id = current
        .model_data()
        .offers
        .iter()
        .find(|offer| offer.billing_class == hiroute_domain::BillingClass::Paid)
        .expect("current catalog has a paid offer")
        .model_configuration_ids[0]
        .clone();
    let clients = vec![
        SupportedFeedClient {
            version: "0.3.2".into(),
            catalog: without_model(&id),
        },
        SupportedFeedClient {
            version: "0.3.3".into(),
            catalog: current.clone(),
        },
    ];
    let first = project(
        &current,
        &clients,
        1,
        "2026-10-18T00:00:00Z",
        &"1".repeat(40),
        vec![],
        None,
    )
    .unwrap();
    assert!(
        first
            .records
            .iter()
            .any(|r| r.kind() == "model" && r.id() == id)
    );
    assert!(
        first
            .records
            .iter()
            .any(|r| r.kind() == "model_endpoint_capability")
    );
    assert!(first.records.iter().any(|r| r.kind() == "offer"));
    let admission = admit(&clients[0].catalog, &first, "0.3.2").unwrap();
    assert_eq!(admission.counts.unresolved, 0);
    let effective = clients[0].catalog.with_feed(&admission).unwrap();
    assert_eq!(
        effective.model_data().model(&id),
        current.model_data().model(&id)
    );
    assert!(
        admit(&clients[1].catalog, &first, "0.3.3")
            .unwrap()
            .records
            .iter()
            .all(|r| r.outcome == RecordAdmission::IgnoredEmbedded)
    );
    let second = project(
        &current,
        &clients,
        2,
        "2026-10-19T00:00:00Z",
        &"2".repeat(40),
        vec![],
        Some(&first),
    )
    .unwrap();
    assert!(
        second
            .records
            .iter()
            .all(|r| r.included_on() == "2026-10-18")
    );
    validate_transition(&first, &second).unwrap();
    let mut changed = second.clone();
    if let FeedRecord::Model { included_on, .. } = changed
        .records
        .iter_mut()
        .find(|r| r.kind() == "model")
        .unwrap()
    {
        *included_on = "2026-10-19".into();
    }
    assert!(validate_transition(&first, &changed).is_err());
    changed = second;
    changed.sequence = first.sequence;
    assert!(validate_transition(&first, &changed).is_err());
}

#[test]
fn model_feed_bootstrap_rejects_invalid_editorial_inputs_before_release_lookup() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("assets/model-feed");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("policy.json"),
        r#"{"minimum_feed_client":"0.3.2"}"#,
    )
    .unwrap();
    std::fs::write(
        directory.join("publication.json"),
        r#"{"sequence":1,"issued_at":"2026-02-30T00:00:00Z"}"#,
    )
    .unwrap();
    std::fs::write(directory.join("featured.json"), "[]").unwrap();
    let error = build(root.path(), true).unwrap_err();
    assert!(
        !error.contains("cannot read"),
        "schema validation must run in bootstrap: {error}"
    );
}
