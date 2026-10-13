param(
  [ValidateSet('components','product','background')][string]$Scope,
  [Parameter(Mandatory)][string]$ExpectedSha,
  [Parameter(Mandatory)][string]$Evidence,
  [string]$Plan = 'windows-headless',
  [ValidateSet('focused','final')][string]$Phase = 'focused',
  [string[]]$RelatedRun = @(),
  [string]$CpaEvidence,
  [string]$CpaSource
)
$ErrorActionPreference = 'Stop'
$evidence = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Evidence)
$failures = [System.Collections.Generic.List[string]]::new()
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
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
               plan = $Plan; phase = $Phase; related_runs = $RelatedRun }
  $started = Get-Date
  try {
    cargo test --locked @CargoArguments -- --list 2>&1 | Tee-Object "$evidence/$Name-inventory.log"
    if ($LASTEXITCODE -ne 0) { throw "$Name inventory failed to build" }
    $inventory = Get-Content "$evidence/$Name-inventory.log" -Raw
    foreach ($case in $Required) {
      $escaped = [regex]::Escape($case)
      if ($inventory -notmatch "(?m)^${escaped}: test`r?$") { throw "Required case not selected: $case" }
    }
    cargo test --locked @CargoArguments -- --test-threads=1 @HarnessArguments 2>&1 | Tee-Object "$evidence/$Name.log"
    $result.process_exit = $LASTEXITCODE
    $log = Get-Content "$evidence/$Name.log" -Raw
    foreach ($case in $Required) {
      $escaped = [regex]::Escape($case)
      if ($log -match "(?m)^test ${escaped} \.\.\. (ok|FAILED)`r?$") { $result.executed++ }
    }
    if ($LASTEXITCODE -ne 0) { throw "$Name native acceptance failed" }
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

if ($Scope -eq 'components') {
  Invoke-RequiredTests 'platform' @('-p','hiroute-platform','--test','windows') @(
    'private_file_identity_is_held_and_reparse_free',
    'ancestor_junction_cannot_redirect_private_read_or_creation',
    'pipe_exclusivity_bidirectional_frames_and_peer_identity',
    'pipe_deadline_cancels_read_without_consuming_the_next_frame',
    'native_permissions_survive_atomic_restore_and_detect_acl_changes')
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
  cargo build --locked -p hiroute-cli --bin hiroute -p hiroute-daemon --bin hirouted 2>&1 | Tee-Object "$evidence/headless-link.log"
  if ($LASTEXITCODE -ne 0) { throw 'Production headless binaries failed to build' }
  Invoke-RequiredTests 'headless' @('-p','hiroute-daemon','--test','windows_headless') @(
    'installed_native_headless_roundtrip_and_graceful_then_forced_restart')
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
} else {
  cargo build --locked -p hiroute-cli --bin hiroute -p hiroute-daemon --bin hirouted 2>&1 | Tee-Object "$evidence/headless-link.log"
  if ($LASTEXITCODE -ne 0) { throw 'Production background binaries failed to build' }
  python scripts/windows-ci-package.py --expected-sha $ExpectedSha --source-repo $CpaSource --cpa-evidence $CpaEvidence --output "$evidence/package" > "$evidence/package-result.json"
  if ($LASTEXITCODE -ne 0) { throw 'Production package prerequisites failed; background scenarios not executed' }
  $package = Get-Content "$evidence/package-result.json" -Raw | ConvertFrom-Json
  $env:HIROUTE_WINDOWS_PACKAGE_MANIFEST = $package.manifest
  $env:HIROUTE_WINDOWS_PACKAGE_ARCHIVE = $package.archive
  $env:HIROUTE_WINDOWS_PYTHON = $package.python
  $env:HIROUTE_WINDOWS_ADMIN_CLI = $package.administrator_cli
  $env:HIROUTE_WORKER_DAEMON_BINARY = $package.worker_daemon
  Invoke-RequiredTests 'background' @('-p','hiroute-daemon','--test','windows_background') @(
    'task_native_background_lua_gateway_control_stop_crash_recovery_and_autostart',
    'scm_standard_owner_pending_admin_finalize_session_zero_gateway_control_and_delete') @('--skip','windows_background_owner_fixture','--nocapture')
}
if ($failures.Count -gt 0) { throw ($failures -join '; ') }
