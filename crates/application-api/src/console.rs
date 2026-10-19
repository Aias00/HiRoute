//! Owner credential operations. Credential exchanges never traverse Local Control.
use serde::{Deserialize, Serialize};
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleLaunchRequestV1 {
    #[serde(default)]
    pub loopback_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing: Option<String>,
}

/// A route hint only. It never carries a credential, URL, or executable command.
pub fn valid_console_landing(value: &str) -> bool {
    value == "first-run"
        || value
            .strip_prefix("agent:")
            .is_some_and(|agent| matches!(agent, "codex" | "claude-code"))
        || value.strip_prefix("suggestion:").is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_landing_is_additive_and_bounded() {
        let old: ConsoleLaunchRequestV1 = serde_json::from_str("{}").unwrap();
        assert!(old.landing.is_none());
        assert_eq!(
            serde_json::to_value(old).unwrap(),
            serde_json::json!({"loopback_only":false})
        );
        for value in [
            "first-run",
            "agent:codex",
            "agent:claude-code",
            "suggestion:abc-123",
        ] {
            assert!(valid_console_landing(value));
        }
        for value in [
            "",
            "https://example.invalid",
            "agent:other",
            "suggestion:",
            "suggestion:../x",
            "suggestion:x&code=x",
        ] {
            assert!(!valid_console_landing(value));
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleListTokensRequestV1 {}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleCreateTokenRequestV1 {
    pub name: String,
    #[serde(default)]
    pub expires_days: Option<u32>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleRevokeTokenRequestV1 {
    pub id: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleLaunchV1 {
    pub url: String,
    pub expires_in: u32,
}
