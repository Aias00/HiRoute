import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import {readFile, stat} from 'node:fs/promises';
import {runTask, createBudget} from '../scripts/loop.mjs';
import {parseAX, stateKey, createCuaDriver} from '../scripts/ax.mjs';
import {decide, normalizeDecision} from '../scripts/jev.mjs';
import {scenario as navCodex, select as selectCodex} from '../scripts/nav-codex.mjs';
import {saveTrace} from '../scripts/trace.mjs';

const frame = (page = '首页', other = 'unchanged') =>
  `Window: "HiRoute", App: HiRoute.\n0 standard window HiRoute\n1 button Agent\n2 button Codex\n3 text field 默认模型\n4 button 删除账号\n5 heading ${page}, Value: 1\n6 text other=${other}`;
const choice = overrides => ({action: 'click_element', targetIndex: 1,
  confidence: 1, risk: 0, done: 0, ...overrides});
const task = overrides => ({allowedActions: ['click_element'],
  project: ({ax}) => ({candidates: parseAX(ax).filter(e => e.role === 'button' && e.index !== 4).map(e => ({...e, readOnly: true})), context: 'Test navigation'}),
  beforeAction: ({action}) => action.kind === 'click_element' && action.targetIndex === 1,
  afterAction: ({ax}) => ax.includes('other=unchanged'),
  verify: ({ax}) => ax.includes('heading Agent,'), ...overrides});
function fixture({initial = frame(), apply = () => frame('Agent')} = {}) {
  let ax = initial;
  const executed = [];
  return {executed, setAX: next => { ax = next; }, driver: {
    observe: async () => ax,
    execute: async action => {executed.push(action); ax = apply(ax, action);},
  }};
}
const run = (f, overrides = {}) => runTask({driver: f.driver, appName: 'HiRoute', goal: 'Open Agent',
  scenario: task(), budget: createBudget(), dryRun: false, decide: async () => choice(), ...overrides});

test('AX CRLF parsing and progress ignore index/focus churn', () => {
  assert.equal(parseAX('1 pop up button 菜单\r\n2 text field Name, Secondary Actions: Clear')[1].label, 'Name');
  assert.equal(stateKey('1 button Next\nThe focused UI element is 1 button Next'), stateKey('9 button Next'));
});

test('concrete NAV-02 reaches Codex in two guarded navigation steps', async () => {
  const home = '1 heading 首页, Value: 1\n2 button Agent';
  const claude = '3 heading Agent, Value: 1\n4 heading Claude Code, Value: 2\n5 button Codex logo Codex 已发现';
  const codex = '6 heading Agent, Value: 1\n7 heading Codex, Value: 2';
  const f = fixture({initial: home, apply: ax => ax === home ? claude : codex});
  const result = await run(f, {scenario: navCodex,
    decide: async ({candidates}) => choice({targetIndex: candidates[0].index})});
  assert.equal(result.verified, true);
  assert.equal(result.stepsUsed, 2);
  assert.deepEqual(f.executed.map(a => a.targetIndex), [2, 5]);
  assert.equal(navCodex.afterAction({before: home, ax: '1 heading 设置, Value: 1'}), false);
});

test('safe API error code is preserved without action or arbitrary exception data', async () => {
  for (const message of ['jev_http_429', 'jev_http_500 PRIVATE_KEY']) {
    const f = fixture();
    const result = await run(f, {decide: async () => {throw Error(message);}});
    assert.equal(result.reason, message === 'jev_http_429' ? message : 'decide_failed');
    assert.equal(f.executed.length, 0);
    assert.ok(!JSON.stringify(result).includes('PRIVATE_KEY'));
  }
});

test('missing mandatory guards cannot execute, including dry-run', async () => {
  for (const field of ['project', 'beforeAction', 'afterAction', 'verify']) {
    const f = fixture();
    const result = await run(f, {scenario: task({[field]: undefined})});
    assert.equal(result.reason, 'invalid_contract');
    assert.equal(f.executed.length, 0);
  }
});

