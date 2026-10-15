import test from 'node:test';
import assert from 'node:assert/strict';
import {createSkyDriver, createBudget, runTask} from '../scripts/loop.mjs';

test('Sky maps full AX and all supported indexed actions to the selected app', async () => {
  const calls = [];
  const sky = Object.fromEntries(['get_app_state', 'click', 'set_value', 'scroll'].map(name =>
    [name, async args => { calls.push([name, args]); return {text: '1 button Agent', screenshot: {url: 'file:///unused'}}; }]));
  const driver = createSkyDriver(sky, 'HiRoute');
  assert.equal(await driver.observe(), '1 button Agent');
  await driver.execute({kind: 'click_element', targetIndex: 1});
  await driver.execute({kind: 'set_value', targetIndex: 2, text: '测试'});
  await driver.execute({kind: 'scroll', targetIndex: 3, direction: 'down'});
  assert.deepEqual(calls, [
    ['get_app_state', {app: 'HiRoute', disableDiff: true}],
    ['click', {app: 'HiRoute', element_index: 1}],
    ['set_value', {app: 'HiRoute', element_index: 2, value: '测试'}],
    ['scroll', {app: 'HiRoute', element_index: 3, direction: 'down', pages: 1}],
  ]);
  assert.throws(() => driver.execute({kind: 'click_at'}), /unsupported_action/);
  assert.throws(() => createSkyDriver(sky, ''), /app_required/);
  assert.equal(calls.length, 4);
});

test('Sky feeds the existing loop, refreshes before click and observes its result', async () => {
  const calls = [];
  let done = false;
  const driver = createSkyDriver({
    get_app_state: async () => { calls.push('observe'); return {text: done ? '2 heading Codex' : '1 button Agent'}; },
    click: async () => { calls.push('click'); done = true; },
  }, 'HiRoute');
  const result = await runTask({driver, appName: 'HiRoute', goal: 'Open Agent',
    budget: createBudget(), dryRun: false, select: () => ({action: 'click_element', targetIndex: 1}),
    scenario: {
      allowedActions: ['click_element'],
      project: () => ({context: '', candidates: [{index: 1, role: 'button', label: 'Agent'}]}),
      beforeAction: () => true, afterAction: ({ax}) => ax.includes('Codex'),
      verify: ({ax}) => ax.includes('Codex'),
    },
  });
  assert.equal(result.verified, true);
  assert.deepEqual(calls, ['observe', 'observe', 'click', 'observe']);
});

test('Sky errors propagate without retry or alternate UI calls', async () => {
  let attempts = 0;
  const driver = createSkyDriver({click: async () => { attempts++; throw new Error('unavailable'); }}, 'HiRoute');
  await assert.rejects(driver.execute({kind: 'click_element', targetIndex: 1}), /unavailable/);
  assert.equal(attempts, 1);
});
