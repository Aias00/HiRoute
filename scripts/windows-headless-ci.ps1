param(
  [ValidateSet('components','product','background','core','agents','accounts','upgrade')][string]$Scope,
  [Parameter(Mandatory)][string]$ExpectedSha,
  [Parameter(Mandatory)][string]$Evidence,
  [string]$Plan = 'windows-headless',
  [ValidateSet('focused','final')][string]$Phase = 'focused',
  [string[]]$RelatedRun = @(),
  [string]$CpaEvidence,
  [string]$CpaSource,
  [string]$CpaOriginalRun,
  [string]$CpaOriginalRevision,
  [string]$BaselineManifest,
  [string]$BaselineArchive,
  [switch]$CaptureArtifacts
)
$ErrorActionPreference = 'Stop'
$evidence = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Evidence)
$failures = [System.Collections.Generic.List[string]]::new()
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
. "$PSScriptRoot/windows-native-scopes.ps1"
if (-not $IsWindows) { throw 'Native Windows required' }
if ($ExpectedSha -notmatch '^[0-9a-f]{40}$' -or (git rev-parse HEAD) -ne $ExpectedSha) {
  throw 'Exact committed candidate required'
}
git diff --exit-code HEAD --
if ($LASTEXITCODE -ne 0) { throw 'Candidate has tracked changes' }
if ($env:CARGO_TARGET_DIR -or $env:CARGO_BUILD_TARGET_DIR) { throw 'Checkout-local Cargo target required' }
New-Item -ItemType Directory -Force $evidence | Out-Null
$env:HIROUTE_BUILD_SOURCE_SHA = $ExpectedSha
$env:CARGO_TERM_COLOR = 'never'
$env:CARGO_INCREMENTAL = '0'
$os = Get-CimInstance Win32_OperatingSystem
@{ source_revision = $ExpectedSha; validation_revision = $env:GITHUB_SHA; scope = $Scope;
   plan = $Plan; phase = $Phase; related_runs = $RelatedRun; caption = $os.Caption;
   version = $os.Version; cpu_count = [Environment]::ProcessorCount;
   host_architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString();
   image_version = $env:ImageVersion; rustc = (rustc --version); cargo = (cargo --version)
} | ConvertTo-Json -Depth 5 | Out-File -Encoding utf8 "$evidence/provenance.json"

