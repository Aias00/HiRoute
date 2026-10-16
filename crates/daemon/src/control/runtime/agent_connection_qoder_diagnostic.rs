//! Closed, content-free diagnostics shared by the producer and native acceptance capture.
pub(crate) const PREFIX: &str = "HiRoute Qoder collaboration failure: stage=";

pub(crate) fn stage(value: &str) -> &'static str {
    match value {
        "Pi frozen budget" => "Pi frozen budget",
        "Skill front matter" => "Skill front matter",
        "Skill target" => "Skill target",
        "Skill target encoding" => "Skill target encoding",
        "actual native Skill path and contents" => "actual native Skill path and contents",
        "challenge entropy" => "challenge entropy",
        "duplicate auth" => "duplicate auth",
        "duplicate length" => "duplicate length",
        "evidence cache" => "evidence cache",
        "evidence digest" => "evidence digest",
        "evidence generation" => "evidence generation",
        "frozen Plan context budget" => "frozen Plan context budget",
        "frozen Plan output budget" => "frozen Plan output budget",
        "installed user Skill changed" => "installed user Skill changed",
        "installed user Skill encoding" => "installed user Skill encoding",
        "installed user Skill missing" => "installed user Skill missing",
        "installed user Skill read" => "installed user Skill read",
        "installed user Skill target" => "installed user Skill target",
        "loopback accept" => "loopback accept",
        "loopback address" => "loopback address",
        "loopback bind" => "loopback bind",
        "loopback mode" => "loopback mode",
        "loopback response" => "loopback response",
        "loopback route" => "loopback route",
        "model alias" => "model alias",
        "native Skill call result" => "native Skill call result",
        "native Skill discovery" => "native Skill discovery",
        "native check deadline" => "native check deadline",
        "native check superseded" => "native check superseded",
        "native collaboration execution" => "native collaboration execution",
        "native context changed" => "native context changed",
        "native context identity" => "native context identity",
        "native executable identity" => "native executable identity",
        "native input contract" => "native input contract",
        "native launch" => "native launch",
        "native login required" => "native login required",
        "native output bound" => "native output bound",
        "native process scope" => "native process scope",
        "native request limit" => "native request limit",
        "native settings bound" => "native settings bound",
        "native settings metadata" => "native settings metadata",
        "native settings receipt" => "native settings receipt",
        "native wait" => "native wait",
        "private directory" => "private directory",
        "private file" => "private file",
        "private output" => "private output",
        "private workspace" => "private workspace",
        "provider identity" => "provider identity",
        "request body read" => "request body read",
        "request bound" => "request bound",
        "request credential" => "request credential",
        "request deadline" => "request deadline",
        "request framing" => "request framing",
        "request header" => "request header",
        "request header read" => "request header read",
        "request json" => "request json",
        "request length" => "request length",
        "request model" => "request model",
        "request path" => "request path",
        "selected executable" => "selected executable",
        "selected native context" => "selected native context",
        "short request" => "short request",
        "socket mode" => "socket mode",
        "socket timeout" => "socket timeout",
        "transient route input" => "transient route input",
        "transient settings" => "transient settings",
        "trusted CLI" => "trusted CLI",
        "trusted CLI baseline" => "trusted CLI baseline",
        "trusted CLI bound" => "trusted CLI bound",
        "trusted CLI call result" => "trusted CLI call result",
        "trusted CLI contract" => "trusted CLI contract",
        "trusted CLI identity" => "trusted CLI identity",
        "trusted CLI output" => "trusted CLI output",
        "trusted CLI path" => "trusted CLI path",
        "trusted CLI process scope" => "trusted CLI process scope",
        "trusted CLI result mismatch" => "trusted CLI result mismatch",
        "trusted CLI wait" => "trusted CLI wait",
        _ => "unknown",
    }
}

#[cfg(test)]
pub(crate) fn line(bytes: &[u8]) -> Option<&'static str> {
    let value = std::str::from_utf8(bytes).ok()?.strip_prefix(PREFIX)?;
    let selected = stage(value);
    (selected == value).then_some(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_stage_cannot_reflect_content() {
        assert_eq!(stage("private/path token=secret"), "unknown");
        assert_eq!(stage("native launch"), "native launch");
    }
    #[test]
    fn only_complete_closed_lines_are_projected() {
        assert_eq!(
            line(b"HiRoute Qoder collaboration failure: stage=native launch"),
            Some("native launch")
        );
        assert_eq!(
            line(b"HiRoute Qoder collaboration failure: stage=unknown"),
            Some("unknown")
        );
        for bytes in [
            b"HiRoute Qoder collaboration failure: stage=private".as_slice(),
            b"HiRoute Qoder collaboration failure: stage=native launch token=secret",
            b"prefix HiRoute Qoder collaboration failure: stage=native launch",
            b"HiRoute Qoder collaboration failure: stage=native launch\nsecret",
        ] {
            assert!(line(bytes).is_none());
        }
    }
}
