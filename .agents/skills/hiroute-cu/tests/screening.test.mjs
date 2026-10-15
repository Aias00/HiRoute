import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import {runTask, createBudget} from '../scripts/loop.mjs';
import {decide, buildQuestions} from '../scripts/jev.mjs';
import {review as navReview} from '../scripts/nav-codex.mjs';

const home = '1 button Agent\n2 heading 首页\n99 text PRIVATE_LOCAL';
const agent = '1 heading Agent\n2 text Opened';
const verdict = (kind = 'normal', evidence = 'none', confidence = 1) => ({kind, evidence, confidence});
const choice = assessment => ({targetIndex: 1, action: 'click_element', confidence: 1,
  risk: 0, done: 0, assessment: assessment ?? verdict()});
function fixture(overrides = {}) {
  let ax = home;
  const calls = [], actions = [];
  const scenario = {
    allowedActions: ['click_element'],
    project: () => ({context: 'Open Agent via navigation', candidates: [{index: 1, role: 'button', label: 'Agent', readOnly: true}]}),
    review: ({ax}) => ({expectation: 'Agent navigation should open Agent without errors.',
      facts: [ax === home ? 'Home page' : 'Agent page'], raw: 'PRIVATE_LOCAL'}),
    beforeAction: () => true, afterAction: () => true,
    verify: ({ax}) => ax === agent,
    ...overrides,
  };
  return {calls, actions, scenario, setAX: value => {ax = value;},
    driver: {observe: async () => ax, execute: async action => {actions.push(action); ax = agent;}},
    decide: async input => {calls.push(input); return choice();}};
}
const run = (f, options = {}) => runTask({driver: f.driver, appName: 'HiRoute', goal: 'Open Agent',
  scenario: f.scenario, decide: f.decide, budget: createBudget(), dryRun: false, ...options});

test('screening covers initial and terminal state with one selection call plus final assessment', async () => {
  const f = fixture();
  const r = await run(f, {budget: createBudget({maxSteps: 1, maxCalls: 2})});
  assert.equal(r.status, 'done'); assert.equal(r.verified, true);
  assert.equal(r.stepsUsed, 1); assert.equal(r.callsUsed, 2);
  assert.equal(f.calls[0].review.previous, null);
  assert.deepEqual(f.calls[1].review.previous, {facts: ['Home page'], action: {kind: 'click_element', target: 'Agent'}});
  assert.deepEqual(f.calls[1].candidates, []);
  assert.equal(r.assessments.length, 2);
  assert.ok(r.assessments.every(a => r.observations[a.observation].ax));
  assert.ok(!JSON.stringify(f.calls).includes('PRIVATE_LOCAL'));
  assert.ok(r.observations[0].ax.includes('PRIVATE_LOCAL'));
});

test('terminal visible problem overrides otherwise satisfied local goal', async () => {
  const f = fixture();
  const r = await run(f, {decide: async input => input.candidates.length ? choice()
    : {assessment: verdict('suspected_issue', 'f0')}});
  assert.equal(r.reason, 'assessment_suspected_issue'); assert.equal(r.verified, false);
  assert.equal(f.actions.length, 1);
  assert.equal(r.assessments.at(-1).evidenceText, 'Agent page');
});

test('initially at goal still screens without target selection or action', async () => {
  const f = fixture(); f.setAX(agent);
  const r = await run(f, {decide: async input => {
    assert.deepEqual(input.candidates, []); return {assessment: verdict()};
  }});
  assert.equal(r.status, 'done'); assert.equal(r.callsUsed, 1); assert.equal(r.stepsUsed, 0);
});

test('terminal API wait cannot turn changed UI into stale success', async () => {
  const f = fixture(); f.setAX(agent);
  const r = await run(f, {decide: async () => {f.setAX(home); return {assessment: verdict()};}});
  assert.equal(r.reason, 'observation_changed'); assert.equal(r.verified, false);
});

test('loading, unknown and suspected issues stop before independent next-action choice', async () => {
  for (const kind of ['loading', 'unknown', 'suspected_issue']) {
    const f = fixture();
    const r = await run(f, {decide: async () => choice(verdict(kind, kind === 'suspected_issue' ? 'f0' : 'none'))});
    assert.equal(r.reason, `assessment_${kind}`); assert.equal(f.actions.length, 0);
    assert.equal(r.assessments.length, 1);
  }
});

test('missing, fabricated, inconsistent and uncertain assessments fail closed', async () => {
  for (const [assessment, reason] of [
    [undefined, 'invalid_assessment'], [verdict('normal', 'f99'), 'invalid_assessment'],
    [verdict('normal', 'none', null), 'invalid_assessment'],
    [verdict('normal', 'none', 0.2), 'assessment_low_confidence'],
    [verdict('normal', 'f0'), 'assessment_inconsistent'],
    [verdict('suspected_issue', 'none'), 'assessment_inconsistent'],
  ]) {
    const f = fixture();
    const r = await run(f, {decide: async () => ({...choice(), assessment})});
    assert.equal(r.reason, reason); assert.equal(f.actions.length, 0);
  }
});

