# Read-only collaboration control

`readonly_control.rs` owns the nonce-bound actual denied-write receipt before
the Codex probe accepts installed Skill reads and public CLI output. The native
SDK sandbox, shell selection, login/profile and token policy remain their own
owners. Diagnostics are observations and never grant this capability.

`readonly_control/script.rs` is the single PowerShell command producer. It uses
language expressions for checkpoint output. FullLanguage retains the original
`File.WriteAllText` operation and typed `UnauthorizedAccessException` catch.
ConstrainedLanguage uses the actual FileSystem provider through module-qualified
`Set-Content -LiteralPath -NoNewline -ErrorAction Stop`, without `-Force`. Its
extra receipt requires the provider's exact UnauthorizedAccess error identity,
HRESULT `0x80070005` and `ErrorCategory.PermissionDenied`. The nonce-bound attempt,
typed receipt and denial must occur in that order, and the file must stay absent
after owned processes stop. Language-method rejection, unknown modes/errors,
echoes, duplicate frames or a successful write remain failed checks.

`readonly_control/windows_tests.rs` runs both the system Windows PowerShell and
required native pwsh under held Jobs. It proves actual writable-file creation,
actual ACL denial and a separate CLM method-policy rejection. Only fresh
explicitly user-owned fixture ACLs change, and their exact native descriptor is
restored. Output remains in memory; root exit and natural Job emptiness precede
inspection. These controls do not prove that an actual installed Codex completed
the whole Skill/CLI journey; that opt-in native Main case remains required.