function Invoke-RequiredTests {
  param([string]$Name, [string[]]$CargoArguments, [string[]]$Required, [string[]]$HarnessArguments = @())
  $result = @{ scenario = 'red'; name = $Name; cases = $Required; executed = 0;
               source_revision = $ExpectedSha; ignored = 0; process_exit = $null;
               plan = $Plan; phase = $Phase; related_runs = $RelatedRun;
               command = @('cargo','test','--locked') + $CargoArguments + @('--','--test-threads=1') + $HarnessArguments }
  $started = Get-Date
  try {
    cargo test --locked @CargoArguments -- --list 2>&1 | Tee-Object "$evidence/$Name-inventory.log"
    if ($LASTEXITCODE -ne 0) { throw "$Name inventory failed to build" }
    $inventory = Get-Content "$evidence/$Name-inventory.log" -Raw
    foreach ($case in $Required) {
      $escaped = [regex]::Escape($case)
      if ($inventory -notmatch "(?m)^${escaped}: test`r?$") { throw "Required case not selected: $case" }
    }
    if ($CaptureArtifacts) {
      # Cargo's progress belongs to stderr even with --message-format=json.
      # Keep the compiler-artifact stream parseable and retain diagnostics separately.
      cargo test --locked @CargoArguments --no-run --message-format=json > "$evidence/$Name-cargo-artifacts.log" 2> "$evidence/$Name-cargo-artifacts-stderr.log"
      if ($LASTEXITCODE -ne 0) {
        Get-Content "$evidence/$Name-cargo-artifacts-stderr.log" | Write-Host
        throw "$Name executable provenance failed to build"
      }
      $filter = $null
      for ($i = 0; $i -lt $CargoArguments.Count; $i++) {
        if ($CargoArguments[$i] -in @('-p','--package','--test','--bin','--features')) { $i++; continue }
        if (-not $CargoArguments[$i].StartsWith('-')) {
          if ($filter) { throw 'Multiple native test filters are not supported' }
          $filter = $CargoArguments[$i]
        }
      }
      @{ required_cases = $Required; cargo_arguments = $CargoArguments; harness_arguments = $HarnessArguments; filter = $filter } |
        ConvertTo-Json -Depth 5 | Out-File -Encoding utf8 "$evidence/$Name-recipe.json"
      python scripts/windows-native-artifacts.py capture --expected-sha $ExpectedSha --evidence $evidence --name $Name --cargo-json "$evidence/$Name-cargo-artifacts.log" --recipe-json "$evidence/$Name-recipe.json"
      if ($LASTEXITCODE -ne 0) { throw "$Name executable provenance failed" }
    }
    $result.command_started_at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0
    cargo test --locked @CargoArguments -- --test-threads=1 @HarnessArguments 2>&1 | Tee-Object "$evidence/$Name.log"
    $result.process_exit = $LASTEXITCODE
    $result.command_finished_at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000.0
    $log = Get-Content "$evidence/$Name.log" -Raw
    foreach ($case in $Required) {
      $escaped = [regex]::Escape($case)
      if ($log -match "(?m)^test ${escaped} \.\.\. (ok|FAILED)`r?$") { $result.executed++ }
    }
    $summaries = [regex]::Matches($log, '(?m)^test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored.*$')
    if ($summaries.Count -gt 0) {
      # --nocapture can interleave per-case output; use the terminal parent summary.
      $summary = $summaries[$summaries.Count - 1]
      $result.tests = @{ passed = [int]$summary.Groups[1].Value; failed = [int]$summary.Groups[2].Value; ignored = [int]$summary.Groups[3].Value }
      $result.executed = $result.tests.passed + $result.tests.failed
      $result.ignored = $result.tests.ignored
    }
    if ($result.process_exit -ne 0) { throw "$Name native acceptance failed" }
    if ($log -notmatch "test result: ok\. $($Required.Count) passed; 0 failed; 0 ignored") {
      throw "$Name required runtime count differs"
    }
    $result.executed = $Required.Count
    $result.scenario = 'green'
  } catch {
    $failures.Add($_.Exception.Message)
    Write-Warning $_.Exception.Message
  } finally {
    $result.seconds = ((Get-Date) - $started).TotalSeconds
    $result | ConvertTo-Json -Depth 5 | Out-File -Encoding utf8 "$evidence/$Name-result.json"
  }
}