test('last permitted action is post-checked and verified', async () => {
  const f = fixture();
  const phases = [];
  const result = await run(f, {budget: createBudget({maxSteps: 1}), scenario: task({
    beforeAction: () => {phases.push('before'); return true;},
    afterAction: () => {phases.push('after'); return true;},
    verify: ({ax}) => {phases.push('verify'); return ax.includes('heading Agent,');},
  })});
  assert.equal(result.verified, true);
  assert.deepEqual(phases, ['verify', 'before', 'after', 'verify']);
  assert.equal(f.executed.length, 1);
});

test('projection is the hard candidate scope, not the whole visible page', async () => {
  const f = fixture();
  const result = await run(f, {scenario: task({project: ({ax}) => ({candidates: parseAX(ax).filter(e => e.index === 1).map(e => ({...e, readOnly: true})), context: ''})}),
    decide: async () => choice({targetIndex: 2})});
  assert.equal(result.reason, 'target_out_of_scope');
  assert.equal(f.executed.length, 0);
});

test('action outside declared types is rejected before execution', async () => {
  const f = fixture();
  const result = await run(f, {decide: async () => choice({action: 'set_value'})});
  assert.equal(result.reason, 'action_out_of_scope');
  assert.equal(f.executed.length, 0);
});

test('batch host input values are checked; selector cannot override text', async () => {
  for (const text of ['unexpected-model', 'expected-model']) {
    const f = fixture();
    const result = await run(f, {scenario: task({allowedActions: ['set_value'],
      project: ({ax}) => ({candidates: parseAX(ax).filter(e => e.index === 3), context: ''}),
      resources: () => ({text}),
      beforeAction: ({action}) => action.targetIndex === 3 && action.text === 'expected-model',
    }), decide: undefined, select: async () => ({action: 'set_value', targetIndex: 3, text: 'selector-injected-value'})});
    assert.equal(result.status, text === 'expected-model' ? 'done' : 'escalate');
    assert.equal(f.executed.length, text === 'expected-model' ? 1 : 0);
    if (f.executed.length) assert.equal(f.executed[0].text, text);
  }
});

test('postcondition detects unrelated mutation even when final goal matches', async () => {
  const f = fixture({apply: () => frame('Agent', 'changed')});
  const budget = createBudget();
  const result = await run(f, {budget});
  assert.equal(result.reason, 'after_action_rejected');
  assert.equal(result.verified, false);
  assert.equal(result.needsReconcile, true);
  assert.equal((await run(f, {budget})).reason, 'reconciliation_required');
});

test('failed guard and exception do not count as true and never leak errors', async () => {
  for (const beforeAction of [() => undefined, () => 'true', () => {throw Error('PRIVATE_KEY');}]) {
    const f = fixture();
    const result = await run(f, {scenario: task({beforeAction})});
    assert.equal(result.status, 'escalate');
    assert.equal(f.executed.length, 0);
    assert.ok(!JSON.stringify(result).includes('PRIVATE_KEY'));
  }
});

test('self-reported done never overrides local verify', async () => {
  const f = fixture();
  assert.equal((await run(f, {decide: async () => choice({done: 1})})).reason, 'unverified_done');
  assert.equal(f.executed.length, 0);
});

test('risk and true AX label cannot be spoofed by renamed projection/model', async () => {
  const f = fixture();
  const result = await run(f, {scenario: task({project: () => ({context: '', candidates: [{index: 4, role: 'button', label: 'Safe', readOnly: true}]})}),
    decide: async () => choice({targetIndex: 4, targetLabel: 'Safe'})});
  assert.equal(result.status, 'confirm');
  assert.equal(f.executed.length, 0);
});

test('malformed probabilities are never allowed, including a high done', async () => {
  const f = fixture();
  assert.equal((await run(f, {decide: async () => choice({done: 1, risk: null})})).reason, 'invalid_decision');
  assert.equal(f.executed.length, 0);
});

