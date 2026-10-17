use super::*;

#[test]
fn stock_questions_and_composition_are_the_public_frozen_c_full() {
    let p = NativeProtocolV1::default();
    let mut j = JudgmentSettingsV1::default();
    j.competence.floor_millis = 317;
    j.degree.simple_threshold_millis = 819;
    p.stock_assessment(&mut j);
    assert_eq!(j.competence.floor_millis, 317);
    assert_eq!(j.degree.simple_threshold_millis, 819);
    assert_eq!(composed_assessment(Some(&p), &j), stock_c_questions()["q1"]);
    assert_eq!(
        CanonicalDigest::of(&stock_c_questions()).unwrap().as_str(),
        format!("sha256:{C_FULL_CHECKSUM}")
    );
    let review = AssessmentReviewV1::new(Some(&p), &j, AssessmentAdoptionV1::Stock, None).unwrap();
    review.validate(Some(&p), &j).unwrap();
    assert_eq!(review.validate(None, &j), Err(NativeProtocolError::Review));
    j.competence.instructions.push_str(" changed");
    assert_eq!(
        review.validate(Some(&p), &j),
        Err(NativeProtocolError::Review)
    );
}

#[test]
fn frozen_content_and_effective_authoring_must_both_agree() {
    let p = NativeProtocolV1::default();
    let mut j = JudgmentSettingsV1::default();
    p.stock_assessment(&mut j);
    let frozen = FrozenNativeProtocolV1::freeze(
        &p,
        j.degree.definition().into(),
        [(super::super::SMART_SAVING_SCOPE_ID.into(), j.clone())],
    )
    .unwrap();
    frozen.validate_smart(&j).unwrap();
    assert_eq!(
        serde_json::from_value::<FrozenNativeProtocolV1>(serde_json::to_value(&frozen).unwrap())
            .unwrap(),
        frozen
    );
    let mut corrupt = frozen.clone();
    corrupt.protocol.prior_context_max_turns = 2;
    assert_eq!(corrupt.validate(), Err(NativeProtocolError::DigestMismatch));
    let mut changed = j.clone();
    changed.degree.simple_threshold_millis = 900;
    assert_eq!(
        frozen.validate_smart(&changed),
        Err(NativeProtocolError::ContentMismatch)
    );
    let mut changed = p.clone();
    changed.fixed.applicability["instructions"] = json!("substituted default");
    assert_eq!(changed.validate(), Err(NativeProtocolError::FixedContent));
    let mut changed = p.clone();
    changed.schema = "hiroute.system-one-c/v99".into();
    assert_eq!(changed.validate(), Err(NativeProtocolError::Unsupported));
}

#[test]
fn legacy_omission_and_rubric_digest_remain_byte_stable() {
    let bytes = include_str!("authoring/fixtures/released-plan-editor.v2.json");
    let fixture: Value = serde_json::from_str(bytes).unwrap();
    let editor: super::super::PlanEditorStateV2 =
        serde_json::from_value(fixture["editor"].clone()).unwrap();
    let bytes = serde_json::to_vec(&editor).unwrap();
    let reopened: super::super::PlanEditorStateV2 = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
    assert!(
        serde_json::to_value(&editor.smart)
            .unwrap()
            .get("native_protocol")
            .is_none()
    );
    assert!(
        serde_json::to_value(&editor.smart)
            .unwrap()
            .get("assessment_review")
            .is_none()
    );
    assert!(editor.smart.native_protocol.is_none());
    assert!(editor.smart.assessment_review.is_none());
    let j = JudgmentSettingsV1::default();
    assert_eq!(
        j.execution_policy("old").criteria_digest,
        Some(CanonicalDigest::of(&(&j.competence.instructions, &j.competence.criteria)).unwrap())
    );
    assert_eq!(j.degree.simple_threshold_millis, 800);
    assert_eq!(j.competence.floor_millis, 500);
}