function Invoke-LibraryTests {
  param([string]$Package)
  $name = $Package -replace '^hiroute-','core-'
  try {
    cargo test --locked -p $Package --lib -- --list 2>&1 | Tee-Object "$evidence/$name-discovery.log"
    if ($LASTEXITCODE -ne 0) { throw "$Package native library inventory failed" }
    $inventory = Get-Content "$evidence/$name-discovery.log" -Raw
    $optIn = @()
    if ($Package -eq 'hiroute-integrations') {
      $optIn = @(
        'agents::native_ingress_probe::ephemeral::tests::real_codex_ephemeral_authentication_without_managed_configuration',
        'agents::qoder_probe::tests::real_qoder_discovers_skill_and_runs_read_only_cli_in_selected_context',
        'agents::windows_runtime::tests::windows_native_environment_fixture',
        'agents::windows_runtime::tests::windows_selected_node_version_uses_the_private_capture_and_held_job',
        'agents::windows_runtime::tests::windows_selected_pi_sdk_capabilities_load_the_actual_package',
        'agents::windows_collaboration_tests::actual_windows_codex_native_skill_executes_the_public_cli',
        'agents::windows_collaboration_tests::actual_windows_claude_native_skill_executes_the_public_cli',
        'agents::windows_collaboration_tests::actual_windows_qoder_native_skill_executes_the_public_cli_without_synthesized_account')
    }
    if ($Package -eq 'hiroute-cpa-bridge') {
      $optIn = @(
        'accounts::stock::pin_tests::live_managed_cpa_pin_reads',
        'runtime::tests::borrowed_stock::stock_cpa_v7_2_140_accepts_managed_auth_ready_and_shutdown_contract',
        'runtime::tests::borrowed_stock::stock_cpa_v7_2_140_real_borrowed_codex_responses_smoke')
      foreach ($case in @(
        'windows_private::tests::private_publication_and_owner_claim_use_actual_native_owner_and_acl',
        'windows_private::tests::private_io_rejects_foreign_acl_hardlink_and_junction_without_adoption',
        'windows_private::tests::generated_auth_files_keep_user_owner_and_inherited_private_acl',
        'windows_private::tests::damaged_private_record_is_preserved_and_never_rebuilt')) {
        if ($inventory -notmatch "(?m)^$([regex]::Escape($case)): test`r?$") { throw "Required CPA private case missing: $case" }
      }
    }
    $required = @([regex]::Matches($inventory, '(?m)^(\S+): test\r?$') | ForEach-Object { $_.Groups[1].Value } | Where-Object { $_ -notin $optIn })
    if ($required.Count -eq 0) { throw "$Package selected no native library tests" }
    $harness = @($optIn | ForEach-Object { '--skip'; $_ })
    @{ source_revision = $ExpectedSha; package = $Package; explicit_opt_in_or_child_fixtures = $optIn;
       reason = 'Real selected installation inputs and helper children have separate explicit native gates; any unregistered ignored case still fails this inventory.'
    } | ConvertTo-Json -Depth 5 | Out-File -Encoding utf8 "$evidence/$name-selection.json"
    Invoke-RequiredTests $name @('-p',$Package,'--lib') $required $harness
  } catch { $failures.Add($_.Exception.Message); Write-Warning $_.Exception.Message }
}

function Initialize-WindowsAgentInputs {
  cargo build --locked -p hiroute-daemon --bin hirouted -p hiroute-cli --bin hiroute 2>&1 | Tee-Object "$evidence/agent-daemon-link.log"
  if ($LASTEXITCODE -ne 0) { throw 'Production Worker daemon failed to build' }
  python scripts/windows-agent-fixtures.py --daemon target/debug/hirouted.exe --output "$evidence/clients" > "$evidence/clients-result.json"
  if ($LASTEXITCODE -ne 0) { throw 'Fixed upstream client prerequisites failed; Agent scenarios not executed' }
  $clients = Get-Content "$evidence/clients-result.json" -Raw | ConvertFrom-Json
  foreach ($property in $clients.environment.PSObject.Properties) {
    if ($property.Name -notmatch '^HIROUTE_WORKER_[A-Z_]+$') { throw 'Unknown client environment key' }
    [Environment]::SetEnvironmentVariable($property.Name, [string]$property.Value, 'Process')
  }
  $env:PATH = (@($clients.discovery_path) + @($env:PATH)) -join [IO.Path]::PathSeparator
  $env:HIROUTE_NATIVE_CODEX = $env:HIROUTE_WORKER_CODEX_BINARY
}

