# Shared required inventories for the native producer and artifact consumer.
function Get-WindowsDaemonAgentCases {
  return @(
    'delegation::installation::tests::actual_codex_acp_acceptance_uses_the_selected_installation',
    'delegation::installation::tests::actual_claude_acp_acceptance_uses_the_selected_installation',
    'delegation::installation::installation_probe::windows_acceptance::actual_windows_pi_new_load_and_cancel_use_the_selected_sdk',
    'delegation::installation::installation_probe::windows_acceptance::actual_windows_dsh_new_load_and_cancel_use_the_selected_native_acp',
    'delegation::installation::installation_probe::windows_acceptance::actual_windows_qoder_new_load_and_cancel_use_the_selected_native_acp',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_codex_skills_read_edit_shell_and_exact_history',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_claude_skills_read_edit_shell_and_exact_history',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_pi_skills_read_edit_shell_and_exact_history',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_dsh_skills_read_edit_shell_and_exact_history',
    'delegation::installation::installation_probe::windows_acceptance::windows_tools::actual_windows_qoder_skills_read_edit_shell_and_exact_history')
}
function Get-WindowsMainAgentCases {
  return @(
    'agents::windows_collaboration_tests::actual_windows_codex_native_skill_executes_the_public_cli',
    'agents::windows_collaboration_tests::actual_windows_claude_native_skill_executes_the_public_cli',
    'agents::windows_collaboration_tests::actual_windows_qoder_native_skill_executes_the_public_cli_without_synthesized_account')
}
function Get-WindowsCorePackages {
  return @('hiroute-domain','hiroute-application-api','hiroute-application',
    'hiroute-client-core','hiroute-observation','hiroute-integrations','hiroute-local-storage',
    'hiroute-gateway-core','hiroute-gateway','hiroute-host-runtime','hiroute-cli')
}
function Get-WindowsRequiredRecipes {
  param([Parameter(Mandatory)][string]$Scope)
  switch ($Scope) {
    'components' { return @('platform','platform-unit','jobs','diagnostics','scm-owner') }
    'product' { return @('artifacts','cpa','headless','credential-options','worker-materials',
      'worker-cleanup','authority-key','native-endpoint','native-home','startup-admission','live-capture') }
    'core' { return @(Get-WindowsCorePackages | ForEach-Object { $_ -replace '^hiroute-','core-' }) }
    'background' { return @('background') }
    'upgrade' { return @('upgrade') }
    'agents' {
      return @('agent-private-root','agent-private-node') +
        @(Get-WindowsDaemonAgentCases | ForEach-Object { 'agent-' + (($_ -split 'actual_')[1] -replace '_','-') }) +
        @(Get-WindowsMainAgentCases | ForEach-Object { 'main-' + (($_ -split 'actual_windows_')[1] -replace '_','-') })
    }
    default { throw 'Unknown required native scope' }
  }
}
