//! One owned Hermes default/provider transition, with private conditional restore state.
use super::hermes_yaml::{HermesDocument, HermesNativeError, at, hermes_error};
use hiroute_domain::{AdditionalAgentModelV1, CanonicalDigest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

pub const HERMES_PLUGIN_NAME: &str = "hiroute";
const SELECTORS: &[&str] = &[
    "default",
    "provider",
    "context_length",
    "base_url",
    "api_mode",
    "transport",
    "api_key",
    "api_key_env",
    "key_env",
    "key_cmd",
    "model",
    "url",
    "api",
];

/// This record contains native credentials; it belongs only in the protected restore port.
#[derive(Deserialize, Serialize)]
pub struct HermesConfigRestoreV1 {
    schema: String,
    provider_id: String,
    original: Vec<u8>,
    managed_file_digest: CanonicalDigest,
    before: BTreeMap<String, Option<Value>>,
    after: BTreeMap<String, Option<Value>>,
    original_enabled: bool,
}
impl Drop for HermesConfigRestoreV1 {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.original.zeroize();
        self.before
            .values_mut()
            .chain(self.after.values_mut())
            .filter_map(Option::as_mut)
            .for_each(super::pi_sources::clear_native_value);
    }
}

pub fn hermes_configure(
    current: &[u8],
    previous: Option<&HermesConfigRestoreV1>,
    provider_id: &str,
    endpoint: &str,
    model: &AdditionalAgentModelV1,
    token: &str,
) -> Result<(Zeroizing<Vec<u8>>, HermesConfigRestoreV1), HermesNativeError> {
    model
        .validate_pi()
        .map_err(|_| hermes_error("Plan budget"))?;
    if !provider_id.starts_with("hiroute-main-") || token.is_empty() {
        return Err(hermes_error("owned provider"));
    }
    let doc = HermesDocument::parse(current)?;
    if doc
        .value
        .pointer("/plugins/disabled")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries
                .iter()
                .any(|v| v.as_str() == Some(HERMES_PLUGIN_NAME))
        })
    {
        return Err(hermes_error("native plugin explicitly disabled"));
    }
    let concrete_provider =
        hiroute_domain::additional_model_plan_provider_id(provider_id, &model.alias);
    if let Some(previous) = previous {
        if previous.provider_id != provider_id || !previous.applied(current)? {
            return Err(hermes_error("owned configuration changed"));
        }
    } else if doc
        .value
        .get("providers")
        .and_then(|p| p.get(&concrete_provider))
        .is_some()
        || doc.value.pointer("/plugins/entries/hiroute").is_some()
    {
        return Err(hermes_error("foreign provider or plugin entry"));
    }
    for key in ["model", "providers", "plugins"] {
        if doc.value.get(key).is_some_and(|v| !v.is_object()) {
            return Err(hermes_error("native mapping shape"));
        }
    }
    let mode = match model.protocol {
        hiroute_domain::AgentIngressProtocolV1::Responses => "codex_responses",
        hiroute_domain::AgentIngressProtocolV1::Messages => "anthropic_messages",
    };
    let base = if model.protocol == hiroute_domain::AgentIngressProtocolV1::Messages {
        endpoint
            .strip_suffix("/v1")
            .ok_or_else(|| hermes_error("Messages base"))?
    } else {
        endpoint
    };
    let mut changes = BTreeMap::new();
    for key in SELECTORS {
        changes.insert(vec!["model".into(), (*key).into()], None);
    }
    changes.insert(
        vec!["model".into(), "default".into()],
        Some(json!(model.alias)),
    );
    changes.insert(
        vec!["model".into(), "provider".into()],
        Some(json!(concrete_provider)),
    );
    changes.insert(
        vec!["model".into(), "context_length".into()],
        Some(json!(model.context_window_tokens)),
    );
    changes.insert(vec!["providers".into(),concrete_provider.clone()],Some(json!({
        "api":base,"transport":mode,"api_key":"hiroute-local-placeholder",
        "default_model":model.alias,"models":{&model.alias:{"context_length":model.context_window_tokens}},
        "extra_headers":{"X-HiRoute-Token":token}
    })));
    changes.insert(
        vec![
            "plugins".into(),
            "entries".into(),
            HERMES_PLUGIN_NAME.into(),
        ],
        Some(json!({
            "provider_id":concrete_provider,"model":model.alias,"base_url":base,"api_mode":mode
        })),
    );
    // A provider alias cannot distinguish another user entry selecting the same route.
    if let Some(providers) = doc.value.get("providers").and_then(Value::as_object) {
        for (id, p) in providers {
            if id == &concrete_provider
                || previous.is_some_and(|p| p.after.contains_key(&format!("providers/{id}")))
            {
                continue;
            }
            let address = p
                .get("api")
                .or_else(|| p.get("url"))
                .or_else(|| p.get("base_url"))
                .and_then(Value::as_str);
            if address == Some(base) {
                return Err(hermes_error("route-matching provider ambiguity"));
            }
        }
    }
    let mut enabled = enabled_plugins(&doc.value)?;
    let original_enabled = previous.map_or(enabled.iter().any(|v| v == HERMES_PLUGIN_NAME), |p| {
        p.original_enabled
    });
    if !enabled.iter().any(|v| v == HERMES_PLUGIN_NAME) {
        enabled.push(HERMES_PLUGIN_NAME.into());
    }
    changes.insert(
        vec!["plugins".into(), "enabled".into()],
        Some(json!(enabled)),
    );
    if let Some(previous) = previous {
        // Reconfiguration must remove the prior per-Plan provider only after owned selectors
        // move, and only if no foreign native reference still uses it.
        for key in previous
            .after
            .keys()
            .filter(|k| k.starts_with("providers/"))
        {
            let path: Vec<_> = key.split('/').map(str::to_owned).collect();
            if !changes.contains_key(&path) {
                changes.insert(path, None);
            }
        }
    }
    let bytes = doc.edit(&changes)?;
    // A later native consumer may reference the previous Plan provider. Never remove
    // that provider while an unowned selector still references it.
    if let Some(previous) = previous {
        let next = HermesDocument::parse(&bytes)?;
        for key in previous
            .after
            .keys()
            .filter_map(|k| k.strip_prefix("providers/"))
        {
            if key != concrete_provider && refers_to(&next.value, key) {
                return Err(hermes_error("remaining native provider reference"));
            }
        }
    }
    let original = previous.map_or_else(|| current.to_vec(), |p| p.original.clone());
    let base_doc = HermesDocument::parse(&original)?;
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for (path, value) in &changes {
        if path == &["plugins".to_owned(), "enabled".to_owned()] {
            continue;
        }
        before.insert(path.join("/"), at(&base_doc.value, path).cloned());
        after.insert(path.join("/"), value.clone());
    }
    let restore = HermesConfigRestoreV1 {
        schema: "hiroute.hermes-config-restore/v1".into(),
        provider_id: provider_id.into(),
        original,
        managed_file_digest: CanonicalDigest::of_bytes(&bytes),
        before,
        after,
        original_enabled,
    };
    Ok((bytes, restore))
}