try {
if ($Scope -eq 'components') {
  Invoke-RequiredTests 'platform' @('-p','hiroute-platform','--test','windows') @(
    'private_file_identity_is_held_and_reparse_free',
    'ancestor_junction_cannot_redirect_private_read_or_creation',
    'pipe_exclusivity_bidirectional_frames_and_peer_identity',
    'pipe_deadline_cancels_read_without_consuming_the_next_frame',
    'native_permissions_survive_atomic_restore_and_detect_acl_changes',
    'sealed_readonly_artifact_rename_preserves_bytes_identity_and_native_permissions',
    'readonly_atomic_restore_preserves_permissions_and_requires_target_write_attributes')
  Invoke-RequiredTests 'platform-unit' @('-p','hiroute-platform','--lib') @(
    'durable::tests::initial_private_publication_never_replaces_a_committed_key',
    'local_record::tests::rejects_wrong_channel_direction_truncation_end_and_oversize',
    'windows::background::tests::native_argv_quotes_spaces_quotes_and_trailing_slashes_without_shell',
    'windows::background::tests::profile_is_absolute_and_definition_rejects_unknown_or_foreign_owner',
    'windows::lifecycle::tests::lifecycle_payload_is_closed_and_does_not_accept_extra_authority',
    'windows::service::token::tests::restricted_business_child_runs_with_owner_medium_token',
    'windows::task::tests::registration_preserves_demand_start_and_logon_policy_across_reopen') @('--nocapture')
  Invoke-RequiredTests 'jobs' @('-p','hiroute-platform','--test','job_windows') @(
    'job_stops_surviving_tool_after_leader_exit_and_preserves_neighbor',
    'native_job_argv_preserves_spaces_quotes_unicode_and_shell_characters',
    'managed_helper_executes_from_cmd_with_literal_special_character_path_and_argv',
    'duplicate_windows_environment_keys_are_rejected_before_launch',
    'inherited_credential_pipe_roundtrips_and_preserves_original_handle',
    'inherited_credential_rejects_invalid_stdio_file_and_noninherited_handles',
    'inherited_credential_pipe_timeout_and_byte_limit_are_bounded',
    'inherited_credential_crosses_an_explicit_job_child_handle_list') @('--skip','job_probe')
  Invoke-RequiredTests 'diagnostics' @('-p','hiroute-diagnostics','--test','windows_files_safety') @(
    'held_directory_read_write_enumerate_rename_and_remove_ignore_a_path_replacement',
    'existing_accessible_native_acl_is_preserved_and_new_objects_are_private',
    'junction_ancestor_and_child_cannot_redirect_diagnostic_operations',
    'hardlinks_and_replaced_file_identities_are_refused_before_mutation',
    'held_writer_can_atomically_replace_and_delete_without_share_mode_self_lock',
    'lock_contention_settings_commit_and_real_runtime_writer_are_supported')
  Invoke-RequiredTests 'scm-owner' @('-p','hiroute-platform','--test','windows_scm_owner') @(
    'native_standard_owner_credentials_select_profile_and_reader_failure_reverts_identity')
} elseif ($Scope -eq 'product') {
  cargo check --locked -p hiroute-daemon -p hiroute-cli --all-targets --keep-going 2>&1 | Tee-Object "$evidence/headless-build.log"
  if ($LASTEXITCODE -ne 0) { throw 'Full headless compilation failed; business scenarios are not executed' }
  Invoke-RequiredTests 'artifacts' @('-p','hiroute-local-storage','--lib','windows_artifacts_tests') @(
    'control::windows_artifacts_tests::windows_artifact_restart_restores_original_content_and_native_acl',
    'control::windows_artifacts_tests::windows_acl_change_loses_compensation_ownership_even_with_equal_content',
    'control::windows_artifacts_tests::windows_backup_authentication_binds_native_permission_facts',
    'control::windows_artifacts_tests::windows_owned_directory_cleanup_preserves_user_additions_and_replacements')
  New-Item -ItemType Directory -Force target/debug | Out-Null
  rustc --edition 2024 crates/cpa-bridge/tests/support/windows_cpa_fixture.rs -o target/debug/hiroute-cpa-process-fixture.exe
  if ($LASTEXITCODE -ne 0) { throw 'CPA fixture failed to build' }
  $env:HIROUTE_WINDOWS_CPA_FIXTURE = (Resolve-Path target/debug/hiroute-cpa-process-fixture.exe).Path
  Invoke-RequiredTests 'cpa' @('-p','hiroute-cpa-bridge','--lib','process::windows_tests') @(
    'process::windows_tests::native_bootstrap_delivers_stdin_eof_and_minimal_windows_environment',
    'process::windows_tests::native_root_exit_cannot_report_stopped_until_descendant_job_is_empty',
    'process::windows_tests::numeric_pid_is_never_an_orphan_termination_capability')
  Invoke-LibraryTests 'hiroute-cpa-bridge'
  cargo build --locked -p hiroute-cli --bin hiroute -p hiroute-daemon --bin hirouted 2>&1 | Tee-Object "$evidence/headless-link.log"
  if ($LASTEXITCODE -ne 0) { throw 'Production headless binaries failed to build' }
  $headless = 'installed_native_headless_roundtrip_and_graceful_then_forced_restart'
  $fragmented = 'server::core_runtime::adapters::tests::replay_ten_thousand_small_fields_use_one_stream_and_linear_template_scan'
  Invoke-RequiredTests 'replay-fragmentation' @('-p','hiroute-gateway','--lib',$fragmented) @($fragmented) @('--exact')
  Invoke-RequiredTests 'headless' @('-p','hiroute-daemon','--test','windows_headless',$headless) @($headless) @('--exact')
  Invoke-RequiredTests 'credential-options' @('-p','hiroute-cli','--lib','windows_handle_carrier_') @(
    'args::tests::windows_handle_carrier_rejects_missing_invalid_and_conflicting_authority',
    'args::tests::windows_handle_carrier_is_separate_from_a_typed_request_source')
  Invoke-RequiredTests 'worker-materials' @('-p','hiroute-daemon','--lib','windows_materials_') @(
    'delegation::local_worker::materials::tests::windows_materials_pin_root_and_cleanup_only_owned_children',
    'delegation::local_worker::materials::tests::windows_materials_refuse_an_existing_unowned_root')
  Invoke-RequiredTests 'worker-cleanup' @('-p','hiroute-daemon','--lib','windows_native_cleanup_') @(
    'delegation::native_cleanup::windows_tests::windows_native_cleanup_progress_preserves_marker_until_final_budget',
    'delegation::native_cleanup::windows_tests::windows_native_cleanup_refuses_replaced_marker_without_deleting_payload')
  Invoke-RequiredTests 'authority-key' @('-p','hiroute-daemon','--lib','authority_key_is_complete_stable') @(
    'control::runtime::observation_key::tests::authority_key_is_complete_stable_and_never_replaced_by_bad_lengths')
  Invoke-RequiredTests 'native-endpoint' @('-p','hiroute-daemon','--lib','windows_endpoint_fixture_') @(
    'control::runtime::agent_connection::discovery::endpoint_tests::windows_endpoint_fixture_registers_a_private_native_configuration')
  Invoke-RequiredTests 'native-home' @('-p','hiroute-daemon','--lib','windows_agent_discovery_uses_') @(
    'control::runtime::windows_agent_discovery_uses_the_private_isolated_test_home')
  Invoke-RequiredTests 'startup-admission' @('-p','hiroute-daemon','--bin','hirouted','standalone::bootstrap::tests::startup_reporting_failure_drains_actual_role_and_preserves_the_primary_error') @(
    'standalone::bootstrap::tests::startup_reporting_failure_drains_actual_role_and_preserves_the_primary_error') @('--exact')
  Invoke-RequiredTests 'live-capture' @('-p','hiroute-daemon','--lib','windows_live_capture_') @(
    'control::runtime::agent_live_check::windows::tests::windows_live_capture_observes_exit_and_bounds_final_stderr',
    'control::runtime::agent_live_check::windows::tests::windows_live_capture_timeout_stops_the_owned_tool_tree') @('--skip','windows_live_capture_fixture')
} elseif ($Scope -eq 'core') {
  # Entire native libraries, not an implementation-specific subset. Separate jobs
  # own platform primitives, CPA process fixtures and opt-in upstream clients.
  foreach ($package in (Get-WindowsCorePackages)) {
    Invoke-LibraryTests $package
  }
} elseif ($Scope -in @('background','upgrade')) {
  cargo build --locked -p hiroute-cli --bin hiroute -p hiroute-daemon --bin hirouted 2>&1 | Tee-Object "$evidence/headless-link.log"
  if ($LASTEXITCODE -ne 0) { throw 'Production background binaries failed to build' }
  $packageArgs = @('--expected-sha',$ExpectedSha,'--source-repo',$CpaSource,'--output',"$evidence/package",'--build-run-id',$env:GITHUB_RUN_ID)
  if ($CpaEvidence) {
    $packageArgs += @('--cpa-evidence',$CpaEvidence,'--cpa-original-run',$CpaOriginalRun,'--cpa-original-revision',$CpaOriginalRevision)
  }
  python scripts/windows-ci-package.py @packageArgs > "$evidence/package-result.json"
  if ($LASTEXITCODE -ne 0) { throw 'Production package prerequisites failed; background scenarios not executed' }
  $package = Get-Content "$evidence/package-result.json" -Raw | ConvertFrom-Json
  $env:HIROUTE_WINDOWS_PACKAGE_MANIFEST = $package.manifest
  $env:HIROUTE_WINDOWS_PACKAGE_ARCHIVE = $package.archive
  $env:HIROUTE_WINDOWS_PYTHON = $package.python
  $env:HIROUTE_WINDOWS_ADMIN_CLI = $package.administrator_cli
  $env:HIROUTE_WORKER_DAEMON_BINARY = $package.worker_daemon
  if ($Scope -eq 'background') {
    Invoke-RequiredTests 'background' @('-p','hiroute-daemon','--test','windows_background') @(
      'task_native_background_lua_gateway_control_stop_crash_recovery_and_autostart',
      'scm_standard_owner_pending_admin_finalize_session_zero_gateway_control_and_delete') @('--skip','windows_background_owner_fixture','--skip','windows_background_upgrade::','--nocapture')
  } else {
    $env:HIROUTE_WINDOWS_BASELINE_PACKAGE_MANIFEST = (Resolve-Path $BaselineManifest).Path
    $env:HIROUTE_WINDOWS_BASELINE_PACKAGE_ARCHIVE = (Resolve-Path $BaselineArchive).Path
    Invoke-RequiredTests 'upgrade' @('-p','hiroute-daemon','--test','windows_background','windows_background_upgrade::') @(
      'windows_background_upgrade::task_native_candidate_upgrade_recovers_prepared_rollback_and_preserves_business_state') @('--nocapture')
  }
} elseif ($Scope -eq 'accounts') {
  # Account acceptance is explicitly selected after official login preparation.
  # Ordinary PR jobs never receive credentials or claim these cases executed.
  Initialize-WindowsAgentInputs
  foreach ($recipe in (Get-WindowsQoderAccountRecipes)) {
    Invoke-RequiredTests $recipe.name @('-p',$recipe.package,'--lib',$recipe.case) @($recipe.case) @('--ignored','--exact','--nocapture')
  }
} else {
  $privateRoot = 'delegation::installation::installation_probe::windows_probe_root_creates_strict_private_directories_before_native_launch'
  Invoke-RequiredTests 'agent-private-root' @('-p','hiroute-daemon','--lib',$privateRoot) @($privateRoot) @('--exact','--nocapture')
  Initialize-WindowsAgentInputs
  $privateNode = 'agents::windows_runtime::tests::windows_selected_node_version_uses_the_private_capture_and_held_job'
  Invoke-RequiredTests 'agent-private-node' @('-p','hiroute-integrations','--lib',$privateNode) @($privateNode) @('--ignored','--exact','--nocapture')
  $pathIdentity = 'delegation::installation::installation_probe::windows_isolated_profile_uses_one_native_path_identity'
  Invoke-RequiredTests 'agent-path-identity' @('-p','hiroute-daemon','--lib',$pathIdentity) @($pathIdentity) @('--exact')
  $accountFixture = 'delegation::installation::installation_probe::windows_acceptance::windows_qoder_account_fixture_rejects_missing_or_ambiguous_official_profile'
  Invoke-RequiredTests 'agent-account-fixture' @('-p','hiroute-daemon','--lib',$accountFixture) @($accountFixture) @('--exact')
  $piSdk = 'agents::windows_runtime::tests::windows_selected_pi_sdk_capabilities_load_the_actual_package'
  Invoke-RequiredTests 'agent-pi-sdk' @('-p','hiroute-integrations','--lib',$piSdk) @($piSdk) @('--ignored','--exact','--nocapture')
  $codexIngress = 'agents::native_ingress_probe::ephemeral::tests::real_codex_ephemeral_authentication_without_managed_configuration'
  Invoke-RequiredTests 'agent-native-codex-ingress' @('-p','hiroute-integrations','--lib',$codexIngress) @($codexIngress) @('--ignored','--exact','--nocapture')
  Invoke-RequiredTests 'agent-business-oracle' @('-p','hiroute-daemon','--test','windows_headless','windows_worker_business::oracle::tests::') @(
    'windows_worker_business::oracle::tests::a_prompt_or_unrelated_or_error_tool_result_cannot_supply_a_native_receipt',
    'windows_worker_business::oracle::tests::a_running_native_shell_does_not_complete_even_after_printing_json',
    'windows_worker_business::oracle::tests::ambiguous_native_cli_stdout_is_rejected')
  $publicBusiness = 'actual_windows_complete_public_headless_business'
  Invoke-RequiredTests 'agent-public-business' @('-p','hiroute-daemon','--test','windows_headless',$publicBusiness) @($publicBusiness) @('--ignored','--exact','--nocapture')
  foreach ($case in (Get-WindowsDaemonAgentCases)) {
    $name = 'agent-' + ($case -split 'actual_')[1] -replace '_','-'
    Invoke-RequiredTests $name @('-p','hiroute-daemon','--lib',$case) @($case) @('--ignored','--exact','--nocapture')
  }
  foreach ($case in (Get-WindowsMainAgentCases)) {
    $name = 'main-' + ($case -split 'actual_windows_')[1] -replace '_','-'
    Invoke-RequiredTests $name @('-p','hiroute-integrations','--lib',$case) @($case) @('--ignored','--exact','--nocapture')
  }
}
} finally {
  if ($CaptureArtifacts) {
    $artifactArgs = @('finalize','--expected-sha',$ExpectedSha,'--evidence',$evidence)
    if ($Scope -in @('product','background','upgrade','agents','accounts')) { $artifactArgs += '--include-product-binaries' }
    if ($Scope -in @('background','upgrade') -and (Test-Path "$evidence/package-result.json") -and (Get-Item "$evidence/package-result.json").Length -gt 0) {
      $artifactArgs += @('--package-result',"$evidence/package-result.json")
    }
    python scripts/windows-native-artifacts.py @artifactArgs
    if ($LASTEXITCODE -ne 0) { $failures.Add('Native replay artifacts could not be finalized') }
    else {
      python scripts/windows-native-runtime-audit.py --expected-sha $ExpectedSha --evidence $evidence > "$evidence/runtime-audit.json"
      if ($LASTEXITCODE -ne 0) { $failures.Add('Native imports require an unavailable Debug CRT or could not be verified') }
    }
  }
  python scripts/windows-ci-report.py $evidence
  if ($LASTEXITCODE -ne 0) { throw 'Native validation feedback could not be generated' }
}
if ($failures.Count -gt 0) { throw ($failures -join '; ') }
