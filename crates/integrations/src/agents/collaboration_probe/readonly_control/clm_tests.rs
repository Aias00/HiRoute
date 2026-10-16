use super::*;

fn receipt(control: &ReadOnlyControl, mode: &str, typed: &str) -> String {
    format!(
        "{}\n{}{mode}\n{}\n{}{mode}\n{}\n{typed}\n{}\n",
        control.pipeline_entered,
        control.pipeline_mode_prefix,
        control.entered,
        control.mode_prefix,
        control.attempted,
        control.denied
    )
}

#[test]
fn readonly_clm_requires_typed_filesystem_denial_and_known_matching_modes() {
    let root = tempfile::tempdir().unwrap();
    let mut control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
    let valid = receipt(&control, "ConstrainedLanguage", &control.filesystem_denial);
    for invalid in [
        format!("{}\n{}\n", control.attempted, control.denied),
        receipt(&control, "ConstrainedLanguage", ""),
        valid.replace(":-2147024891:", ":-2146233087:"),
        valid.replace(":18:", ":7:"),
        valid.replace(
            "GetContentWriterUnauthorizedAccessError",
            "ConstrainedMethodInvocation",
        ),
        valid.replace("ConstrainedLanguage", "Unknown"),
        valid.replace("ConstrainedLanguage", "RestrictedLanguage"),
        valid.replace(
            &format!("{}ConstrainedLanguage", control.mode_prefix),
            &format!("{}FullLanguage", control.mode_prefix),
        ),
        valid.replace(&control.entered, "missing-entered"),
        valid.replace(
            &format!("{}ConstrainedLanguage\n", control.pipeline_mode_prefix),
            "",
        ),
    ] {
        assert!(control.confirm(&invalid).is_err());
        assert!(!control.verified());
    }
    control.confirm(&valid).unwrap();
    control.verify_after_stop().unwrap();
}

#[test]
fn readonly_clm_rejects_language_failure_duplicate_unknown_and_success_rows() {
    let root = tempfile::tempdir().unwrap();
    let mut control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
    let valid = receipt(&control, "ConstrainedLanguage", &control.filesystem_denial);
    for invalid in [
        format!(
            "{valid}{}file-write:-2146233087:7\n",
            control.pipeline_failure_prefix
        ),
        format!(
            "{valid}{}ConstrainedMethodInvocation\n",
            control.pipeline_error_prefix
        ),
        format!(
            "{valid}{}file-write:-2146233087:7\n",
            control.failure_prefix
        ),
        format!("{valid}{}\n", control.filesystem_denial),
        format!(
            "{valid}{}:unknown\n",
            control.filesystem_denial.split_once(':').unwrap().0
        ),
        format!("{valid}hiroute-readonly-write-succeeded\n"),
        format!(
            "{}\n{}\n{}\n",
            control.filesystem_denial, control.attempted, control.denied
        ),
        valid.replace(
            &control.filesystem_denial,
            &format!("echo '{}'", control.filesystem_denial),
        ),
        control.command().to_owned(),
    ] {
        assert!(control.confirm(&invalid).is_err());
        assert!(!control.verified());
        assert!(control.verify_after_stop().is_err());
    }
    let full = receipt(&control, "FullLanguage", "");
    control.confirm(&full).unwrap();
}

#[test]
fn readonly_script_preserves_full_language_and_uses_cmdlet_only_for_clm() {
    let root = tempfile::tempdir().unwrap();
    let control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
    let script = control.command();
    assert!(!script.contains('\n') && !script.contains("[Console]"));
    assert!(!script.contains("-Force") && !script.contains("SetTokenInformation"));
    assert!(!script.contains("LanguageMode =") && !script.contains("LanguageMode="));
    assert!(script.contains("if ($hirouteProbeMode -ceq 'FullLanguage')"));
    assert!(script.contains("[System.IO.File]::WriteAllText("));
    assert!(script.contains("catch [System.UnauthorizedAccessException]"));
    assert!(script.contains("Microsoft.PowerShell.Management\\Set-Content -LiteralPath '"));
    assert!(script.contains("-NoNewline -ErrorAction Stop"));
    assert!(script.contains("$hirouteException -is [System.UnauthorizedAccessException]"));
    assert!(script.contains("$hirouteException.HResult -eq -2147024891"));
    assert!(script.contains("[int]$_.CategoryInfo.Category -ne 18"));
    assert!(script.contains(
        "GetContentWriterUnauthorizedAccessError,Microsoft.PowerShell.Commands.SetContentCommand"
    ));
    assert!(
        script.find("hirouteProbePhase = 'file-write'").unwrap()
            < script.find(&control.filesystem_denial).unwrap()
    );
}
