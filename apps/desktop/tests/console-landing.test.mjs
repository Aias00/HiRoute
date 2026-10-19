import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validConsoleLanding, acceptConsoleLanding, takeConsoleLanding } from '../src/host/console-landing.ts';
test('console landing is a bounded route hint, consumed only once after authenticated exchange', () => {
  for (const value of ['first-run', 'agent:codex', 'agent:claude-code', 'suggestion:trust-abc']) assert.equal(validConsoleLanding(value), true);
  for (const value of [null, '', 'https://example.invalid', 'agent:other', 'suggestion:', 'suggestion:../x', 'suggestion:x&code=x', `suggestion:${'x'.repeat(129)}`]) assert.equal(validConsoleLanding(value), false);
  acceptConsoleLanding('first-run'); assert.equal(takeConsoleLanding(), 'first-run'); assert.equal(takeConsoleLanding(), null);
  acceptConsoleLanding('agent:codex'); acceptConsoleLanding(null); assert.equal(takeConsoleLanding(), null);
});
