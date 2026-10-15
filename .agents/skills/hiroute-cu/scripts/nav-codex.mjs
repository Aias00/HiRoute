// Concrete read-only NAV-02 recipe. No credentials, settings or conversation
// titles are projected; only navigation controls and known page states.
import {parseAX} from './ax.mjs';

function page(ax) {
  const elements = parseAX(ax);
  if (elements.some(e => ['dialog', 'sheet'].includes(e.role))) return 'unknown';
  if (elements.some(e => e.role === 'heading' && e.label === '首页, Value: 1')) return 'home';
  if (!elements.some(e => e.role === 'heading' && e.label === 'Agent, Value: 1')) return 'unknown';
  if (elements.some(e => e.role === 'heading' && e.label === 'Codex, Value: 2')) return 'codex';
  if (elements.some(e => e.role === 'heading' && e.label === 'Claude Code, Value: 2')) return 'claude';
  return 'unknown';
}
function allowed(ax, element) {
  return element?.role === 'button' && (page(ax) === 'home' && element.label === 'Agent'
    || page(ax) === 'claude' && element.label.startsWith('Codex logo Codex'));
}
export const scenario = {
  allowedActions: ['click_element'],
  project: ({ax}) => ({context: `Current page: ${page(ax)}. Read-only navigation to Codex details.`,
    candidates: parseAX(ax).filter(e => allowed(ax, e)).map(e => ({...e, readOnly: true,
      label: e.label === 'Agent' ? 'Agent' : 'Codex'}))}),
  beforeAction: ({ax, action, element}) => action.kind === 'click_element'
    && action.targetIndex === element.index && allowed(ax, element),
  afterAction: ({before, ax}) => page(before) === 'home' && ['claude', 'codex'].includes(page(ax))
    || page(before) === 'claude' && page(ax) === 'codex',
  verify: ({ax}) => page(ax) === 'codex',
};

// Codex distilled the observed path into semantic selection, not recorded AX
// indices. Ambiguity returns control to Codex; this never invokes Jev itself.
export function select({candidates, context}) {
  const label = context.startsWith('Current page: home.') ? 'Agent'
    : context.startsWith('Current page: claude.') ? 'Codex' : null;
  const matches = candidates.filter(c => c.role === 'button' && c.label === label);
  return matches.length === 1 ? {action: 'click_element', targetIndex: matches[0].index} : null;
}

// Optional exploration screening, not imported/called by the batch selector.
// This narrow projection covers navigation and known visible error banners only;
// it is not a general business-health oracle or a full AX upload.
export function review({ax}) {
  const errors = parseAX(ax).filter(e => ['text', 'static text'].includes(e.role)
    && /^(本机服务启动失败|部分信息暂时无法读取|状态刷新失败|上一项后台变更状态暂不可用)$/.test(e.label));
  return {expectation: 'Navigation should show Home or Agent details. Clicking Agent may show either Claude Code or Codex. Clicking Codex must show Codex details. No service/read failure is expected. A not-connected agent is normal; connection status is not under test.',
    facts: [`Current page: ${page(ax)}.`, ...errors.slice(0, 10).map(e => e.label)]};
}
