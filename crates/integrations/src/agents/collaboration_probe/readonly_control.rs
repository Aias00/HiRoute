//! An independent negative control for the real Windows Codex shell sandbox.
//! A correlated native shell must enter the fixed script, attempt a real write,
//! catch only UnauthorizedAccessException, and leave the initially absent file absent.
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ReceiptFacts {
    pub(super) attempt_matches: usize,
    pub(super) denial_matches: usize,
    pub(super) attempt_substring: bool,
    pub(super) denial_substring: bool,
    pub(super) script: ScriptFacts,
}

/// Observations from the nonce-bound command, never evidence of sandbox enforcement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LanguageMode {
    Unknown,
    FullLanguage,
    ConstrainedLanguage,
    RestrictedLanguage,
    NoLanguage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScriptPhase {
    Entered,
    ConsoleAttempt,
    FileWrite,
    ConsoleDenial,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ScriptFailure {
    pub(super) phase: ScriptPhase,
    pub(super) hresult: i32,
    pub(super) category: u8,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ScriptFacts {
    pub(super) entered_matches: usize,
    pub(super) mode_matches: usize,
    pub(super) mode: LanguageMode,
    pub(super) failure_matches: usize,
    pub(super) failure: Option<ScriptFailure>,
}

pub(super) struct ReadOnlyControl {
    path: PathBuf,
    command: String,
    attempted: String,
    denied: String,
    entered: String,
    mode_prefix: String,
    failure_prefix: String,
    verified: bool,
}

impl ReadOnlyControl {
    #[cfg(any(windows, test))]
    pub(super) fn new(home: &Path, nonce: &str) -> Result<Self, &'static str> {
        let path = home.join(format!("hiroute-readonly-control-{nonce}.txt"));
        require_absent(&path)?;
        let literal = path
            .to_str()
            .ok_or("read-only control path encoding")?
            .replace('\'', "''");
        let attempted = format!("hiroute-readonly-attempt-{nonce}");
        let denied = format!("hiroute-readonly-denied-{nonce}");
        let entered = format!("hiroute-readonly-entered-{nonce}");
        let mode_prefix = format!("hiroute-readonly-mode-{nonce}:");
        let failure_prefix = format!("hiroute-readonly-failure-{nonce}:");
        // One physical command line prevents a rejected tool's echoed command
        // from containing a receipt as a standalone output line. Policy rejection,
        // invalid syntax and unrelated I/O failures must stay red.
        let command = format!(
            "$ErrorActionPreference = 'Stop'; $hirouteProbePhase = 'entered'; try {{ Write-Output '{entered}'; switch ($ExecutionContext.SessionState.LanguageMode) {{ 'FullLanguage' {{ Write-Output '{mode_prefix}FullLanguage' }} 'ConstrainedLanguage' {{ Write-Output '{mode_prefix}ConstrainedLanguage' }} 'RestrictedLanguage' {{ Write-Output '{mode_prefix}RestrictedLanguage' }} 'NoLanguage' {{ Write-Output '{mode_prefix}NoLanguage' }} default {{ Write-Output '{mode_prefix}Unknown' }} }}; $hirouteProbePhase = 'console-attempt'; [Console]::WriteLine('{attempted}'); $hirouteProbePhase = 'file-write'; try {{ [System.IO.File]::WriteAllText('{literal}', 'hiroute-readonly-unexpected-write'); [Console]::WriteLine('hiroute-readonly-write-succeeded'); exit 1 }} catch [System.UnauthorizedAccessException] {{ $hirouteProbePhase = 'console-denial'; [Console]::WriteLine('{denied}') }} }} catch {{ Write-Output ('{failure_prefix}' + $hirouteProbePhase + ':' + [string][int]$_.Exception.HResult + ':' + [string][int]$_.CategoryInfo.Category); throw }}"
        );
        Ok(Self {
            path,
            command,
            attempted,
            denied,
            entered,
            mode_prefix,
            failure_prefix,
            verified: false,
        })
    }

    pub(super) fn command(&self) -> &str {
        &self.command
    }

    #[cfg(test)]
    fn verified(&self) -> bool {
        self.verified
    }

    pub(super) fn receipt_facts(&self, output: &str) -> ReceiptFacts {
        ReceiptFacts {
            attempt_matches: matching_lines(output, &self.attempted).count(),
            denial_matches: matching_lines(output, &self.denied).count(),
            attempt_substring: output.contains(&self.attempted),
            denial_substring: output.contains(&self.denied),
            script: self.script_facts(output),
        }
    }

    fn script_facts(&self, output: &str) -> ScriptFacts {
        let modes: Vec<_> = output
            .lines()
            .filter_map(|line| line.trim().strip_prefix(&self.mode_prefix))
            .collect();
        let mode = match modes.as_slice() {
            ["FullLanguage"] => LanguageMode::FullLanguage,
            ["ConstrainedLanguage"] => LanguageMode::ConstrainedLanguage,
            ["RestrictedLanguage"] => LanguageMode::RestrictedLanguage,
            ["NoLanguage"] => LanguageMode::NoLanguage,
            _ => LanguageMode::Unknown,
        };
        let failures: Vec<_> = output
            .lines()
            .filter_map(|line| line.trim().strip_prefix(&self.failure_prefix))
            .collect();
        ScriptFacts {
            entered_matches: matching_lines(output, &self.entered).count(),
            mode_matches: modes.len(),
            mode,
            failure_matches: failures.len(),
            failure: match failures.as_slice() {
                [failure] => script_failure(failure),
                _ => None,
            },
        }
    }

    pub(super) fn confirm(&mut self, output: &str) -> Result<(), &'static str> {
        let attempted = unique_line(output, &self.attempted);
        let denied = unique_line(output, &self.denied);
        let ordered = attempted
            .zip(denied)
            .is_some_and(|(attempt, denial)| attempt < denial);
        let absent = require_absent(&self.path).is_ok();
        eprintln!(
            "native collaboration readonly control: shell-attempted={} access-denied={} receipt-order={ordered} file-absent={absent}",
            attempted.is_some(),
            denied.is_some()
        );
        if !ordered || !absent {
            return Err("native read-only write rejection");
        }
        self.verified = true;
        Ok(())
    }

    pub(super) fn verify_after_stop(&self) -> Result<(), &'static str> {
        if !self.verified {
            return Err("native read-only write rejection");
        }
        require_absent(&self.path)
    }
}