test('normal screening never bypasses deterministic after-action rejection', async () => {
  const f = fixture({afterAction: () => false});
  const r = await run(f);
  assert.equal(r.reason, 'after_action_rejected'); assert.equal(r.needsReconcile, true);
  assert.equal(r.verified, false); assert.equal(r.callsUsed, 1);
});

test('missing terminal call budget is incomplete, not successful', async () => {
  const f = fixture();
  const r = await run(f, {budget: createBudget({maxCalls: 1})});
  assert.equal(r.reason, 'call_budget'); assert.equal(r.verified, false); assert.equal(r.stepsUsed, 1);
});

test('step exhaustion and no-progress are assessed but cannot execute extra actions', async () => {
  const f = fixture({verify: () => false});
  const r = await run(f, {budget: createBudget({maxSteps: 1})});
  assert.equal(r.reason, 'step_budget'); assert.equal(r.callsUsed, 2); assert.equal(r.stepsUsed, 1);
  const same = fixture({verify: () => false});
  same.driver.execute = async action => same.actions.push(action);
  const stopped = await run(same);
  assert.equal(stopped.reason, 'no_progress'); assert.equal(stopped.callsUsed, 3); assert.equal(stopped.stepsUsed, 2);
});

test('empty action scope can report a visible issue without inventing navigation', async () => {
  const f = fixture({project: () => ({context: 'Error page', candidates: []})});
  const r = await run(f, {decide: async () => ({assessment: verdict('suspected_issue', 'f0')})});
  assert.equal(r.reason, 'assessment_suspected_issue'); assert.equal(f.actions.length, 0);
});

test('batch regression ignores optional screening and stays zero-model', async () => {
  const f = fixture({review: () => {throw Error('must not run');}});
  const r = await run(f, {decide: undefined, select: () => ({action: 'click_element', targetIndex: 1})});
  assert.equal(r.status, 'done'); assert.equal(r.callsUsed, 0); assert.deepEqual(r.assessments, []);
});

test('invalid or oversized review projection is rejected before network and actions', async () => {
  for (const review of [() => null, () => ({expectation: '', facts: ['x']}),
    () => ({expectation: 'x', facts: []}), () => ({expectation: 'x', facts: ['x'.repeat(501)]})]) {
    const f = fixture({review});
    assert.equal((await run(f)).reason, 'invalid_review_projection');
    assert.equal(f.calls.length, 0); assert.equal(f.actions.length, 0);
  }
});

test('terminal-only questions contain no empty action choices and handle expected errors explicitly', () => {
  const q = buildQuestions('Inspect', [], [], {facts: ['Expected invalid credential refusal']});
  assert.deepEqual(Object.keys(q), ['assessment', 'evidence']);
  assert.equal(q.evidence.criteria.f0, 'Expected invalid credential refusal');
  assert.ok(q.assessment.criteria.unknown);
});

test('NAV-02 review projects only known navigation and errors, not private values', () => {
  const r = navReview({ax: '1 heading 首页, Value: 1\n2 text 部分信息暂时无法读取\n3 text SECRET'});
  assert.deepEqual(r.facts, ['Current page: home.', '部分信息暂时无法读取']);
  assert.ok(!JSON.stringify(r).includes('SECRET'));
});

test('real HTTP transport carries review and normalizes evidence and usage through the loop', async t => {
  const requests = [];
  const server = http.createServer((req, res) => {
    let raw = ''; req.on('data', c => raw += c);
    req.on('end', () => {
      const payload = JSON.parse(raw); requests.push(payload);
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify({answers: {
        target: {choice: 'i1', confidence: 1}, action: {choice: 'click_element'},
        done: {noul: 0}, risk: {noul: 0}, assessment: {choice: 'normal', confidence: 1}, evidence: {choice: 'none'},
      }, usage: {input_tokens: 123, output_tokens: 8}}));
    });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => {server.closeAllConnections(); server.close();});
  const f = fixture();
  const r = await run(f, {decide: input => decide(input, {apiKey: 'test-key',
    fetchImpl: (_, options) => fetch(`http://127.0.0.1:${server.address().port}`, options)})});
  assert.equal(r.status, 'done'); assert.equal(requests.length, 2);
  assert.deepEqual(Object.keys(requests[1].questions), ['assessment', 'evidence']);
  assert.equal(requests[1].state.review.previous.action.target, 'Agent');
  assert.deepEqual(r.events.map(e => e.usage), [{input_tokens: 123, output_tokens: 8}, {input_tokens: 123, output_tokens: 8}]);
  assert.ok(!JSON.stringify(requests).includes('PRIVATE_LOCAL'));
});