fn enabled_plugins(root: &Value) -> Result<Vec<String>, HermesNativeError> {
    match root.pointer("/plugins/enabled") {
        None => Ok(Vec::new()),
        Some(Value::Array(a)) if a.len() <= 256 => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| hermes_error("enabled plugin list"))
            })
            .collect(),
        _ => Err(hermes_error("enabled plugin list")),
    }
}

impl HermesConfigRestoreV1 {
    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>, HermesNativeError> {
        self.validate()?;
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .map_err(|_| hermes_error("restore encode"))
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, HermesNativeError> {
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| hermes_error("restore decode"))?;
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), HermesNativeError> {
        if self.schema != "hiroute.hermes-config-restore/v1"
            || !self.provider_id.starts_with("hiroute-main-")
            || self.before.len() != self.after.len()
            || self.before.keys().ne(self.after.keys())
        {
            return Err(hermes_error("restore record"));
        }
        for key in self.after.keys() {
            if !(key.starts_with("model/") && SELECTORS.contains(&&key[6..]))
                && !key.starts_with(&format!("providers/{}-", self.provider_id))
                && key != "plugins/entries/hiroute"
            {
                return Err(hermes_error("restore ownership"));
            }
        }
        Ok(())
    }
    pub fn applied(&self, current: &[u8]) -> Result<bool, HermesNativeError> {
        self.validate()?;
        let doc = HermesDocument::parse(current)?;
        Ok(self.after.iter().all(|(key, value)| {
            at(
                &doc.value,
                &key.split('/').map(str::to_owned).collect::<Vec<_>>(),
            ) == value.as_ref()
        }) && enabled_plugins(&doc.value)?
            .iter()
            .any(|v| v == HERMES_PLUGIN_NAME))
    }
    pub fn restore(&self, current: &[u8]) -> Result<Zeroizing<Vec<u8>>, HermesNativeError> {
        if !self.applied(current)? {
            return Err(hermes_error("owned configuration changed"));
        }
        if CanonicalDigest::of_bytes(current) == self.managed_file_digest {
            return Ok(Zeroizing::new(self.original.clone()));
        }
        let doc = HermesDocument::parse(current)?;
        let mut changes: BTreeMap<Vec<String>, Option<Value>> = self
            .before
            .iter()
            .map(|(key, value)| (key.split('/').map(str::to_owned).collect(), value.clone()))
            .collect();
        let mut enabled = enabled_plugins(&doc.value)?;
        if !self.original_enabled {
            enabled.retain(|v| v != HERMES_PLUGIN_NAME);
        }
        let original = HermesDocument::parse(&self.original)?;
        changes.insert(
            vec!["plugins".into(), "enabled".into()],
            if enabled.is_empty() && original.value.pointer("/plugins/enabled").is_none() {
                None
            } else {
                Some(json!(enabled))
            },
        );
        let mut next = doc.edit(&changes)?;
        // Removing the final owned member must not leave a YAML null overriding a native
        // default. Only empty containers introduced by this adapter can be released.
        for path in [
            vec!["model".into()],
            vec!["providers".into()],
            vec!["plugins".into(), "entries".into()],
            vec!["plugins".into()],
        ] {
            let restored = HermesDocument::parse(&next)?;
            let value = at(&restored.value, &path);
            let before = at(&original.value, &path);
            if value.is_some_and(|v| v.is_null() || v.as_object().is_some_and(|m| m.is_empty()))
                && before.is_none_or(|v| v.as_object().is_some_and(|m| m.is_empty()))
            {
                next = restored.edit(&BTreeMap::from([(path, before.cloned())]))?;
            }
        }
        let next_doc = HermesDocument::parse(&next)?;
        for key in self
            .after
            .keys()
            .filter_map(|k| k.strip_prefix("providers/"))
        {
            if self.before[&format!("providers/{key}")].is_none() && refers_to(&next_doc.value, key)
            {
                return Err(hermes_error("remaining native provider reference"));
            }
        }
        Ok(next)
    }
}