fn script_failure(text: &str) -> Option<ScriptFailure> {
    let mut parts = text.split(':');
    let phase = match parts.next()? {
        "entered" => ScriptPhase::Entered,
        "console-attempt" => ScriptPhase::ConsoleAttempt,
        "file-write" => ScriptPhase::FileWrite,
        "console-denial" => ScriptPhase::ConsoleDenial,
        _ => return None,
    };
    let code = parts.next()?;
    let hresult: i32 = code.parse().ok()?;
    if hresult.to_string() != code {
        return None;
    }
    let code = parts.next()?;
    // Microsoft ErrorCategory has exactly the fixed numeric range 0..=31.
    let category: u8 = code.parse().ok()?;
    if category > 31 || category.to_string() != code || parts.next().is_some() {
        return None;
    }
    Some(ScriptFailure {
        phase,
        hresult,
        category,
    })
}

fn require_absent(path: &Path) -> Result<(), &'static str> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("read-only control file unexpectedly present or unreadable"),
    }
}

fn unique_line(output: &str, marker: &str) -> Option<usize> {
    let mut matching = matching_lines(output, marker);
    let first = matching.next()?;
    matching.next().is_none().then_some(first)
}

fn matching_lines<'a>(output: &'a str, marker: &'a str) -> impl Iterator<Item = usize> + 'a {
    output
        .lines()
        .enumerate()
        .filter_map(move |(index, line)| (line.trim() == marker).then_some(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_control_requires_actual_attempt_and_access_denial_not_just_absence() {
        let root = tempfile::tempdir().unwrap();
        for output in [
            "",
            "hiroute-readonly-attempt-nonce",
            "hiroute-readonly-denied-nonce",
        ] {
            let mut control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
            assert!(control.confirm(output).is_err());
            assert!(!control.verified());
            assert!(control.verify_after_stop().is_err());
        }
        let mut control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
        control.confirm("Final output:\nhiroute-readonly-attempt-nonce\r\nhiroute-readonly-denied-nonce\r\n").unwrap();
        control.verify_after_stop().unwrap();
    }

    #[test]
    fn readonly_control_rejects_echoed_policy_error_and_unrelated_receipts() {
        let root = tempfile::tempdir().unwrap();
        let mut control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
        let echoed = format!(
            "CreateProcess {{ message: Rejected(\"`{}` rejected: blocked by policy\") }}",
            control.command()
        );
        assert!(control.confirm(&echoed).is_err());
        assert!(
            control
                .confirm("hiroute-readonly-attempt-other\nhiroute-readonly-denied-other")
                .is_err()
        );
        assert!(!control.verified());
    }

    #[test]
    fn readonly_control_rejects_reversed_or_repeated_receipt_lines() {
        let root = tempfile::tempdir().unwrap();
        for output in [
            "hiroute-readonly-denied-nonce\nhiroute-readonly-attempt-nonce",
            "hiroute-readonly-attempt-nonce\nhiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce",
            "hiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce\nhiroute-readonly-denied-nonce",
        ] {
            let mut control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
            assert!(control.confirm(output).is_err());
            assert!(!control.verified());
        }
    }

    #[test]
    fn readonly_control_rejects_an_existing_file_and_any_write_before_or_after_receipt() {
        let root = tempfile::tempdir().unwrap();
        let mut control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
        std::fs::write(&control.path, b"unexpected write").unwrap();
        assert!(ReadOnlyControl::new(root.path(), "nonce").is_err());
        assert!(
            control
                .confirm("hiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce")
                .is_err()
        );
        assert!(!control.verified());
        std::fs::remove_file(&control.path).unwrap();
        control
            .confirm("hiroute-readonly-attempt-nonce\nhiroute-readonly-denied-nonce")
            .unwrap();
        std::fs::write(&control.path, b"late unexpected write").unwrap();
        assert!(control.verify_after_stop().is_err());
    }

    #[test]
    fn readonly_control_preserves_a_literal_path_and_only_catches_access_denial() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home 中文 % ! ' &");
        std::fs::create_dir(&home).unwrap();
        let control = ReadOnlyControl::new(&home, "nonce").unwrap();
        assert!(!control.command().contains('\n'));
        assert!(control.command().contains("home 中文 % ! '' &"));
        assert!(
            control
                .command()
                .contains("[System.IO.File]::WriteAllText(")
        );
        assert!(
            control
                .command()
                .contains("catch [System.UnauthorizedAccessException]")
        );
        assert!(control.command().ends_with("; throw }"));
    }

    #[test]
    fn readonly_control_script_facts_are_closed_nonce_bound_and_never_satisfy_denial() {
        let root = tempfile::tempdir().unwrap();
        let mut control = ReadOnlyControl::new(root.path(), "private-nonce").unwrap();
        assert!(
            control
                .command()
                .contains("Write-Output 'hiroute-readonly-entered-private-nonce'")
        );
        for (caption, mode) in [
            ("FullLanguage", LanguageMode::FullLanguage),
            ("ConstrainedLanguage", LanguageMode::ConstrainedLanguage),
            ("RestrictedLanguage", LanguageMode::RestrictedLanguage),
            ("NoLanguage", LanguageMode::NoLanguage),
        ] {
            for (caption_phase, phase) in [
                ("entered", ScriptPhase::Entered),
                ("console-attempt", ScriptPhase::ConsoleAttempt),
                ("file-write", ScriptPhase::FileWrite),
                ("console-denial", ScriptPhase::ConsoleDenial),
            ] {
                let output = format!(
                    "hiroute-readonly-entered-private-nonce\nhiroute-readonly-mode-private-nonce:{caption}\nhiroute-readonly-failure-private-nonce:{caption_phase}:-2146233087:7\nprivate-output-path-token"
                );
                let facts = control.receipt_facts(&output);
                assert_eq!(facts.script.entered_matches, 1);
                assert_eq!(facts.script.mode_matches, 1);
                assert_eq!(facts.script.mode, mode);
                assert_eq!(facts.script.failure_matches, 1);
                assert_eq!(
                    facts.script.failure,
                    Some(ScriptFailure {
                        phase,
                        hresult: -2146233087,
                        category: 7
                    })
                );
                assert_eq!(facts.attempt_matches, 0);
                assert_eq!(facts.denial_matches, 0);
                assert!(!format!("{facts:?}").contains("private-"));
                assert!(control.confirm(&output).is_err());
                assert!(!control.verified());
            }
        }
    }

    #[test]
    fn readonly_control_script_facts_reject_duplicate_unknown_and_malformed_failure_rows() {
        let root = tempfile::tempdir().unwrap();
        let control = ReadOnlyControl::new(root.path(), "nonce").unwrap();
        for failure in [
            "unknown:-1:7",
            "console-attempt:+1:7",
            "console-attempt:-01:7",
            "console-attempt:2147483648:7",
            "console-attempt:-1:32",
            "console-attempt:-1:-1",
            "console-attempt:-1:07",
            "console-attempt:-1:7:private-extra",
            "console-attempt:private-error:7",
            "console-attempt:-1:private-category",
        ] {
            let facts = control.receipt_facts(&format!("hiroute-readonly-failure-nonce:{failure}"));
            assert_eq!(facts.script.failure_matches, 1);
            assert_eq!(facts.script.failure, None);
            assert!(!format!("{facts:?}").contains("private-"));
        }
        let facts = control.receipt_facts("hiroute-readonly-mode-nonce:FullLanguage\nhiroute-readonly-mode-nonce:FullLanguage\nhiroute-readonly-failure-nonce:file-write:-1:18\nhiroute-readonly-failure-nonce:file-write:-1:18");
        assert_eq!(facts.script.mode_matches, 2);
        assert_eq!(facts.script.mode, LanguageMode::Unknown);
        assert_eq!(facts.script.failure_matches, 2);
        assert_eq!(facts.script.failure, None);
        for output in [
            "hiroute-readonly-entered-other\nhiroute-readonly-mode-other:FullLanguage\nhiroute-readonly-failure-other:file-write:-1:18",
            "rejected echoed 'hiroute-readonly-entered-nonce'; 'hiroute-readonly-mode-nonce:FullLanguage'; 'hiroute-readonly-failure-nonce:file-write:-1:18'",
        ] {
            let facts = control.receipt_facts(output);
            assert_eq!(facts.script.entered_matches, 0);
            assert_eq!(facts.script.mode_matches, 0);
            assert_eq!(facts.script.failure_matches, 0);
            assert_eq!(facts.script.mode, LanguageMode::Unknown);
        }
    }

    #[cfg(windows)]
    #[test]
    fn readonly_control_windows_parser_checks_actual_command_without_execution() {
        use std::{process::Command, time::Duration};
        let root = super::super::super::probe_files::temporary("hiroute-readonly-parser-").unwrap();
        let control = ReadOnlyControl::new(root.path(), "parse-only").unwrap();
        let program = hiroute_platform::windows::launcher::system_executable(Path::new(
            "WindowsPowerShell/v1.0/powershell.exe",
        ))
        .unwrap();
        let broken = control.command().strip_suffix('}').unwrap();
        for (text, expected) in [(control.command(), "accepted"), (broken, "rejected")] {
            let text = text.replace('\'', "''");
            let script = format!(
                "$ErrorActionPreference='Stop'; $tokens=$null; $errors=$null; $null=[System.Management.Automation.Language.Parser]::ParseInput('{text}',[ref]$tokens,[ref]$errors); if($errors.Count -eq 0){{Write-Output 'accepted'}}else{{Write-Output 'rejected'}}"
            );
            let mut command = Command::new(&program);
            command
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .env_clear()
                .current_dir(root.path());
            let bytes = super::super::super::windows_runtime::bounded(
                &command,
                false,
                Duration::from_secs(10),
                1024,
            )
            .unwrap();
            assert_eq!(std::str::from_utf8(&bytes).unwrap().trim(), expected);
            require_absent(&control.path).unwrap();
        }
        // The actual command was only a string input to Parser.ParseInput.
        assert!(!control.verified());
    }
}
