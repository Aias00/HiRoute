//! Content-free facts for the exact failing native Bash receipt, never an alternate probe.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const PREFIX: &str = "HiRoute Qoder CLI result diagnostic=";
const BOUND: usize = 128 * 1024;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Facts {
    actual_bytes: usize,
    expected_bytes: usize,
    actual_json: bool,
    semantic_json_equal: bool,
    contains_expected: bool,
    expected_prefix: bool,
    marker_not_found: bool,
    marker_no_such_file: bool,
    marker_permission_denied: bool,
    marker_approval: bool,
    marker_denied: bool,
    marker_timed_out: bool,
    marker_truncat: bool,
}

impl Facts {
    pub(crate) fn observe(actual: &str, expected: &str) -> Option<Self> {
        if actual.len() > BOUND || expected.is_empty() || expected.len() > BOUND {
            return None;
        }
        let actual_json = serde_json::from_str::<Value>(actual).ok();
        let expected_json = serde_json::from_str::<Value>(expected).ok();
        let lower = actual.to_ascii_lowercase();
        let trimmed = actual.trim();
        Some(Self {
            actual_bytes: actual.len(),
            expected_bytes: expected.len(),
            actual_json: actual_json.is_some(),
            semantic_json_equal: actual_json.is_some() && actual_json == expected_json,
            contains_expected: actual.contains(expected.trim()),
            // A nonempty strict prefix indicates possible truncation, not proof of it.
            expected_prefix: !trimmed.is_empty()
                && trimmed.len() < expected.trim().len()
                && expected.trim().starts_with(trimmed),
            marker_not_found: lower.contains("not found"),
            marker_no_such_file: lower.contains("no such file"),
            marker_permission_denied: lower.contains("permission denied"),
            marker_approval: lower.contains("approval"),
            marker_denied: lower.contains("denied"),
            marker_timed_out: lower.contains("timed out"),
            marker_truncat: lower.contains("truncat"),
        })
    }

    pub(crate) fn emit(&self) {
        eprintln!(
            "{PREFIX}{}",
            serde_json::to_string(self).expect("fixed diagnostic facts")
        );
    }

    #[cfg(test)]
    pub(crate) fn parse(line: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(line).ok()?.strip_prefix(PREFIX)?;
        let value: Self = serde_json::from_str(text).ok()?;
        (value.actual_bytes <= BOUND
            && (1..=BOUND).contains(&value.expected_bytes)
            && (!value.semantic_json_equal || value.actual_json))
            .then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_wrapping_semantics_truncation_and_fixed_errors_without_content() {
        let expected = r#"{"ok":true}"#;
        let semantic = Facts::observe("{ \"ok\": true }", expected).unwrap();
        assert!(semantic.actual_json && semantic.semantic_json_equal);
        let wrapped = Facts::observe("header\n{\"ok\":true}\nfooter", expected).unwrap();
        assert!(!wrapped.actual_json && wrapped.contains_expected && !wrapped.expected_prefix);
        assert!(
            Facts::observe("{\"ok\":", expected)
                .unwrap()
                .expected_prefix
        );
        assert!(!Facts::observe("", expected).unwrap().expected_prefix);
        let error = Facts::observe("private/path Permission denied; approval timed out; TRUNCATED; No such file; not found", expected).unwrap();
        assert!(
            error.marker_denied
                && error.marker_permission_denied
                && error.marker_approval
                && error.marker_timed_out
                && error.marker_truncat
                && error.marker_no_such_file
                && error.marker_not_found
        );
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("private/path")
        );
    }

    #[test]
    fn parser_rejects_unknown_duplicate_missing_fields_types_bounds_and_trailing_text() {
        let facts = Facts::observe("no such file", r#"{"ok":true}"#).unwrap();
        let text = serde_json::to_string(&facts).unwrap();
        let line = format!("{PREFIX}{text}");
        assert_eq!(Facts::parse(line.as_bytes()), Some(facts));
        let value: Value = serde_json::from_str(&text).unwrap();
        for (key, replacement) in [
            ("actual_bytes", serde_json::json!(BOUND + 1)),
            ("actual_bytes", serde_json::json!(-1)),
            ("expected_bytes", serde_json::json!(0)),
            ("actual_json", serde_json::json!("false")),
            ("private/path", serde_json::json!(true)),
            ("semantic_json_equal", serde_json::json!(true)),
        ] {
            let mut invalid = value.clone();
            invalid[key] = replacement;
            assert!(Facts::parse(format!("{PREFIX}{invalid}").as_bytes()).is_none());
        }
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove("marker_denied");
        assert!(Facts::parse(format!("{PREFIX}{missing}").as_bytes()).is_none());
        assert!(Facts::parse(format!("{line} private/path").as_bytes()).is_none());
        assert!(
            Facts::parse(format!("{PREFIX}{{\"actual_bytes\":1,{}", &text[1..]).as_bytes())
                .is_none()
        );
    }
}
