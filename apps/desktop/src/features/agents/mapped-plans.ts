import type { AgentModelSelection } from './types';
export type MappedPlan = { planId: string; slots: ('default' | 'opus' | 'sonnet' | 'haiku')[] };
/** Every accepted mapping is retained. Start slots annotate the complete list. */
export function mappedPlanList(selection: AgentModelSelection | null | undefined): MappedPlan[] {
  const plans = new Map<string, MappedPlan>();
  const add = (planId: string, slot?: MappedPlan['slots'][number]) => {
    const row = plans.get(planId) ?? { planId, slots: [] };
    if (slot && !row.slots.includes(slot)) row.slots.push(slot);
    plans.set(planId, row);
  };
  if (selection?.mode === 'codex_default') {
    for (const id of selection.allowed_plan_ids) add(id);
    if (selection.default_selection.kind === 'plan') add(selection.default_selection.plan_id, 'default');
  } else if (selection?.mode === 'claude_launcher') {
    for (const slot of ['opus', 'sonnet', 'haiku'] as const) {
      const mapping = selection.preset_mappings[slot];
      if (mapping.kind === 'plan') add(mapping.plan_id, slot);
    }
  }
  return [...plans.values()];
}
