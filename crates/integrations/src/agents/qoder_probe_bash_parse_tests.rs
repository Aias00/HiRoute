//! Unix-only parser controls, not shell selection or product admission.
//! Bash promises a nonzero syntax status. Observed Linux 5.0 uses 1 and macOS
//! 3.2 uses 2; both require the exact parser caption and echoed command.
use std::process::Output;

pub(super) const MINIMAL_INVALID: &str = "& true";

fn message(line: &str) -> Option<&str> {
    let line = line.strip_prefix("/bin/bash: -c: line ")?;
    let (number, message) = line.split_once(": ")?;
    if number.is_empty() || number.len() > 9 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(message)
}

fn syntax(command: &str, output: &Output) -> bool {
    if !output.stdout.is_empty() {
        return false;
    }
    let Ok(stderr) = std::str::from_utf8(&output.stderr) else {
        return false;
    };
    let Some(stderr) = stderr.strip_suffix('\n') else {
        return false;
    };
    let mut lines = stderr.split('\n');
    let (Some(caption), Some(echo), None) = (lines.next(), lines.next(), lines.next()) else {
        return false;
    };
    if !matches!(
        message(caption),
        Some("syntax error near unexpected token `&'" | "syntax error near unexpected token '&'")
    ) {
        return false;
    }
    message(echo).is_some_and(|echo| {
        echo.strip_prefix(['`', '\''])
            .and_then(|echo| echo.strip_suffix('\''))
            == Some(command)
    })
}

pub(super) fn correlated_rejection(command: &str, old: &Output, control: &Output) -> bool {
    matches!(old.status.code(), Some(1 | 2))
        && old.status.code() == control.status.code()
        && syntax(command, old)
        && syntax(MINIMAL_INVALID, control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn output(command: &str, code: i32) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: format!(
                "/bin/bash: -c: line 1: syntax error near unexpected token `&'\n/bin/bash: -c: line 1: `{command}'\n"
            )
            .into_bytes(),
        }
    }

    #[test]
    fn old_bash_rejection_requires_correlated_two_line_syntax_for_both_observed_exit_codes() {
        let command = "& 'fixed 'literal'' 中文'";
        for code in [1, 2] {
            assert!(correlated_rejection(
                command,
                &output(command, code),
                &output(MINIMAL_INVALID, code)
            ));
        }
        assert!(!correlated_rejection(
            command,
            &output(command, 1),
            &output(MINIMAL_INVALID, 2)
        ));
        assert!(!correlated_rejection(
            command,
            &output(command, 2),
            &output("foreign control", 2)
        ));
    }

    #[test]
    fn non_parser_exits_and_foreign_or_ambiguous_diagnostics_never_prove_old_bash_rejection() {
        let command = "& fixed";
        let control = output(MINIMAL_INVALID, 2);
        for code in [0, 3, 126, 127, 255] {
            assert!(!correlated_rejection(
                command,
                &output(command, code),
                &control
            ));
        }
        let mut killed = output(command, 2);
        killed.status = ExitStatus::from_raw(9);
        assert!(!correlated_rejection(command, &killed, &control));
        let expected = output(command, 2).stderr;
        for stderr in [
            b"Permission denied\n".to_vec(),
            b"syntax error near unexpected token `&'\n".to_vec(),
            [expected.as_slice(), b"extra\n"].concat(),
            [expected.as_slice(), expected.as_slice()].concat(),
            output("foreign input", 2).stderr,
            String::from_utf8(expected.clone())
                .unwrap()
                .replace("line 1:", "line -1:")
                .into_bytes(),
            String::from_utf8(expected.clone())
                .unwrap()
                .replace("/bin/bash:", "foreign:")
                .into_bytes(),
            expected[..expected.len() - 1].to_vec(),
            vec![0xff],
        ] {
            let mut rejected = output(command, 2);
            rejected.stderr = stderr;
            assert!(!correlated_rejection(command, &rejected, &control));
        }
        let mut unexpected = output(command, 2);
        unexpected.stdout = b"unexpected".to_vec();
        assert!(!correlated_rejection(command, &unexpected, &control));
    }
}
