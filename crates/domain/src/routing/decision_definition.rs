//! The current v1 model-decision vocabulary. Execution policy is kept out of the wire.
use serde::{Deserialize, Serialize};

use super::DegreePolicyV1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OrdinalLevelV1 {
    pub id: String,
    pub criterion: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename = "ordinal", deny_unknown_fields)]
pub struct OrdinalDefinitionV1 {
    pub instructions: String,
    pub levels: Vec<OrdinalLevelV1>,
}

// Serde's tagged-struct deserializer does not consume its own emitted tag when
// deny_unknown_fields is enabled. Read the public ordinal tag explicitly, while
// retaining the untagged form accepted by the existing v1 reader.
impl<'de> Deserialize<'de> for OrdinalDefinitionV1 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Kind {
            Ordinal,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(default, rename = "kind")]
            _kind: Option<Kind>,
            instructions: String,
            levels: Vec<OrdinalLevelV1>,
        }
        let Wire {
            _kind: _,
            instructions,
            levels,
        } = Wire::deserialize(deserializer)?;
        Ok(Self {
            instructions,
            levels,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryOptionV1 {
    pub id: String,
    pub criterion: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refinement: Option<OrdinalDefinitionV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionDefinitionV1 {
    Ordinal {
        instructions: String,
        levels: Vec<OrdinalLevelV1>,
    },
    Categorical {
        instructions: String,
        options: Vec<CategoryOptionV1>,
    },
}

impl From<OrdinalDefinitionV1> for DecisionDefinitionV1 {
    fn from(value: OrdinalDefinitionV1) -> Self {
        Self::Ordinal {
            instructions: value.instructions,
            levels: value.levels,
        }
    }
}

impl DegreePolicyV1 {
    pub fn definition(&self) -> OrdinalDefinitionV1 {
        OrdinalDefinitionV1 {
            instructions: self.instructions.clone(),
            levels: vec![
                OrdinalLevelV1 {
                    id: "simple".into(),
                    criterion: self.simple.clone(),
                },
                OrdinalLevelV1 {
                    id: "complex".into(),
                    criterion: self.complex.clone(),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ordinal_definition_reads_its_public_tag_and_existing_untagged_form() {
        let value = DegreePolicyV1::default().definition();
        let mut wire = serde_json::to_value(&value).unwrap();
        assert_eq!(wire["kind"], "ordinal");
        assert_eq!(
            serde_json::from_value::<OrdinalDefinitionV1>(wire.clone()).unwrap(),
            value
        );
        wire.as_object_mut().unwrap().remove("kind");
        assert_eq!(
            serde_json::from_value::<OrdinalDefinitionV1>(wire.clone()).unwrap(),
            value
        );
        wire["kind"] = json!("categorical");
        assert!(serde_json::from_value::<OrdinalDefinitionV1>(wire.clone()).is_err());
        wire["kind"] = json!("ordinal");
        wire["unrecognized"] = json!(true);
        assert!(serde_json::from_value::<OrdinalDefinitionV1>(wire).is_err());
    }
}
