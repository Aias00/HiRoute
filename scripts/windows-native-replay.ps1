param(
  [Parameter(Mandatory)][ValidateSet('components','core','product','background','agents','accounts','upgrade')][string]$Scope,
  [Parameter(Mandatory)][string]$ExpectedSha,
  [Parameter(Mandatory)][string]$Bundle,
  [Parameter(Mandatory)][string]$Evidence,
  [string]$Plan = 'windows-headless',
  [ValidateSet('focused','final')][string]$Phase = 'focused',
  [string[]]$RelatedRun = @(),
  [string]$BaselineManifest,
  [string]$BaselineArchive,
  [string]$BaselineRevision,
  [ValidateSet('windows11-arm-x64','windows11-x64')][string]$ExecutionHost = 'windows11-arm-x64'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
. "$PSScriptRoot/windows-native-scopes.ps1"
. "$PSScriptRoot/windows-upgrade-inputs.ps1"
$bundle = (Resolve-Path $Bundle).Path
$evidence = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Evidence)
$os = Get-CimInstance Win32_OperatingSystem
$hostArchitecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$expectedArchitecture = if ($ExecutionHost -eq 'windows11-arm-x64') { 'Arm64' } else { 'X64' }
$runtimeExecution = if ($ExecutionHost -eq 'windows11-arm-x64') { 'x64_emulation' } else { 'native_x64' }
if (-not $IsWindows -or $os.ProductType -ne 1 -or [int]$os.BuildNumber -lt 22000 -or
    $hostArchitecture -cne $expectedArchitecture) {
  throw 'Replay requires an actual Windows 11 machine matching the explicit execution host'
}
New-Item -ItemType Directory -Force $evidence | Out-Null
$env:HIROUTE_BUILD_SOURCE_SHA = $ExpectedSha
$env:HIROUTE_WINDOWS_EXECUTION_HOST = $ExecutionHost
$env:HIROUTE_WINDOWS_SOURCE_ROOT = $root
$env:CARGO_TERM_COLOR = 'never'
python scripts/windows-native-artifacts.py verify --expected-sha $ExpectedSha --evidence $bundle > "$evidence/artifact-verification.json"
if ($LASTEXITCODE -ne 0) { throw 'Replay artifacts failed exact source and byte verification' }
python scripts/windows-native-runtime-audit.py --expected-sha $ExpectedSha --evidence $bundle > "$evidence/runtime-audit.json"
if ($LASTEXITCODE -ne 0) { throw 'Native import dependencies failed verification on the consumer' }
$manifest = Get-Content "$bundle/native-artifacts.json" -Raw | ConvertFrom-Json
$requiredRecipes = @(Get-WindowsRequiredRecipes $Scope)
$actualRecipes = @($manifest.recipes | ForEach-Object { [string]$_.name })
if ($actualRecipes.Count -ne $requiredRecipes.Count -or
    (Compare-Object ($actualRecipes | Sort-Object) ($requiredRecipes | Sort-Object))) {
  throw 'Complete scope recipe inventory required; partial producer artifacts cannot establish Windows 11 acceptance'
}
@{ source_revision = $ExpectedSha; validation_revision = $env:GITHUB_SHA; scope = $Scope;
   plan = $Plan; phase = $Phase; related_runs = $RelatedRun; caption = $os.Caption;
   version = $os.Version; host_architecture = $hostArchitecture; target_architecture = 'X64';
   execution = $runtimeExecution; arm64_native_evidence = $false;
   x64_native_evidence = ($ExecutionHost -eq 'windows11-x64');
   image_version = $env:ImageVersion; rustc = $manifest.build.rustc_verbose;
   cargo = 'Exact compiler-artifact replay; Cargo not run on consumer';
   producer_revision = $manifest.build.source_sha
} | ConvertTo-Json -Depth 5 | Out-File -Encoding utf8 "$evidence/provenance.json"

