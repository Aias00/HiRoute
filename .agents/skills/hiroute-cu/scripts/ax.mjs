// Portions adapted from Sac-Y/Jev-cu; Copyright (c) 2026 Sac-Y.
// MIT notice: ../references/jev-LICENSE.txt
// Adapted from jev-cu 12d8e6d; see references/jev-origin.md.
export const actions = Object.freeze(['click_element', 'set_value', 'scroll']);
const roles = [
  'standard window', 'radio button', 'pop up button', 'text field', 'search field',
  'toggle button', 'combo box', 'scroll area', 'menu item', 'HTML content',
  'button', 'checkbox', 'heading', 'summary', 'tab', 'link', 'text', 'container',
];

export function parseAX(ax) {
  return String(ax).split(/\r?\n/).flatMap(line => {
    const match = line.match(/^\s*(\d+)\s+(.*)$/);
    if (!match) return [];
    const rest = match[2];
    const role = roles.find(r => rest === r || rest.startsWith(r + ' ')) ?? rest.split(' ')[0];
    return [{index: Number(match[1]), role,
      label: rest.slice(role.length).trim().replace(/,?\s*Secondary Actions:.*$/i, '').trim()}];
  });
}

// AX indices and the focused-element index can change without business progress.
export function stateKey(ax) {
  return String(ax).split(/\r?\n/)
    .filter(line => !/focused UI element/i.test(line))
    .map(line => line.replace(/^\s*\d+\s+/, '').trim()).filter(Boolean).join('\n');
}

// Sky may launch the explicitly named app. The shared loop needs full AX text.
export function createSkyDriver(sky, app) {
  if (typeof app !== 'string' || !app.trim()) throw new Error('app_required');
  return {
    observe: async () => (await sky.get_app_state({app, disableDiff: true})).text,
    execute: action => {
      switch (action.kind) {
        case 'click_element': return sky.click({app, element_index: action.targetIndex});
        case 'set_value': return sky.set_value({app, element_index: action.targetIndex, value: action.text});
        case 'scroll': return sky.scroll({app, element_index: action.targetIndex, direction: action.direction, pages: 1});
        default: throw new Error('unsupported_action');
      }
    },
  };
}

// Takes an already selected native app: no hidden app discovery or launch.
export function createCuaDriver(app) {
  return {
    observe: () => app.getAXState({emit: false, disableDiffing: true}),
    execute: action => {
      switch (action.kind) {
        case 'click_element': return app.click(action.targetIndex);
        case 'set_value': return app.setValue(action.targetIndex, action.text);
        case 'scroll': return app.scroll(action.targetIndex, action.direction, 1);
        default: throw new Error('unsupported_action');
      }
    },
  };
}
