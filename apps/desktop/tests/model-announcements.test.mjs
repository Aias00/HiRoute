import assert from 'node:assert/strict';
import test from 'node:test';
import { modelAnnouncements } from '../src/features/model-feed/announcements.ts';

test('unresolved announcements do not fall back to unrelated directory entries', () => {
  const result = modelAnnouncements({ featured: [], models: [{ model_configuration_id: 'unrelated' }] });
  assert.deepEqual(result, { fromFeed: false, entries: [] });
});

test('feed editorial selection replaces the bundled selection, including an unresolved entry', () => {
  const entry = { id: 'future', model_configuration_id: 'future-model' };
  assert.deepEqual(modelAnnouncements({ featured: [entry], models: [{ model_configuration_id: 'model.deepseek.v4-1-flash' }] }), { fromFeed: true, entries: [] });
  assert.deepEqual(modelAnnouncements({ featured: [entry], models: [{ model_configuration_id: 'future-model' }] }), { fromFeed: true, entries: [entry] });
});