if ($manifest.product_binaries.hiroute -and $manifest.product_binaries.hirouted) {
  $cli = Join-Path $bundle $manifest.product_binaries.hiroute
  $daemon = Join-Path $bundle $manifest.product_binaries.hirouted
  if ((Split-Path $cli -Parent) -ne (Split-Path $daemon -Parent)) { throw 'Product binaries must be sibling candidate files' }
  $env:HIROUTE_VALIDATION_PRODUCT_BIN_DIR = Split-Path $cli -Parent
}
if ($manifest.product_binaries.cpa_process_fixture) {
  $env:HIROUTE_WINDOWS_CPA_FIXTURE = Join-Path $bundle $manifest.product_binaries.cpa_process_fixture
}
if ($Scope -in @('background','upgrade')) {
  if (-not $manifest.package) { throw 'Complete verified production package required' }
  $packageManifest = Join-Path $bundle $manifest.package.manifest
  $packageArchive = Join-Path $bundle $manifest.package.archive
  python scripts/windows-ci-package.py --restage-only --expected-sha $ExpectedSha --manifest $packageManifest --archive $packageArchive --output "$evidence/package" > "$evidence/package-result.json"
  if ($LASTEXITCODE -ne 0) { throw 'Fresh administrator package trust could not be established' }
  $package = Get-Content "$evidence/package-result.json" -Raw | ConvertFrom-Json
  $env:HIROUTE_WINDOWS_PACKAGE_MANIFEST = $package.manifest
  $env:HIROUTE_WINDOWS_PACKAGE_ARCHIVE = $package.archive
  $env:HIROUTE_WINDOWS_ADMIN_CLI = $package.administrator_cli
  $env:HIROUTE_WINDOWS_PYTHON = $package.python
  $env:HIROUTE_WORKER_DAEMON_BINARY = $package.worker_daemon
  if ($Scope -eq 'upgrade') {
    Initialize-WindowsUpgradeBaseline -ExpectedSha $ExpectedSha -BaselineRevision $BaselineRevision -BaselineManifest $BaselineManifest -BaselineArchive $BaselineArchive -Evidence $evidence
  }
}
if ($Scope -in @('agents','accounts','upgrade')) {
  if (-not $daemon) { throw 'Actual compiled Worker daemon required' }
  python scripts/windows-agent-fixtures.py --daemon $daemon --output "$evidence/clients" > "$evidence/clients-result.json"
  if ($LASTEXITCODE -ne 0) { throw 'Locked actual client fixtures unavailable on Windows 11' }
  $clients = Get-Content "$evidence/clients-result.json" -Raw | ConvertFrom-Json
  foreach ($property in $clients.environment.PSObject.Properties) {
    if ($property.Name -notmatch '^HIROUTE_WORKER_[A-Z_]+$') { throw 'Unknown client environment key' }
    [Environment]::SetEnvironmentVariable($property.Name, [string]$property.Value, 'Process')
  }
  $env:PATH = (@($clients.discovery_path) + @($env:PATH)) -join [IO.Path]::PathSeparator
  $env:HIROUTE_NATIVE_CODEX = $env:HIROUTE_WORKER_CODEX_BINARY
  if ($Scope -eq 'upgrade') {
    $env:HIROUTE_WINDOWS_UPGRADE_CODEX_BINARY = $clients.environment.HIROUTE_WORKER_CODEX_BINARY
    $env:HIROUTE_WINDOWS_UPGRADE_CODEX_SHA256 = $clients.binaries.HIROUTE_WORKER_CODEX_BINARY.sha256
    $env:HIROUTE_WINDOWS_UPGRADE_CODEX_VERSION = $clients.declarations.'@openai/codex'
  }
}

$failures = [System.Collections.Generic.List[string]]::new()
try {
  if ($manifest.recipes.Count -eq 0) { throw 'Required replay inventory is empty' }
  foreach ($recipe in $manifest.recipes) {
    $executable = Join-Path $bundle $recipe.executable
    $arguments = @()
    if ($recipe.filter) { $arguments += [string]$recipe.filter }
    $harness = @($recipe.harness_arguments | ForEach-Object { [string]$_ })
    $required = @($recipe.required_cases | ForEach-Object { [string]$_ })
    $name = [string]$recipe.name
    $result = @{ scenario = 'red'; name = $name; cases = $required; executed = 0;
                 source_revision = $ExpectedSha; ignored = 0; process_exit = $null;
                 plan = $Plan; phase = $Phase; related_runs = $RelatedRun;
                 command = @($executable) + $arguments + @('--test-threads=1') + $harness;
                 producer_target = $manifest.build.target; execution = $runtimeExecution }
    $started = Get-Date
    try {
      & $executable @arguments --list @harness 2>&1 | Tee-Object "$evidence/$name-inventory.log"
      if ($LASTEXITCODE -ne 0) { throw "$name native executable inventory failed" }
      $inventory = Get-Content "$evidence/$name-inventory.log" -Raw
      foreach ($case in $required) {
        $escaped = [regex]::Escape($case)
        if ($inventory -notmatch "(?m)^${escaped}: test`r?$") { throw "Required replay case missing: $case" }
      }
      $result.command_started_at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0
      & $executable @arguments --test-threads=1 @harness 2>&1 | Tee-Object "$evidence/$name.log"
      $result.process_exit = $LASTEXITCODE
      $result.command_finished_at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0
      $log = Get-Content "$evidence/$name.log" -Raw
      $summaries = [regex]::Matches($log, '(?m)^test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored.*$')
      if ($summaries.Count -gt 0) {
        $summary = $summaries[$summaries.Count - 1]
        $result.tests = @{ passed = [int]$summary.Groups[1].Value; failed = [int]$summary.Groups[2].Value; ignored = [int]$summary.Groups[3].Value }
        $result.executed = $result.tests.passed + $result.tests.failed
        $result.ignored = $result.tests.ignored
      }
      if ($result.process_exit -ne 0 -or $log -notmatch "test result: ok\. $($required.Count) passed; 0 failed; 0 ignored") {
        throw "$name required Windows 11 runtime cases failed or differed"
      }
      $result.scenario = 'green'
    } catch { $failures.Add($_.Exception.Message); Write-Warning $_.Exception.Message }
    finally {
      $result.seconds = ((Get-Date) - $started).TotalSeconds
      $result | ConvertTo-Json -Depth 5 | Out-File -Encoding utf8 "$evidence/$name-result.json"
    }
  }
} finally {
  python scripts/windows-ci-report.py $evidence
  if ($LASTEXITCODE -ne 0) { throw 'Windows 11 validation feedback could not be generated' }
}
if ($failures.Count -gt 0) { throw ($failures -join '; ') }
