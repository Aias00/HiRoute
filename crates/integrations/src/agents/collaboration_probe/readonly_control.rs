//! An independent negative control for the real Windows Codex shell sandbox.
//! A correlated native shell must enter the fixed script, attempt a real write,
//! catch only UnauthorizedAccessException, and leave the initially absent file absent.
use std::path::{Path, PathBuf};

pub(super) struct ReadOnlyControl {
    path: PathBuf,
    command: String,
    attempted: String,
    denied: String,
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
        // One physical command line prevents a rejected tool's echoed command
        // from containing a receipt as a standalone output line. Policy rejection,
        // invalid syntax and unrelated I/O failures must stay red.
        let command = format!(
            "$ErrorActionPreference = 'Stop'; [Console]::WriteLine('{attempted}'); try {{ [System.IO.File]::WriteAllText('{literal}', 'hiroute-readonly-unexpected-write'); [Console]::WriteLine('hiroute-readonly-write-succeeded'); exit 1 }} catch [System.UnauthorizedAccessException] {{ [Console]::WriteLine('{denied}') }}"
        );
        Ok(Self {
            path,
            command,
            attempted,
            denied,
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

fn require_absent(path: &Path) -> Result<(), &'static str> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("read-only control file unexpectedly present or unreadable"),
    }
}

fn unique_line(output: &str, marker: &str) -> Option<usize> {
    let mut matching = output
        .lines()
        .enumerate()
        .filter_map(|(index, line)| (line.trim() == marker).then_some(index));
    let first = matching.next()?;
    matching.next().is_none().then_some(first)
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
        assert!(!control.command().contains("catch {"));
    }
}
