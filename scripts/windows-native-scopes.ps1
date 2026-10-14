# Shared required inventories for the native producer and artifact consumer.
function Get-WindowsDaemonAgentCases {
  return @(
    'delegation::installation::tests::actual_codex_acp_acceptance_uses_the_selected_installation',
    'delegation::installation::tests::actual_claude_acp_acceptance_uses_the_selected_installation',
    'delegation::installation::installation_probe::windows_acceptance::actual_windows_pi_new_load_and_cancel_use_the_selected_sdk',
    'delegation::installation::installation_probe::windows_acceptance::actual_windows_dsh_new_load_and_cancel_use_the_selected_native_acp',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_codex_skills_read_edit_shell_and_exact_history',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_claude_skills_read_edit_shell_and_exact_history',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_pi_skills_read_edit_shell_and_exact_history',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_dsh_skills_read_edit_shell_and_exact_history')
}
function Get-WindowsMainAgentCases {
  return @(
    'agents::windows_collaboration_tests::actual_windows_codex_native_skill_executes_the_public_cli',
    'agents::windows_collaboration_tests::actual_windows_claude_native_skill_executes_the_public_cli')
}
function Get-WindowsLiveHistoryCases {
  $prefix = 'delegation::lifecycle::tests::terminal::'
  return @(
    ($prefix + 'claude_terminal_checkpoint_keeps_transport_live_until_final_tail'),
    ($prefix + 'eof_before_claude_checkpoint_prevents_final_tail'),
    ($prefix + 'claude_cancel_and_protocol_error_do_not_wait_for_history'),
    ($prefix + 'claude_disconnect_after_terminal_keeps_completion_without_history_ready'),
    ($prefix + 'interrupted_terminal_checkpoint_retains_lease_completion_and_false_readiness'),
    ($prefix + 'terminal_publication_failure_does_not_wait_or_double_write'),
    ($prefix + 'execute_outer_cancel_and_deadline_after_completion_preserve_terminal_receipt'),
    ($prefix + 'claude_completed_settled_history_does_not_replace_failed_owned_stop_evidence'))
}
function Get-WindowsQoderAccountRecipes {
  return @(
    @{ name = 'account-qoder-lifecycle'; package = 'hiroute-daemon'; target = @('--lib'); case = 'delegation::installation::installation_probe::windows_acceptance::actual_windows_qoder_new_load_and_cancel_use_the_selected_native_acp' },
    @{ name = 'account-qoder-tools'; package = 'hiroute-daemon'; target = @('--lib'); case = 'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_qoder_skills_read_edit_shell_and_exact_history' },
    @{ name = 'account-qoder-main'; package = 'hiroute-integrations'; target = @('--lib'); case = 'agents::windows_collaboration_tests::actual_windows_qoder_native_skill_executes_the_public_cli_without_synthesized_account' },
    @{ name = 'account-qoder-user-models'; package = 'hiroute-daemon'; target = @('--test','windows_headless'); case = 'actual_windows_qoder_persisted_user_models_with_official_account' })
}
function Get-WindowsCorePackages {
  return @('hiroute-domain','hiroute-application-api','hiroute-application',
    'hiroute-client-core','hiroute-observation','hiroute-integrations','hiroute-local-storage',
    'hiroute-gateway-core','hiroute-gateway','hiroute-host-runtime','hiroute-cli')
}
function Get-WindowsNativeReaderCases {
  $prefix = 'control::windows_native_reader_tests::'
  return @(
    ($prefix + 'windows_native_reader_ports_preserve_missing_and_borrowed_content_permissions'),
    ($prefix + 'windows_native_reader_enforces_cap_before_read_and_accepts_exact_bound'),
    ($prefix + 'windows_native_reader_rejects_directory_hardlink_and_junction_aliases'),
    ($prefix + 'windows_native_reader_lease_denies_data_write_and_file_or_ancestor_replacement'),
    ($prefix + 'windows_native_reader_rejects_live_writer_without_changing_source'),
    ($prefix + 'windows_native_reader_rejects_acl_change_on_same_held_identity'),
    ($prefix + 'windows_native_reader_reopen_binds_identity_without_changing_fingerprint_contract'),
    ($prefix + 'windows_native_reader_private_restore_and_native_cipher_reopen_preserve_authority'))
}
function Get-WindowsRequiredRecipes {
  param([Parameter(Mandatory)][string]$Scope)
  switch ($Scope) {
    'components' { return @('platform','read-leases','platform-unit','jobs','diagnostics','scm-owner') }
    'product' { return @('artifacts','cpa','core-cpa-bridge','replay-fragmentation','headless','credential-options','worker-materials',
      'worker-cleanup','authority-key','native-endpoint','native-home','startup-admission','live-capture') }
    'core' { return @(Get-WindowsCorePackages | ForEach-Object { $_ -replace '^hiroute-','core-' }) }
    'background' { return @('background-capture','packaged-proxy-oracle','packaged-foreground','background','background-standard-owner') }
    'upgrade' { return @('upgrade') }
    'accounts' { return @(Get-WindowsQoderAccountRecipes | ForEach-Object { $_.name }) }
    'agents' {
      return @('agent-private-root','agent-private-node','agent-path-identity','agent-managed-path','agent-account-fixture','agent-pi-sdk',
        'agent-acp-notifications','agent-cancel-oracle','agent-main-oracle','agent-launch-oracle','agent-launch-cause','agent-cli-selection','agent-npm-inspection','agent-pi-locator','agent-native-reader','agent-worker-profile-identity','agent-worker-directory-identity','agent-worker-failure-oracle','agent-history-oracle',
        'agent-native-codex-ingress','agent-native-source-oracle','agent-worker-dependency-oracle','agent-business-oracle','agent-http','agent-public-business','agent-user-models','agent-user-model-oracle','agent-qoder-user-model-oracle','agent-management','agent-management-oracle','agent-live-history','agent-legacy-history') +
        @(Get-WindowsDaemonAgentCases | ForEach-Object { 'agent-' + (($_ -split 'actual_')[1] -replace '_','-') }) +
        @(Get-WindowsMainAgentCases | ForEach-Object { 'main-' + (($_ -split 'actual_windows_')[1] -replace '_','-') })
    }
    default { throw 'Unknown required native scope' }
  }
}