fn refers_to(value: &Value, id: &str) -> bool {
    match value {
        Value::String(s) => s == id || s.starts_with(&format!("{id}/")),
        Value::Array(a) => a.iter().any(|v| refers_to(v, id)),
        Value::Object(o) => o.values().any(|v| refers_to(v, id)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conditional_restore_releases_only_introduced_empty_containers() {
        let model = AdditionalAgentModelV1 {
            alias: "hiroute_test".into(),
            protocol: hiroute_domain::AgentIngressProtocolV1::Responses,
            context_window_tokens: 64000,
            max_output_tokens: 8192,
        };
        for original in [
            b"unrelated: true\n".as_slice(),
            b"model: {}\nproviders: {}\nunrelated: true\n".as_slice(),
        ] {
            let (managed, record) = hermes_configure(
                original,
                None,
                "hiroute-main-test",
                "http://127.0.0.1:8317/v1",
                &model,
                "local-token",
            )
            .unwrap();
            let mut edited = managed.to_vec();
            edited.extend_from_slice(b"later: retained\n");
            let restored = HermesDocument::parse(&record.restore(&edited).unwrap()).unwrap();
            let mut expected = HermesDocument::parse(original).unwrap().value.clone();
            expected["later"] = json!("retained");
            assert_eq!(restored.value, expected);
        }
    }
    #[test]
    fn scoped_default_restore_keeps_foreign_edits_and_plugin_members() {
        let original=b"model:\n  default: glm-5.3\n  provider: glm\n  base_url: https://stale.example\nproviders:\n  glm:\n    api: https://coding.example\nplugins:\n  enabled: [other]\nauxiliary:\n  title: native # unchanged\n";
        let model = AdditionalAgentModelV1 {
            alias: "hiroute_test".into(),
            protocol: hiroute_domain::AgentIngressProtocolV1::Responses,
            context_window_tokens: 64000,
            max_output_tokens: 8192,
        };
        let (managed, record) = hermes_configure(
            original,
            None,
            "hiroute-main-test",
            "http://127.0.0.1:8317/v1",
            &model,
            "local-token",
        )
        .unwrap();
        assert!(record.applied(&managed).unwrap());
        assert_eq!(record.restore(&managed).unwrap().as_slice(), original);
        let mut edited = managed.to_vec();
        edited.extend_from_slice(b"user_note: preserve\n");
        let restored = record.restore(&edited).unwrap();
        let doc = HermesDocument::parse(&restored).unwrap();
        assert_eq!(doc.value["model"]["default"], "glm-5.3");
        assert_eq!(doc.value["user_note"], "preserve");
        assert_eq!(doc.value["plugins"]["enabled"], json!(["other"]));
        let changed = HermesDocument::parse(&managed)
            .unwrap()
            .edit(&BTreeMap::from([(
                vec!["model".into(), "default".into()],
                Some(json!("foreign")),
            )]))
            .unwrap();
        assert!(record.restore(&changed).is_err());
    }
}