test('two consecutive no-progress actions stop before the third', async () => {
  const f = fixture({apply: ax => ax});
  const result = await run(f);
  assert.equal(result.reason, 'no_progress');
  assert.equal(result.stepsUsed, 2);
  assert.equal(f.executed.length, 2);
});

test('stale observation after decision stops, never clicks changed index', async () => {
  const f = fixture();
  const result = await run(f, {decide: async () => {f.setAX(frame('Another page')); return choice();}});
  assert.equal(result.reason, 'observation_changed');
  assert.equal(f.executed.length, 0);
});

test('dry-run sends decision but executes nothing; shared call budget persists', async () => {
  const f = fixture();
  const budget = createBudget({maxCalls: 1});
  assert.equal((await run(f, {budget, dryRun: true})).status, 'dry_run');
  assert.equal((await run(f, {budget})).reason, 'call_budget');
  assert.equal(f.executed.length, 0);
});

test('takeover resumes from fresh observation using the same cumulative budget', async () => {
  const f = fixture();
  const budget = createBudget({maxCalls: 2});
  assert.equal((await run(f, {budget, scenario: task({beforeAction: () => false})})).reason, 'before_action_rejected');
  assert.equal((await run(f, {budget})).status, 'done');
  assert.equal(budget.callsUsed, 2);
  assert.equal(budget.stepsUsed, 1);
});

test('takeover cannot reset the step limit', async () => {
  const f = fixture({apply: () => frame('Intermediate')});
  const budget = createBudget({maxSteps: 1});
  assert.equal((await run(f, {budget})).reason, 'step_budget');
  assert.equal((await run(f, {budget})).reason, 'step_budget');
  assert.equal(f.executed.length, 1);
});

test('cancellation before start does not query or mutate the app', async () => {
  const f = fixture();
  const c = new AbortController(); c.abort();
  f.driver.observe = async () => {throw Error('should not observe');};
  assert.equal((await run(f, {signal: c.signal})).reason, 'cancelled');
});

test('deadline bounds an unresponsive decision and holds budget until settled', async () => {
  const f = fixture(); let release;
  const budget = createBudget({timeoutMs: 30});
  const result = await run(f, {budget, decide: () => new Promise(resolve => {release = resolve;})});
  assert.equal(result.reason, 'deadline');
  assert.equal(f.executed.length, 0);
  assert.equal(budget.busy, true);
  release(choice()); await new Promise(resolve => setImmediate(resolve));
  assert.equal(budget.busy, false);
});

test('uncertain native action cannot be replayed during or after takeover', async () => {
  const f = fixture(); let release;
  f.driver.execute = () => new Promise(resolve => {release = resolve;});
  const budget = createBudget({timeoutMs: 30});
  assert.equal((await run(f, {budget})).reason, 'deadline');
  assert.equal((await run(f, {budget})).reason, 'budget_in_use');
  release(); await new Promise(resolve => setImmediate(resolve));
  assert.equal((await run(f, {budget})).reason, 'reconciliation_required');
});

test('full local observations are retained verbatim but not given to Jev', async () => {
  const f = fixture({initial: frame() + '\n99 text PRIVATE_KEY'}); let request;
  const result = await run(f, {scenario: task({project: () => ({context: 'Navigation',
    candidates: [{index: 1, role: 'button', label: 'Agent', readOnly: true, raw: 'PRIVATE_KEY'}]})}),
  decide: async input => {request = input; return choice();}});
  assert.ok(!JSON.stringify(request).includes('PRIVATE_KEY'));
  assert.equal(result.observations[0].ax, frame() + '\n99 text PRIVATE_KEY');
  assert.equal(result.status, 'done');
});

