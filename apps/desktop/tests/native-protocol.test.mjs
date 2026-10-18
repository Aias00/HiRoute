import { test } from 'node:test';
import assert from 'node:assert/strict';
import stock from '../../../crates/domain/src/routing/fixtures/system_one_c_v1.json' with { type: 'json' };
import { defaultJudgment } from '../src/features/decision-services/types.ts';
import { stockAssessment, resolveAssessment, assessmentStandard } from '../src/features/decision-services/native-protocol.ts';

test('native default resolves released assessment text without mutating the old input or thresholds', () => {
  const original = structuredClone(defaultJudgment);
  original.degree.simple_threshold_millis = 819;
  original.competence.floor_millis = 317;
  const before = structuredClone(original), current = resolveAssessment(original, true);
  assert.equal(assessmentStandard(current), 'default');
  assert.deepEqual(current.competence.criteria, stock.q1.criteria);
  assert.ok(stock.q1.instructions.endsWith(' ' + current.competence.instructions));
  assert.equal(current.degree.simple_threshold_millis, 819);
  assert.equal(current.competence.floor_millis, 317);
  assert.deepEqual(original, before);
  assert.deepEqual(resolveAssessment(current, true), current);
});

test('native editing preserves custom bytes, explicit custom equality and criteria-only edits', () => {
  for (const equal of [true, false]) {
    const custom = structuredClone(defaultJudgment); custom.competence.standard = 'custom';
    if (!equal) { custom.competence.instructions = '  line one.\nline two.  '; custom.competence.criteria[1] += '  '; }
    assert.deepEqual(resolveAssessment(custom, true), custom);
  }
  const criteriaOnly = structuredClone(defaultJudgment); criteriaOnly.competence.criteria[0] += ' ';
  assert.equal(assessmentStandard(criteriaOnly), 'custom');
  assert.equal(resolveAssessment(criteriaOnly, true).competence.criteria[0], criteriaOnly.competence.criteria[0]);
});

test('reset restores the native default; custom extensions retain their existing default', () => {
  const custom = structuredClone(defaultJudgment); custom.competence.instructions = 'custom'; custom.competence.standard = 'custom';
  const current = stockAssessment(custom, true);
  assert.equal(current.competence.standard, 'default');
  assert.deepEqual(current.competence.criteria, stock.q1.criteria);
  assert.deepEqual(stockAssessment(custom).competence.criteria, defaultJudgment.competence.criteria);
});
