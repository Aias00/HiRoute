//! Read the released rating batch through real CLI, independently of execution
//! source authority. The expected snapshot is the same candidate's immutable
//! production bundle embedded at compilation, never a constructed database row.
use super::InstalledProduct;
use super::windows_headless_business::{cli, failure};
use hiroute_application_api::{
    ModelCatalogResultV1, ModelRatingQueryItemV1, RatingSnapshotSelectionV1,
    ResolveModelRatingsResultV1, ResolveModelRatingsV1,
};
use hiroute_domain::{
    ExactNativeReasoningV1, NativeRatingConfigurationV1, RatingSnapshotV2, RatingUnknownReasonV1,
    RatingValueV1, ReasoningRenderModeV1,
};
use serde_json::{Value, json};

pub struct Ratings {
    query: ResolveModelRatingsV1,
    result: ResolveModelRatingsResultV1,
}
impl Ratings {
    pub fn run(product: &InstalledProduct) -> Self {
        let bundle: Value = serde_json::from_slice(include_bytes!(
            "../../../../assets/release-facts/current/bundle/model-data.json"
        ))
        .unwrap();
        let snapshot: RatingSnapshotV2 =
            serde_json::from_value(bundle["rating_snapshot"].clone()).unwrap();
        snapshot
            .validate()
            .expect("the independently embedded actual release snapshot must validate");
        let record = snapshot
            .records
            .iter()
            .find(|record| {
                matches!(
                    record.native_configuration,
                    NativeRatingConfigurationV1::Fixed { .. }
                ) && !matches!(record.overall, RatingValueV1::Unknown { .. })
            })
            .expect(
                "the actual current bundle must contain a scored explicit native configuration",
            );
        let NativeRatingConfigurationV1::Fixed { profile } = &record.native_configuration else {
            unreachable!()
        };
        let exact = ExactNativeReasoningV1::Fixed {
            profile: profile.clone(),
            render_mode: ReasoningRenderModeV1::NoControlParameter,
        };
        let before = public_identities(product);
        let reference = cli(
            product,
            &["models", "show"],
            Some(
                &json!({"kind":"reference","model_configuration_id":record.model_configuration_id}),
            ),
        );
        let ModelCatalogResultV1::Reference { model } =
            serde_json::from_value(reference["data"].clone()).unwrap()
        else {
            panic!("actual reference query returned another view")
        };
        assert_eq!(model.model_configuration_id, record.model_configuration_id);
        assert!(model.rating_snapshots.contains(&snapshot.snapshot_ref()));
        let missing_configuration = ExactNativeReasoningV1::Fixed {
            profile: "windows-native-uncollected-configuration".into(),
            render_mode: ReasoningRenderModeV1::NoControlParameter,
        };
        let query = ResolveModelRatingsV1 {
            snapshot: RatingSnapshotSelectionV1::Latest,
            // Deliberately non-alphabetic IDs/order: the producer must not sort
            // queries or mix a snapshot while resolving their different outcomes.
            items: vec![
                ModelRatingQueryItemV1 {
                    query_id: "z-actual-score".into(),
                    model_configuration_id: record.model_configuration_id.clone(),
                    exact_native_reasoning: exact.clone(),
                },
                ModelRatingQueryItemV1 {
                    query_id: "a-uncollected-configuration".into(),
                    model_configuration_id: record.model_configuration_id.clone(),
                    exact_native_reasoning: missing_configuration,
                },
                ModelRatingQueryItemV1 {
                    query_id: "m-unresolved-identity".into(),
                    model_configuration_id: "model.windows-native-unresolved-rating".into(),
                    exact_native_reasoning: exact,
                },
            ],
        };
        let result = resolve(product, &query);
        assert_eq!(result.snapshot_ref, snapshot.snapshot_ref());
        assert_eq!(result.items.len(), query.items.len());
        for (request, returned) in query.items.iter().zip(&result.items) {
            assert_eq!(request.query_id, returned.query_id);
            assert_eq!(
                request.model_configuration_id,
                returned.rating.requested_model
            );
            assert_eq!(
                NativeRatingConfigurationV1::from_exact(&request.exact_native_reasoning).unwrap(),
                returned.rating.requested_configuration
            );
        }
        let known = &result.items[0].rating;
        assert_eq!(
            known.matched_model.as_deref(),
            Some(record.model_configuration_id.as_str())
        );
        assert_eq!(
            known.matched_configuration.as_ref(),
            Some(&record.native_configuration)
        );
        assert_eq!(known.overall, record.overall);
        assert_eq!(known.coding, record.coding);
        assert_eq!(known.tool, record.tool);
        for (index, reason) in [
            (1, RatingUnknownReasonV1::ConfigurationNotInSnapshot),
            (2, RatingUnknownReasonV1::IdentityUnresolved),
        ] {
            let unresolved = &result.items[index].rating;
            assert!(
                unresolved.matched_model.is_none() && unresolved.matched_configuration.is_none()
            );
            for rating in [&unresolved.overall, &unresolved.coding, &unresolved.tool] {
                assert_eq!(rating, &RatingValueV1::Unknown { reason });
            }
        }
        let mut explicit = query.clone();
        explicit.snapshot = RatingSnapshotSelectionV1::Version {
            version: snapshot.version.clone(),
        };
        assert_eq!(resolve(product, &explicit), result);
        explicit.snapshot = RatingSnapshotSelectionV1::Version {
            version: "windows-native-snapshot-must-not-fall-back".into(),
        };
        failure(
            product,
            &["models", "show"],
            &request(&explicit),
            "SNAPSHOT_UNAVAILABLE",
        );
        let mut duplicate = query.clone();
        duplicate.items.push(duplicate.items[0].clone());
        failure(
            product,
            &["models", "show"],
            &request(&duplicate),
            "INVALID_ARGUMENTS",
        );
        let mut empty = query.clone();
        empty.items.clear();
        failure(
            product,
            &["models", "show"],
            &request(&empty),
            "INVALID_ARGUMENTS",
        );
        assert_eq!(
            public_identities(product),
            before,
            "reference ratings must not grant, edit or create execution sources/plans"
        );
        Self { query, result }
    }
    pub fn recovered(&self, product: &InstalledProduct) {
        assert_eq!(resolve(product, &self.query), self.result);
        let mut version = self.query.clone();
        version.snapshot = RatingSnapshotSelectionV1::Version {
            version: self.result.snapshot_ref.version.clone(),
        };
        assert_eq!(resolve(product, &version), self.result);
    }
}
fn request(query: &ResolveModelRatingsV1) -> Value {
    json!({"kind":"ratings", "query":query})
}
fn resolve(
    product: &InstalledProduct,
    query: &ResolveModelRatingsV1,
) -> ResolveModelRatingsResultV1 {
    let returned = cli(product, &["models", "show"], Some(&request(query)));
    let ModelCatalogResultV1::Ratings { result } =
        serde_json::from_value(returned["data"].clone()).unwrap()
    else {
        panic!("actual rating query returned another view")
    };
    result
}
fn public_identities(product: &InstalledProduct) -> Value {
    let sources = cli(product, &["compute", "list"], None);
    let plans = cli(product, &["routing", "list"], None);
    json!({
        "revisions":sources["data"]["revisions"],
        "sources":sources["data"]["sources"].as_array().unwrap().iter()
            .map(|source|json!({"id":source["source_id"],"revision":source["revision"]})).collect::<Vec<_>>(),
        "plans":plans["data"]["plans"]
    })
}
