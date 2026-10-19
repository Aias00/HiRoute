/** Authenticated route hints contain neither a URL nor executable text. */
export type ConsoleLanding = 'first-run' | `agent:${'codex' | 'claude-code'}` | `suggestion:${string}`;
export function validConsoleLanding(value: unknown): value is ConsoleLanding {
  return typeof value === 'string' && (value === 'first-run' || /^agent:(codex|claude-code)$/.test(value) || /^suggestion:[A-Za-z0-9._-]{1,128}$/.test(value));
}
let pending: ConsoleLanding | null = null;
export function acceptConsoleLanding(value: unknown): void { pending = validConsoleLanding(value) ? value : null; }
export function takeConsoleLanding(): ConsoleLanding | null { const result = pending; pending = null; return result; }
