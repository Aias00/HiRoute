//! One physical PowerShell command. Receipts never stand alone in an echoed
//! command, and language-policy rejection is distinct from filesystem denial.

pub(super) struct Frames<'a> {
    pub literal: &'a str,
    pub attempted: &'a str,
    pub denied: &'a str,
    pub entered: &'a str,
    pub mode: &'a str,
    pub failure: &'a str,
    pub pipeline_entered: &'a str,
    pub pipeline_mode: &'a str,
    pub pipeline_failure: &'a str,
    pub pipeline_error: &'a str,
    pub filesystem_denial: &'a str,
}

pub(super) fn command(f: Frames<'_>) -> String {
    let modes = [
        "FullLanguage",
        "ConstrainedLanguage",
        "RestrictedLanguage",
        "NoLanguage",
    ]
    .into_iter()
    .map(|mode| {
        format!(
            "'{mode}' {{ '{}{mode}'; '{}{mode}' }}",
            f.pipeline_mode, f.mode
        )
    })
    .collect::<Vec<_>>()
    .join(" ");
    // PowerShell v7.4.13 FileSystemProvider.GetContentWriter catches the actual
    // UnauthorizedAccessException as GetContentWriterUnauthorizedAccessError,
    // ErrorCategory.PermissionDenied (18); the reader/writer opens FileStream.
    // This does not accept CLM's MethodInvocationNotSupported error. Exception
    // inspection uses properties/operators, not prohibited .NET method calls.
    format!(
        "$ErrorActionPreference = 'Stop'; $hirouteProbePhase = 'entered'; try {{ '{}'; $hirouteProbeMode = $ExecutionContext.SessionState.LanguageMode; switch ($hirouteProbeMode) {{ {modes} default {{ '{}Unknown'; '{}Unknown' }} }}; '{}'; if ($hirouteProbeMode -cne 'FullLanguage' -and $hirouteProbeMode -cne 'ConstrainedLanguage') {{ throw 'hiroute-readonly-unsupported-language' }}; $hirouteProbePhase = 'console-attempt'; '{}'; $hirouteProbePhase = 'file-write'; if ($hirouteProbeMode -ceq 'FullLanguage') {{ try {{ [System.IO.File]::WriteAllText('{}', 'hiroute-readonly-unexpected-write'); 'hiroute-readonly-write-succeeded'; exit 1 }} catch [System.UnauthorizedAccessException] {{ $hirouteProbePhase = 'console-denial'; '{}' }} }} else {{ try {{ Microsoft.PowerShell.Management\\Set-Content -LiteralPath '{}' -Value 'hiroute-readonly-unexpected-write' -NoNewline -ErrorAction Stop; 'hiroute-readonly-write-succeeded'; exit 1 }} catch [System.UnauthorizedAccessException] {{ $hirouteTypedDenied = $false; $hirouteException = $_.Exception; for ($hirouteDepth = 0; $hirouteDepth -lt 8 -and $null -ne $hirouteException; $hirouteDepth++) {{ if ($hirouteException -is [System.UnauthorizedAccessException] -and $hirouteException.HResult -eq -2147024891) {{ $hirouteTypedDenied = $true }}; $hirouteException = $hirouteException.InnerException }}; if (-not $hirouteTypedDenied -or [int]$_.CategoryInfo.Category -ne 18 -or $_.FullyQualifiedErrorId -cne 'GetContentWriterUnauthorizedAccessError,Microsoft.PowerShell.Commands.SetContentCommand') {{ throw }}; '{}'; $hirouteProbePhase = 'console-denial'; '{}' }} }} }} catch {{ '{}' + $hirouteProbePhase + ':' + [string][int]$_.Exception.HResult + ':' + [string][int]$_.CategoryInfo.Category; if ($_.FullyQualifiedErrorId -ceq 'MethodInvocationNotSupportedInConstrainedLanguage') {{ '{}ConstrainedMethodInvocation' }} else {{ '{}Unknown' }}; '{}' + $hirouteProbePhase + ':' + [string][int]$_.Exception.HResult + ':' + [string][int]$_.CategoryInfo.Category; throw }}",
        f.pipeline_entered,
        f.pipeline_mode,
        f.mode,
        f.entered,
        f.attempted,
        f.literal,
        f.denied,
        f.literal,
        f.filesystem_denial,
        f.denied,
        f.pipeline_failure,
        f.pipeline_error,
        f.pipeline_error,
        f.failure
    )
}
