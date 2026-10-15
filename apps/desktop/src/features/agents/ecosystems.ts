import type { Agent, AgentModelSelection, AgentModelSurface } from './types';

/** A supported integration stays discoverable before a native profile is selected.
 * This entry is presentation only: no context or settings authority is invented.
 */
export function agentPageEntries(agents: Agent[]): Agent[] {
  if (agents.some(agent => agent.agent_id === 'agent_hermes_default')) return agents;
  return [...agents, {
    agent_id: 'agent_hermes_default', version: '', context_id: null,
    configuration_state: 'profile_not_selected', settings: null, status_error: null,
  }];
}

export type AgentEcosystem = 'codex' | 'claude' | 'qoder' | 'pi' | 'dsh' | 'hermes';

/** Only these current integrations have settings editors; discovery is independent. */
export function agentEcosystem(agentId: string): AgentEcosystem | null {
  switch (agentId) {
    case 'agent_codex_default': return 'codex';
    case 'agent_claude_default': return 'claude';
    case 'agent_dsh_default': return 'dsh';
    case 'agent_pi_default': return 'pi';
    case 'agent_qoder_default': return 'qoder';
    case 'agent_hermes_default': return 'hermes';
    default: return null;
  }
}

export function agentSupportsModelRouting(agentId: string): boolean {
  const ecosystem = agentEcosystem(agentId);
  return ecosystem === 'codex' || ecosystem === 'claude' || ecosystem === 'qoder' || ecosystem === 'pi' || ecosystem === 'dsh' || ecosystem === 'hermes';
}

/** A saved selection must belong to the actual integration, including token-only edits. */
export function agentModelSelectionMatches(agentId: string, selection: AgentModelSelection): boolean {
  switch (agentEcosystem(agentId)) {
    case 'codex': return selection.mode === 'codex_default';
    case 'claude': return selection.mode === 'claude_launcher';
    case 'dsh': return selection.mode === 'dsh_additional';
    case 'pi': return selection.mode === 'pi_additional';
    case 'hermes': return selection.mode === 'hermes_default';
    case 'qoder': return selection.mode === 'qoder_additional';
    default: return false;
  }
}

export function agentDisplayName(agentId: string, language: 'zh' | 'en'): string {
  switch (agentEcosystem(agentId)) {
    case 'codex': return 'Codex';
    case 'claude': return 'Claude Code';
    case 'dsh': return 'DeepSeek Harness';
    case 'pi': return 'Pi';
    case 'qoder': return 'Qoder';
    case 'hermes': return 'Hermes';
    default: return agentId.toLowerCase().includes('cursor') ? 'Cursor' : language === 'zh' ? '本机 Agent' : 'Local Agent';
  }
}

export function agentBrand(agentId: string): 'codex' | 'claude-code' | 'qoder' | 'pi' | 'dsh' | 'hermes' | 'agent' {
  switch (agentEcosystem(agentId)) {
    case 'codex': return 'codex';
    case 'claude': return 'claude-code';
    case 'dsh': return 'dsh';
    case 'pi': return 'pi';
    case 'qoder': return 'qoder';
    case 'hermes': return 'hermes';
    default: return 'agent';
  }
}

export function surfaceName(surface: AgentModelSurface): string {
  switch (surface) {
    case 'codex_desktop': return 'Codex Desktop';
    case 'codex_cli': return 'Codex CLI';
    case 'claude_cli': return 'Claude Code CLI';
    case 'dsh_cli': return 'DeepSeek Harness';
    case 'pi_cli': return 'Pi CLI';
    case 'qoder_cli': return 'Qoder CLI';
    case 'hermes_cli': return 'Hermes';
    default: return surface;
  }
}

export function agentSupportsCollaboration(agentId: string): boolean {
  const ecosystem = agentEcosystem(agentId);
  return ecosystem !== null && ecosystem !== 'hermes';
}
