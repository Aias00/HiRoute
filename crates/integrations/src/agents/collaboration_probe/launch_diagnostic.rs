//! Closed diagnostics for the selected Codex's native shell launch failures.
//!
//! Captions below are from openai/codex a956835d: core/src/unified_exec/{errors,
//! process_manager}.rs and windows-sandbox-rs/src/{token,desktop,process,
//! spawn_prep,resolved_permissions}.rs. The manager flattens anyhow context and
//! also wraps ToolError as CreateProcess; that wrapper alone proves no OS API.
//! Never retain or render the vendor message, command, path or account.
use std::collections::BTreeSet;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum NativeLaunchStage {
    Unknown,
    UnifiedExecCreate,
    CreateProcessW,
    CreateProcessAsUserW,
    CreateProcessWithLogonW,
    Spawn,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum NativeLaunchCause {
    Unknown,
    WindowsBackendRequired,
    PermissionProfile,
    SandboxHomeResolution,
    SandboxCapabilities,
    TokenOpen,
    TokenGroups,
    TokenDefaultDacl,
    TokenRestriction,
    WellKnownSid,
    PrivilegeLookup,
    PrivilegeAdjustment,
    LogonSidMissing,
    DesktopOpen,
    DesktopCreate,
    DesktopDescriptor,
    DesktopAcl,
    DesktopSecurity,
    DesktopCache,
    StdioHandle,
    StdioInheritance,
    StdinPipe,
    StdoutPipe,
    StderrPipe,
    ProcessJob,
    ProcessCreate,
    ApprovalPreparation,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct NativeLaunchDiagnostic {
    pub(super) stage: NativeLaunchStage,
    pub(super) cause: NativeLaunchCause,
    pub(super) ambiguous_cause: bool,
    pub(super) tool_rejected_wrapper: bool,
    pub(super) codex_error_wrapper: bool,
    pub(super) os_error: Option<u32>,
    pub(super) ambiguous_os_error: bool,
}

// Only these upstream captions immediately precede a GetLastError/returned
// Win32 code. In particular, generic "failed: 123" or process exit codes do not.
const NUMERIC_CAPTIONS: &[(&str, NativeLaunchCause)] = &[
    ("OpenProcessToken failed: ", NativeLaunchCause::TokenOpen),
    (
        "GetTokenInformation(TokenGroups) failed: ",
        NativeLaunchCause::TokenGroups,
    ),
    (
        "SetTokenInformation(TokenDefaultDacl) failed: ",
        NativeLaunchCause::TokenDefaultDacl,
    ),
    (
        "CreateRestrictedToken failed: ",
        NativeLaunchCause::TokenRestriction,
    ),
    (
        "CreateWellKnownSid failed: ",
        NativeLaunchCause::WellKnownSid,
    ),
    (
        "LookupPrivilegeValueW failed: ",
        NativeLaunchCause::PrivilegeLookup,
    ),
    (
        "AdjustTokenPrivileges failed: ",
        NativeLaunchCause::PrivilegeAdjustment,
    ),
    (
        "AdjustTokenPrivileges error ",
        NativeLaunchCause::PrivilegeAdjustment,
    ),
    ("OpenDesktopW failed: ", NativeLaunchCause::DesktopOpen),
    ("CreateDesktopW failed: ", NativeLaunchCause::DesktopCreate),
    (
        "CreateDesktopW failed for shared private desktop: ",
        NativeLaunchCause::DesktopCreate,
    ),
    (
        "ConvertStringSecurityDescriptorToSecurityDescriptorW failed: ",
        NativeLaunchCause::DesktopDescriptor,
    ),
    (
        "SetEntriesInAclW failed for private desktop: ",
        NativeLaunchCause::DesktopAcl,
    ),
    (
        "SetSecurityInfo failed for private desktop: ",
        NativeLaunchCause::DesktopSecurity,
    ),
    ("GetStdHandle failed: ", NativeLaunchCause::StdioHandle),
    (
        "SetHandleInformation failed: ",
        NativeLaunchCause::StdioInheritance,
    ),
    (
        "SetHandleInformation failed for stdio handle: ",
        NativeLaunchCause::StdioInheritance,
    ),
    ("CreatePipe stdin failed: ", NativeLaunchCause::StdinPipe),
    ("CreatePipe stdout failed: ", NativeLaunchCause::StdoutPipe),
    ("CreatePipe stderr failed: ", NativeLaunchCause::StderrPipe),
    (
        "CreateProcessAsUserW failed: ",
        NativeLaunchCause::ProcessCreate,
    ),
    (
        "CreateProcessWithLogonW failed: ",
        NativeLaunchCause::ProcessCreate,
    ),
];

const CONTEXT_CAPTIONS: &[(&str, NativeLaunchCause)] = &[
    (
        "Restricted read-only access requires the elevated Windows sandbox backend",
        NativeLaunchCause::WindowsBackendRequired,
    ),
    (
        "deny-read overrides require the elevated Windows sandbox backend",
        NativeLaunchCause::WindowsBackendRequired,
    ),
    (
        "managed networking requires the elevated Windows sandbox backend",
        NativeLaunchCause::WindowsBackendRequired,
    ),
    (
        "network proxy restricting SID requires the elevated Windows sandbox backend",
        NativeLaunchCause::WindowsBackendRequired,
    ),
    (
        "permission profile requests full-disk filesystem writes, which cannot be enforced by the Windows sandbox",
        NativeLaunchCause::PermissionProfile,
    ),
    (
        "only managed permission profiles can be enforced by the Windows sandbox",
        NativeLaunchCause::PermissionProfile,
    ),
    (
        "only restricted managed filesystem permissions can be enforced by the Windows sandbox",
        NativeLaunchCause::PermissionProfile,
    ),
    (
        "elevated Windows sandbox requires effective `:root` read access",
        NativeLaunchCause::PermissionProfile,
    ),
    (
        "windows sandbox: failed to resolve codex_home",
        NativeLaunchCause::SandboxHomeResolution,
    ),
    (
        "workspace-write sandbox has no writable root capability SIDs",
        NativeLaunchCause::SandboxCapabilities,
    ),
    (
        "no capability SIDs provided",
        NativeLaunchCause::SandboxCapabilities,
    ),
    (
        "Logon SID not present on token",
        NativeLaunchCause::LogonSidMissing,
    ),
    (
        "shared private desktop cache was poisoned",
        NativeLaunchCause::DesktopCache,
    ),
    ("create process job", NativeLaunchCause::ProcessJob),
    (
        "could not prepare approval action: ",
        NativeLaunchCause::ApprovalPreparation,
    ),
];

fn decimal_prefix(tail: &str) -> Option<(u32, &str)> {
    let end = tail.bytes().take_while(u8::is_ascii_digit).count();
    (1..=10)
        .contains(&end)
        .then(|| {
            tail[..end]
                .parse::<u32>()
                .ok()
                .map(|code| (code, &tail[end..]))
        })
        .flatten()
}

fn caption_number(tail: &str) -> Option<u32> {
    let (code, rest) = decimal_prefix(tail)?;
    // Debug wrapping can quote/escape the caption several times. The boundary
    // remains punctuation, never an arbitrary digit prefix in a path or argv.
    (rest.is_empty()
        || rest.starts_with([' ', '\n', '\r', '"', '}'])
        || rest.trim_start_matches('\\').starts_with('"'))
    .then_some(code)
}

pub(super) fn native_launch_diagnostic(text: &str) -> NativeLaunchDiagnostic {
    let stage = if text.contains("CreateProcessAsUserW failed: ") {
        NativeLaunchStage::CreateProcessAsUserW
    } else if text.contains("CreateProcessWithLogonW failed: ") {
        NativeLaunchStage::CreateProcessWithLogonW
    } else if text.contains("CreateProcessW `") {
        NativeLaunchStage::CreateProcessW
    } else if text.contains("Failed to create unified exec process:")
        || text.contains("CreateProcess { message:")
    {
        NativeLaunchStage::UnifiedExecCreate
    } else if text.contains("Failed to spawn") {
        NativeLaunchStage::Spawn
    } else {
        NativeLaunchStage::Unknown
    };
    let mut codes = BTreeSet::new();
    let mut causes = BTreeSet::new();
    for (marker, suffix) in [("(os error ", ")"), ("Os { code: ", ",")] {
        for (offset, _) in text.match_indices(marker).take(32) {
            if let Some((code, rest)) = decimal_prefix(&text[offset + marker.len()..])
                && rest.starts_with(suffix)
            {
                codes.insert(code);
            }
        }
    }
    for &(caption, cause) in NUMERIC_CAPTIONS {
        for (offset, _) in text.match_indices(caption).take(32) {
            if let Some(code) = caption_number(&text[offset + caption.len()..]) {
                codes.insert(code);
                causes.insert(cause);
            }
        }
    }
    for &(caption, cause) in CONTEXT_CAPTIONS {
        if text.contains(caption) {
            causes.insert(cause);
        }
    }
    NativeLaunchDiagnostic {
        tool_rejected_wrapper: matches!(stage, NativeLaunchStage::UnifiedExecCreate)
            && text.contains("Rejected("),
        codex_error_wrapper: matches!(stage, NativeLaunchStage::UnifiedExecCreate)
            && text.contains("Codex("),
        stage,
        cause: if causes.len() == 1 {
            *causes.first().expect("one closed cause")
        } else {
            NativeLaunchCause::Unknown
        },
        ambiguous_cause: causes.len() > 1,
        os_error: (codes.len() == 1).then(|| *codes.first().expect("one numeric error")),
        ambiguous_os_error: codes.len() > 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_windows_api_captions_preserve_only_their_own_codes() {
        for (caption, cause) in NUMERIC_CAPTIONS {
            let text = format!("CreateProcess {{ message: \"Rejected(\\\"{caption}5\\\")\" }}");
            let diagnostic = native_launch_diagnostic(&text);
            assert_eq!(diagnostic.cause, *cause, "fixed caption {caption}");
            assert_eq!(diagnostic.os_error, Some(5));
            assert!(!diagnostic.ambiguous_cause);
            assert!(!diagnostic.ambiguous_os_error);
        }
    }

    #[test]
    fn sandbox_preparation_context_does_not_invent_a_win32_error() {
        for (caption, cause) in CONTEXT_CAPTIONS {
            let text = format!("Failed to create unified exec process: Rejected({caption})");
            let diagnostic = native_launch_diagnostic(&text);
            assert_eq!(diagnostic.cause, *cause, "fixed caption {caption}");
            assert_eq!(diagnostic.stage, NativeLaunchStage::UnifiedExecCreate);
            assert_eq!(diagnostic.os_error, None);
            assert!(diagnostic.tool_rejected_wrapper);
            assert!(!diagnostic.ambiguous_cause);
        }
    }

    #[test]
    fn wrapped_tool_error_is_not_assumed_to_be_an_os_create_failure() {
        for (wrapper, rejected, codex) in [("Rejected", true, false), ("Codex", false, true)] {
            let text =
                format!("CreateProcess {{ message: \"{wrapper}(private unclassified reason)\" }}");
            let diagnostic = native_launch_diagnostic(&text);
            assert_eq!(diagnostic.stage, NativeLaunchStage::UnifiedExecCreate);
            assert_eq!(diagnostic.cause, NativeLaunchCause::Unknown);
            assert_eq!(diagnostic.os_error, None);
            assert_eq!(diagnostic.tool_rejected_wrapper, rejected);
            assert_eq!(diagnostic.codex_error_wrapper, codex);
            assert!(!format!("{diagnostic:?}").contains("private unclassified reason"));
        }
    }

    #[test]
    fn distinct_fixed_causes_and_codes_remain_ambiguous_and_private() {
        const PRIVATE: &str = "private-account-path-command-never-emitted";
        let diagnostic = native_launch_diagnostic(&format!(
            "Failed to create unified exec process: {PRIVATE} OpenProcessToken failed: 5\nOpenDesktopW failed: 87"
        ));
        assert_eq!(diagnostic.cause, NativeLaunchCause::Unknown);
        assert!(diagnostic.ambiguous_cause);
        assert_eq!(diagnostic.os_error, None);
        assert!(diagnostic.ambiguous_os_error);
        assert!(!format!("{diagnostic:?}").contains(PRIVATE));
    }

    #[test]
    fn arbitrary_digits_and_unfixed_api_names_are_not_classified() {
        for text in [
            "private/552/5408 command 1114 failed: 5",
            "InventedWindowsAPI failed: 5",
            "OpenProcessToken failed: 5path",
            "OpenProcessToken failed: -5",
            "OpenProcessToken failed: 4294967296",
            "OpenProcessToken failed: secret",
        ] {
            let diagnostic = native_launch_diagnostic(text);
            assert_eq!(diagnostic.cause, NativeLaunchCause::Unknown);
            assert_eq!(diagnostic.os_error, None);
            assert!(!diagnostic.ambiguous_cause);
        }
    }

    #[test]
    fn repeated_fixed_caption_keeps_one_cause_and_code() {
        let diagnostic = native_launch_diagnostic(
            "Failed to create unified exec process: OpenDesktopW failed: 5\nOpenDesktopW failed: 5 (os error 5)",
        );
        assert_eq!(diagnostic.cause, NativeLaunchCause::DesktopOpen);
        assert_eq!(diagnostic.os_error, Some(5));
        assert!(!diagnostic.ambiguous_cause);
        assert!(!diagnostic.ambiguous_os_error);
    }
}
