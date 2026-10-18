//! Finite observation intents over the existing Local Control operations.
pub use hiroute_domain::{
    ObservationAncestryQueryV2, ObservationAncestryV2, ObservationCatalogPageV2,
    ObservationCatalogQueryV2, ObservationContentPageV2, ObservationContentQueryV2,
    ObservationFactsPageV2, ObservationFactsQueryV2, ObservationRequestPage,
    ObservationRequestQuery, ObservationSearchPageV2, ObservationSearchQueryV2,
    ObservationSessionCorrelationKindV1, ObservationSessionPageV2, ObservationSessionSearchPageV1,
    ObservationSessionSummaryV2, ObservationValueQueryV2, ObservationValueSummaryV2,
    PlanCompetenceFilter, PlanQualitySamplesPage, PlanQualitySamplesQuery,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationReadRequestV3 {
    pub schema: ObservationReadSchemaV3,
    pub intent: ObservationReadIntentV3,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ObservationReadSchemaV3 {
    #[serde(rename = "hiroute.observation.query/v3")]
    V3,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "view",
    content = "query",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ObservationReadIntentV3 {
    Status(ObservationStatusQueryV2),
    Sessions(ObservationRequestQuery),
    Timeline(ObservationRequestQuery),
    Catalog(ObservationCatalogQueryV2),
    Ancestry(ObservationAncestryQueryV2),
    Content(ObservationContentQueryV2),
    Facts(ObservationFactsQueryV2),
    Search(ObservationSearchQueryV2),
    SessionSearch(ObservationSearchQueryV2),
    Value(ObservationValueQueryV2),
    HomeValue(ObservationHomeValueQueryV2),
    ValueReport(ObservationValueReportOptionsV2),
    PlanQuality(PlanQualitySamplesQuery),
}
impl ObservationReadRequestV3 {
    pub fn new(intent: ObservationReadIntentV3) -> Self {
        Self {
            schema: ObservationReadSchemaV3::V3,
            intent,
        }
    }
    pub fn operation(&self) -> &'static str {
        match self.intent {
            ObservationReadIntentV3::Status(_)
            | ObservationReadIntentV3::Sessions(_)
            | ObservationReadIntentV3::Search(_)
            | ObservationReadIntentV3::SessionSearch(_) => "ListSessions",
            ObservationReadIntentV3::PlanQuality(_) => "GetPlanQualitySamples",
            ObservationReadIntentV3::Facts(_) => "GetRoutingReceipt",
            ObservationReadIntentV3::Value(_)
            | ObservationReadIntentV3::HomeValue(_)
            | ObservationReadIntentV3::ValueReport(_) => "GetValue",
            _ => "GetSession",
        }
    }
    pub fn protected_operation(&self) -> &'static str {
        match self.intent {
            ObservationReadIntentV3::Status(_) | ObservationReadIntentV3::Sessions(_) => {
                "ListSessionsV2"
            }
            ObservationReadIntentV3::Timeline(_) => "GetSessionTimelineV2",
            ObservationReadIntentV3::Catalog(_) => "ReadSessionCatalogV2",
            ObservationReadIntentV3::Ancestry(_) => "ReadSessionAncestryV2",
            ObservationReadIntentV3::Content(_) => "ReadSessionContentV2",
            ObservationReadIntentV3::Facts(_) => "ReadSessionFactsV2",
            ObservationReadIntentV3::Search(_) | ObservationReadIntentV3::SessionSearch(_) => {
                "SearchSessionContentV2"
            }
            ObservationReadIntentV3::Value(_)
            | ObservationReadIntentV3::HomeValue(_)
            | ObservationReadIntentV3::ValueReport(_) => "GetValueV2",
            ObservationReadIntentV3::PlanQuality(_) => "ReadPlanQualitySamplesV1",
        }
    }
}

pub use hiroute_domain::{
    DeletionDataClass, SessionDeletionOutcomeV2, SessionDeletionPreviewV2, SessionDeletionSpecV1,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationDeletePreviewRequestV2 {
    pub spec: SessionDeletionSpecV1,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationDeleteApplyRequestV2 {
    pub preview: SessionDeletionPreviewV2,
    pub accepted_digest: hiroute_domain::CanonicalDigest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationHomeValueQueryV2 {
    #[serde(default)]
    pub period: Option<crate::ValuePeriodV1>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationStatusQueryV2 {}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationValueReportOptionsV2 {
    pub agent_plan_id: Option<crate::AgentPlanId>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub period: Option<crate::ValuePeriodV1>,
    #[serde(default)]
    pub group_by: crate::ValueGroupByV1,
    pub currency: Option<String>,
    pub session_id: Option<crate::SessionId>,
}

/// Released V2 is a wire adapter; all intents execute the one current business model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservationWireVersion {
    Current,
    LegacyV2,
}

pub fn decode_observation_read(
    value: serde_json::Value,
) -> Result<(ObservationReadRequestV3, ObservationWireVersion), serde_json::Error> {
    let legacy = value.get("schema").and_then(serde_json::Value::as_str)
        == Some("hiroute.observation.query/v2");
    if !legacy {
        return serde_json::from_value(value).map(|r| (r, ObservationWireVersion::Current));
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Legacy {
        schema: LegacySchema,
        intent: ObservationReadIntentV3,
    }
    #[derive(Deserialize)]
    enum LegacySchema {
        #[serde(rename = "hiroute.observation.query/v2")]
        V2,
    }
    let old: Legacy = serde_json::from_value(value)?;
    let _ = old.schema;
    Ok((
        ObservationReadRequestV3::new(old.intent),
        ObservationWireVersion::LegacyV2,
    ))
}
pub fn observation_wire_value(
    request: &ObservationReadRequestV3,
    version: ObservationWireVersion,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(request)?;
    if version == ObservationWireVersion::LegacyV2 {
        value["schema"] = "hiroute.observation.query/v2".into();
    }
    Ok(value)
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    #[test]
    fn legacy_adapter_preserves_typed_authorization_payload_and_current_uses_v3() {
        let old = serde_json::json!({"schema":"hiroute.observation.query/v2","intent":{"view":"facts","query":{"request_id":"request/retained","limit":1,"cursor":null}}});
        let (current, version) = decode_observation_read(old.clone()).unwrap();
        assert_eq!(version, ObservationWireVersion::LegacyV2);
        assert_eq!(observation_wire_value(&current, version).unwrap(), old);
        assert_eq!(current.operation(), "GetRoutingReceipt");
        assert_eq!(current.protected_operation(), "ReadSessionFactsV2");
        let new = observation_wire_value(&current, ObservationWireVersion::Current).unwrap();
        assert_eq!(new["schema"], "hiroute.observation.query/v3");
        assert_ne!(
            hiroute_domain::CanonicalDigest::of(&new).unwrap(),
            hiroute_domain::CanonicalDigest::of(&old).unwrap()
        );
        assert_eq!(
            decode_observation_read(new).unwrap().1,
            ObservationWireVersion::Current
        );
        for schema in [
            "hiroute.observation.query/v2",
            "hiroute.observation.query/v3",
        ] {
            let mut flagged = old.clone();
            flagged["schema"] = schema.into();
            flagged["intent"]["query"]["include_routing_observations"] = true.into();
            assert!(
                decode_observation_read(flagged).is_err(),
                "unreleased projection flags have no support adapter"
            );
        }
    }
}
