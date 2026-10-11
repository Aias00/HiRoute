import assert from 'node:assert/strict';
import { afterEach, test } from 'node:test';
import { requestSubscriptionProxyReturn, clearSubscriptionProxyReturn, takeSubscriptionProxyReturn } from '../src/features/subscription-proxy-return.ts';

function storage() {
  const values = new Map();
  globalThis.window = { localStorage: {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: key => values.delete(key),
  } };
  return values;
}
afterEach(() => { delete globalThis.window; });

test('the restart hint is consumed once and stores only a timestamp', () => {
  const values = storage();
  requestSubscriptionProxyReturn();
  assert.equal(values.size, 1);
  assert.match([...values.values()][0], /^\d+$/);
  assert.equal(takeSubscriptionProxyReturn(), true);
  assert.equal(takeSubscriptionProxyReturn(), false);
  requestSubscriptionProxyReturn(); clearSubscriptionProxyReturn();
  assert.equal(takeSubscriptionProxyReturn(), false);
});

test('damaged, expired and future hints do not reopen a scan later', () => {
  const values = storage();
  for (const value of ['broken', String(Date.now() - 300_001), String(Date.now() + 60_000)]) {
    values.set('hiroute.subscription-proxy-return', value);
    assert.equal(takeSubscriptionProxyReturn(), false);
    assert.equal(values.size, 0);
  }
});

test('unavailable WebView storage leaves ordinary startup usable', () => {
  globalThis.window = { get localStorage() { throw new Error('denied'); } };
  assert.equal(takeSubscriptionProxyReturn(), false);
  assert.doesNotThrow(clearSubscriptionProxyReturn);
  assert.throws(requestSubscriptionProxyReturn);
});