test('batch NAV-02 reselects current semantic targets without any model calls', async () => {
  for (const shift of [0, 100]) {
    const home = `${shift + 1} heading 首页, Value: 1\n${shift + 2} button Agent`;
    const claude = `${shift + 9} heading Agent, Value: 1\n${shift + 10} heading Claude Code, Value: 2\n${shift + 17} button Codex logo Codex 已发现`;
    const f = fixture({initial: home, apply: ax => ax === home ? claude : '1 heading Agent, Value: 1\n2 heading Codex, Value: 2'});
    const r = await run(f, {decide: undefined, select: selectCodex, scenario: navCodex});
    assert.equal(r.status, 'done'); assert.equal(r.mode, 'batch'); assert.equal(r.callsUsed, 0);
    assert.deepEqual(f.executed.map(a => a.targetIndex), [shift + 2, shift + 17]);
    assert.deepEqual(r.events.map(e => e.target.label), ['Agent', 'Codex']);
    assert.ok(r.events.every(e => e.beforeCheck && e.afterCheck && e.execution === 'returned'));
    assert.ok(r.observations.some(o => o.phase === 'observe_after' && o.ax === claude));
  }
});

test('batch ambiguity or unexpected dialog stops without automatic Jev fallback', async () => {
  for (const suffix of ['\n3 button Agent', '\n3 dialog Confirm']) {
    const f = fixture({initial: '1 heading 首页, Value: 1\n2 button Agent' + suffix});
    const r = await run(f, {decide: undefined, select: selectCodex, scenario: navCodex});
    assert.equal(r.status, 'escalate'); assert.equal(r.callsUsed, 0); assert.equal(f.executed.length, 0);
  }
});

test('one explicit selector is required: neither implicit Jev nor dual fallback', async () => {
  for (const options of [{decide: undefined}, {select: selectCodex}, {select: 'invalid'}]) {
    const f = fixture();
    assert.equal((await run(f, options)).reason, 'invalid_contract');
    assert.equal(f.executed.length, 0);
  }
});

test('Jev refuses writes and unclassified controls before contacting the model', async () => {
  for (const scenario of [task({allowedActions: ['set_value']}),
    task({project: () => ({context: '', candidates: [{index: 1, role: 'button', label: 'Agent'}]})})]) {
    const f = fixture(); let calls = 0;
    const r = await run(f, {scenario, decide: async () => {calls++; return choice();}});
    assert.match(r.reason, /jev_read_only_required|read_only_scope_required/);
    assert.equal(calls, 0); assert.equal(f.executed.length, 0);
  }
});

test('Codex can refine Jev prompts across failed attempts without resetting budgets', async () => {
  const f = fixture(); const budget = createBudget({maxCalls: 3}); const prompts = [];
  const decide = async ({goal}) => {prompts.push(goal); return choice({confidence: goal.includes('navigation') ? 1 : 0.1});};
  const first = await run(f, {budget, decide, goal: 'Open Agent'});
  assert.equal(first.reason, 'low_confidence');
  assert.equal(first.events[0].execution, 'not_started');
  const second = await run(f, {budget, decide, goal: 'Open Agent using main navigation; do not change settings'});
  assert.equal(second.status, 'done'); assert.equal(second.callsUsed, 2); assert.equal(second.stepsUsed, 1);
  assert.equal(prompts.length, 2); assert.notEqual(first.goal, second.goal);
  assert.equal(first.callsUsed, 1); // Earlier evidence is not mutated by continuation.
});

test('batch continues after model call budget is spent but keeps guards and step budget', async () => {
  const f = fixture({apply: () => frame('Intermediate')});
  const budget = createBudget({maxCalls: 1, maxSteps: 1}); budget.callsUsed = 1;
  const r = await run(f, {budget, decide: undefined, select: () => ({action: 'click_element', targetIndex: 1})});
  assert.equal(r.reason, 'step_budget'); assert.equal(r.callsUsed, 1); assert.equal(r.stepsUsed, 1);
  const changed = fixture({apply: () => frame('Agent', 'changed')});
  const rejected = await run(changed, {decide: undefined, select: () => ({action: 'click_element', targetIndex: 1})});
  assert.equal(rejected.reason, 'after_action_rejected'); assert.equal(rejected.needsReconcile, true);
});

