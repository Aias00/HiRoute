import { test } from 'node:test';
import assert from 'node:assert/strict';
import stock from '../../../crates/domain/src/routing/fixtures/system_one_c_v1.json' with { type: 'json' };
import { defaultJudgment } from '../src/features/decision-services/types.ts';
import { nativeProtocol, stockAssessment, composedAssessment, contentDigest, freezeNative, stockProbe, nativeReferenceIssue, resolveAssessment, assessmentStandard } from '../src/features/decision-services/native-protocol.ts';

test('published C_full fixture and desktop composition preserve the reviewed content and thresholds', async () => {
  assert.equal(await contentDigest(stock), 'sha256:4dbab2f53954eb2176b6aa8d31b97808c4fee15ebb2fae337012eba348aef3c7');
  const protocol = nativeProtocol();
  const original = structuredClone(defaultJudgment);
  original.degree.simple_threshold_millis = 819;
  original.competence.floor_millis = 317;
  const judgment = stockAssessment(original, protocol);
  assert.deepEqual(composedAssessment(protocol, judgment), stock.q1);
  assert.equal(judgment.degree.simple_threshold_millis, 819);
  assert.equal(judgment.competence.floor_millis, 317);
  assert.deepEqual(original.competence.criteria, defaultJudgment.competence.criteria);
  assert.notDeepEqual(composedAssessment(undefined, judgment), stock.q1);
});

test('content identity changes on effective text, thresholds and history limits', async () => {
  const protocol = nativeProtocol(), judgment = stockAssessment(defaultJudgment, protocol);
  const frozen = await freezeNative(protocol, judgment);
  assert.equal(frozen.scopes.smart_saving.standard, 'default');
  assert.equal(frozen.digest, await contentDigest([frozen.protocol, frozen.context_layout, frozen.decision, frozen.scopes]));
  for (const change of ['text', 'threshold', 'history']) {
    const p = structuredClone(protocol), j = structuredClone(judgment);
    if (change === 'text') { j.competence.standard = 'custom'; j.competence.instructions += ' Custom text.'; }
    if (change === 'threshold') j.degree.simple_threshold_millis++;
    if (change === 'history') p.prior_context_max_turns = 0;
    assert.notEqual((await freezeNative(p, j)).digest, frozen.digest, change);
  }
});

test('explicit saved-service probes bind the standard and maximum branch shapes', async () => {
  const service = { id: 'saved-fixture', revision: 7 };
  const standard = await stockProbe(service, 'standard');
  const maximum = await stockProbe(service, 'max_branches');
  assert.equal(standard.service_revision, 7);
  assert.equal(standard.protocol.decision.kind, 'ordinal');
  assert.equal(maximum.protocol.decision.options.length, 16);
  assert.equal(maximum.protocol.decision.options.filter(o => o.refinement).length + 3, 19);
  assert.equal(Object.keys(maximum.protocol.scopes).length, 16);
  assert.ok(Object.values(maximum.protocol.scopes).every(s => s.standard === 'default')); 
  assert.notEqual(standard.protocol.digest, maximum.protocol.digest);
  assert.equal(nativeReferenceIssue(true, 'Use state.visible_conversation'), true);
  assert.equal(nativeReferenceIssue(false, 'Use state.assessment_stage'), true);
});

test('native category v1 composition is pinned independently of current defaults', async () => {
  const { default: categoryV1 } = await import('../../../crates/domain/src/routing/fixtures/native_category_v1.json', { with: { type: 'json' } });
  const p = await stockProbe({ id: 'fixture', revision: 1 }, 'max_branches');
  assert.equal(p.protocol.decision.instructions, categoryV1.category_prefix + 'probe-1' + categoryV1.category_suffix);
  const option = p.protocol.decision.options[0];
  assert.ok(option.refinement.instructions.startsWith(categoryV1.degree_prefix));
  assert.ok(option.refinement.instructions.includes(categoryV1.degree_suffix));
});


test('default intent follows toggles while custom bytes and explicit custom equality survive', () => {
  const original = structuredClone(defaultJudgment), p = nativeProtocol();
  const enabled = resolveAssessment(original, p);
  assert.equal(assessmentStandard(enabled), 'default');
  const disabled = resolveAssessment(enabled);
  assert.equal(disabled.competence.instructions, original.competence.instructions);
  assert.deepEqual(disabled.competence.criteria, original.competence.criteria);
  for (const equal of [true, false]) {
    const custom = structuredClone(original); custom.competence.standard = 'custom';
    if (!equal) { custom.competence.instructions = '  line one.\nline two.  '; custom.competence.criteria[1] += '  '; }
    assert.deepEqual(resolveAssessment(resolveAssessment(custom,p)), custom);
  }
  const criteriaOnly = structuredClone(original); criteriaOnly.competence.criteria[0]+=' ';
  assert.equal(assessmentStandard(criteriaOnly), 'custom');
  assert.deepEqual(Object.keys(nativeProtocol()).sort(), ['prior_context_max_turns', 'schema']);
});
