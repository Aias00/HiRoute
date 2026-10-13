import assert from 'node:assert/strict';
import test from 'node:test';
import { addDraftKey, buildCredentialEdits, createDraft, moveDraftKey } from '../src/features/models/model-draft.ts';

const source = (ids = ['A']) => ({ source_id: 'source/test', state: 'ready', models: [],
  keys: ids.map((key_id, ordinal) => ({ key_id, ordinal, enabled: true, generation: 3, fingerprint_hint: key_id })) });
const candidate = { candidate_ref: 'input/B', candidate_revision: 1 };
const prepare = async () => ({ key_id: 'B', input_candidate: candidate });

test('adding B then moving it before A saves complete membership in one change', async () => {
  const saved = source();
  let draft = addDraftKey({ ...createDraft(saved), addedSecret: 'test-key-B' });
  draft = moveDraftKey(draft, draft.keys[1].draftId, -1);
  assert.deepEqual(await buildCredentialEdits(saved, draft, prepare), [
    { action: 'add', input_candidate: candidate }, { action: 'set_order', key_ids: ['B', 'A'] },
  ]);
  assert.deepEqual(saved.keys.map(key => key.key_id), ['A']);
});

test('removal and reorder submit only the surviving complete set', async () => {
  const saved = source(['A', 'B', 'C']);
  let draft = createDraft(saved);
  draft.keys[1].removed = true;
  draft = moveDraftKey(moveDraftKey(draft, 'C', -1), 'C', -1);
  assert.deepEqual(await buildCredentialEdits(saved, draft, prepare), [
    { action: 'remove', key_id: 'B', expected_generation: 3 },
    { action: 'set_order', key_ids: ['C', 'A'] },
  ]);
});

test('replacement retains identity and new disabled Key uses its first generation', async () => {
  const saved = source();
  let draft = addDraftKey({ ...createDraft(saved), addedSecret: 'test-key-B' });
  draft.keys[0].replacement = 'replacement-A';
  draft.keys[0].enabled = false;
  draft.keys[1].enabled = false;
  const edits = await buildCredentialEdits(saved, draft, prepare);
  assert.equal(edits[0].key_id, 'A');
  assert.deepEqual(edits.filter(edit => edit.action === 'set_enabled'), [
    { action: 'set_enabled', key_id: 'A', expected_generation: 3, enabled: false },
    { action: 'set_enabled', key_id: 'B', expected_generation: 1, enabled: false },
  ]);
  assert.deepEqual(edits.at(-1), { action: 'set_order', key_ids: ['A', 'B'] });
});