test('trace persistence retains complete local observations and failures in private files', async () => {
  const f = fixture({initial: frame() + '\n99 text FULL_LOCAL_VALUE'});
  const r = await run(f, {scenario: task({beforeAction: () => false})});
  const file = await saveTrace(r);
  assert.deepEqual(JSON.parse(await readFile(file, 'utf8')), r);
  assert.equal((await stat(file)).mode & 0o777, 0o600);
  assert.equal(r.events[0].beforeCheck, false); assert.equal(r.events[0].execution, 'not_started');
});

test('duplicate or fabricated candidate projection is rejected before API call', async () => {
  const f = fixture();
  for (const candidates of [[{index: 99, role: 'button', label: 'x'}],
    [{index: 1, role: 'button', label: 'x'}, {index: 1, role: 'button', label: 'y'}]]) {
    assert.equal((await run(f, {scenario: task({project: () => ({context: '', candidates})})})).reason, 'invalid_projection');
  }
});

test('normalization rejects null and non-finite probabilities', () => {
  const r = normalizeDecision({target: {choice: 'i1', confidence: null}, done: {noul: ''}, risk: {noul: 'NaN'}}, [{index: 1}]);
  assert.equal(r.confidence, null); assert.equal(r.done, null); assert.equal(r.risk, null);
});

test('native driver supports only indexed actions and observes full AX', async () => {
  let observationOptions; let clicked;
  const d = createCuaDriver({getAXState: async options => {observationOptions = options; return frame();}, click: async index => {clicked = index;}});
  await d.observe(); await d.execute({kind: 'click_element', targetIndex: 1});
  assert.deepEqual(observationOptions, {emit: false, disableDiffing: true});
  assert.equal(clicked, 1);
  assert.throws(() => d.execute({kind: 'click_at'}), /unsupported_action/);
});

test('real HTTP exchange exercises OpenRouter Decisions request and response normalization', async t => {
  let payload; let authorization; let endpoint;
  const server = http.createServer((req, res) => {
    let raw = ''; req.on('data', c => raw += c);
    req.on('end', () => {
      payload = JSON.parse(raw); authorization = req.headers.authorization;
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify({answers: {target: {choice: 'i1', confidence: 1}, action: {choice: 'click_element'}, done: {noul: 0}, risk: {noul: 0}}}));
    });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => {server.closeAllConnections(); server.close();});
  const result = await decide({goal: 'Open Agent', app: 'HiRoute', context: 'Home',
    candidates: [{index: 1, role: 'button', label: 'Agent'}], allowedActions: ['click_element']}, {
    apiKey: 'test-only-key', fetchImpl: (url, options) => {endpoint = url; return fetch(`http://127.0.0.1:${server.address().port}`, options);},
  });
  assert.equal(endpoint, 'https://openrouter.ai/api/alpha/decisions');
  assert.equal(authorization, 'Bearer test-only-key');
  assert.equal(result.targetIndex, 1);
  assert.equal(payload.model, 'typesafe/jev-1.13');
  assert.deepEqual(Object.keys(payload.questions.action.criteria), ['click_element']);
  assert.equal(payload.state.goal, 'Open Agent');
  assert.ok(!JSON.stringify(result).includes('test-only-key'));
});

test('Jev timeout also covers response body, not only headers', async () => {
  const input = {goal: 'Open', app: 'HiRoute', context: '', candidates: [{index: 1, role: 'button', label: 'A'}], allowedActions: ['click_element']};
  await assert.rejects(decide(input, {apiKey: 'test-only-key', timeoutMs: 10,
    fetchImpl: async () => ({ok: true, json: () => new Promise(() => {})}),
  }), /jev_cancelled_or_timeout/);
});

test('Jev HTTP errors omit provider body and do not retry', async () => {
  let calls = 0;
  await assert.rejects(decide({goal: 'Open', app: 'HiRoute', context: '', candidates: [], allowedActions: []}, {
    apiKey: 'test-only-key', fetchImpl: async () => {calls++; return {ok: false, status: 429, json: async () => ({error: 'PRIVATE_KEY'})};},
  }), /^Error: jev_http_429$/);
  assert.equal(calls, 1);
});
