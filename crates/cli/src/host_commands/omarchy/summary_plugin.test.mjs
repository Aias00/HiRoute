// The Omarchy indicator reader and catalogs, tested against the producer's shared fixture.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const plugin = join(here, '../../../../../assets/integrations/omarchy/hiroute.status');
const source = readFileSync(join(plugin, 'Summary.js'), 'utf8');
assert.ok(source.startsWith('.pragma library\n'), 'Summary.js must stay a stateless QML library');
const Summary = vm.runInNewContext(
  `${source.slice('.pragma library\n'.length)}\n({ parse, bar, trust, slotKeys, MAX_BYTES })`,
);
const fixture = JSON.parse(readFileSync(join(here, 'summary.fixture.json'), 'utf8'));
const catalogs = Object.fromEntries(['en', 'zh'].map((code) => [code, JSON.parse(readFileSync(join(plugin, `i18n/${code}.json`), 'utf8'))]));
const qml = readFileSync(join(plugin, 'Panel.qml'), 'utf8');

const clone = (value) => JSON.parse(JSON.stringify(value));
const envelope = (data) => JSON.stringify({ schema_version: { major: 2, minor: 0 }, status: 'succeeded', data, warnings: [], next_actions: [] });
const parse = (data) => Summary.parse(envelope(data));
const unavailable = () => ({
  schema: 'hiroute.omarchy-summary/v1', generated_at_ms: fixture.generated_at_ms,
  service: { state: 'unavailable', reason_code: 'service_unavailable' },
  agents: [], suggestions: null, latest_routed_session: null, token_share_today: null, notifications: false,
});

test('accepts the producer fixture and the unavailable summary', () => {
  assert.equal(parse(fixture).agents[1].slot_plans[2].trust_state, 'branch_specific');
  assert.equal(parse(unavailable()).service.reason_code, 'service_unavailable');
});

test('rejects output that is not one current, bounded summary', () => {
  assert.equal(Summary.parse('not json'), null);
  assert.equal(Summary.parse(''), null);
  assert.equal(Summary.parse(undefined), null);
  assert.equal(Summary.parse(JSON.stringify({ status: 'failed', schema_version: { major: 2, minor: 0 } })), null);
  assert.equal(Summary.parse(JSON.stringify({ ...JSON.parse(envelope(fixture)), schema_version: { major: 3, minor: 0 } })), null);
  const padded = clone(fixture);
  padded.agents[0].slot_plans[0].plan_name = 'x'.repeat(80);
  const oversized = envelope(padded).replace('"warnings":[]', `"warnings":[],"padding":"${'x'.repeat(Summary.MAX_BYTES)}"`);
  assert.equal(Summary.parse(oversized), null);
  // Multi-byte text counts in UTF-8 bytes, not UTF-16 units.
  assert.equal(Summary.parse(envelope(fixture).replace('"warnings":[]', `"warnings":[],"x":"${'名'.repeat(22000)}"`)), null);
});

const invalid = {
  'unknown schema': (s) => { s.schema = 'hiroute.omarchy-summary'; },
  'unknown field': (s) => { s.extra = true; },
  'missing notifications': (s) => { delete s.notifications; },
  'ready with a reason': (s) => { s.service.reason_code = 'service_unavailable'; },
  'ready without suggestions': (s) => { s.suggestions = null; },
  'ready without a headline': (s) => { s.token_share_today = null; },
  'unknown agent': (s) => { s.agents[0].agent = 'hermes'; },
  'duplicate agent': (s) => { s.agents[1] = clone(s.agents[0]); },
  'launchable with a reason': (s) => { s.agents[0].check_reason_code = 'native_login_required'; },
  'failure without a reason': (s) => { s.agents[1].check_reason_code = null; },
  'malformed reason': (s) => { s.agents[1].check_reason_code = 'Not Ready'; },
  'codex Claude slot': (s) => { s.agents[0].slot_plans[0].slots = ['opus']; },
  'too many slot Plans': (s) => { s.agents[0].slot_plans.push({ ...clone(s.agents[0].slot_plans[0]), plan_id: 'plan-z' }); },
  'empty slots': (s) => { s.agents[1].slot_plans[0].slots = []; },
  'repeated slot': (s) => { s.agents[1].slot_plans[1].slots = ['opus']; },
  'slot order': (s) => { const p = s.agents[1].slot_plans; [p[0], p[1]] = [p[1], p[0]]; },
  'long Plan name': (s) => { s.agents[0].slot_plans[0].plan_name = 'x'.repeat(81); },
  'control in Plan name': (s) => { s.agents[0].slot_plans[0].plan_name = 'a\u0007b'; },
  'invalid Plan id': (s) => { s.agents[0].slot_plans[0].plan_id = '../plan'; },
  'negative other count': (s) => { s.agents[0].other_mapped_plans = -1; },
  'uniform without values': (s) => { Object.assign(s.agents[0].slot_plans[0], { trust_level: null, threshold_millis: null }); },
  'uniform without threshold': (s) => { s.agents[0].slot_plans[0].threshold_millis = null; },
  'preset with another threshold': (s) => { s.agents[0].slot_plans[0].trust_level = 'trial'; },
  'custom at a preset threshold': (s) => { s.agents[0].slot_plans[0].trust_level = 'custom'; },
  'threshold out of range': (s) => { Object.assign(s.agents[0].slot_plans[0], { trust_level: 'custom', threshold_millis: 1001 }); },
  'not applicable with a level': (s) => { s.agents[1].slot_plans[1].trust_level = 'trusted'; },
  'branch specific with a threshold': (s) => { s.agents[1].slot_plans[2].threshold_millis = 900; },
  'unknown trust state': (s) => { s.agents[1].slot_plans[2].trust_state = 'mixed'; },
  'missing trust state': (s) => { delete s.agents[1].slot_plans[2].trust_state; },
  'ids beyond the count': (s) => { s.suggestions.ids = ['s-1']; },
  'more than twenty ids': (s) => { s.suggestions.count = 21; s.suggestions.ids = Array.from({ length: 21 }, (_, n) => `s-${n}`); },
  'duplicate ids': (s) => { s.suggestions.ids = ['s-1', 's-1']; },
  'invalid id': (s) => { s.suggestions.ids[0] = 'bad id'; },
  'fractional unavailable count': (s) => { s.suggestions.unavailable_plans = 0.5; },
  'session for an absent agent': (s) => { s.agents.pop(); s.latest_routed_session.agent = 'claude-code'; },
  'session older than the window': (s) => { s.latest_routed_session.last_activity_ms = s.generated_at_ms - 7 * 86400000 - 1; },
  'session from the future': (s) => { s.latest_routed_session.last_activity_ms = s.generated_at_ms + 1; },
  'session id that is an option': (s) => { s.latest_routed_session.session_id = '--follow'; },
  'unknown headline': (s) => { s.token_share_today = { kind: 'estimated' }; },
  'share above one': (s) => { s.token_share_today.outside_subscription_ratio_millis = 1001; },
  'headline extra field': (s) => { s.token_share_today = { kind: 'no_routed_requests', ratio: 1 }; },
  'unavailable with figures': (s) => { Object.assign(s, unavailable(), { token_share_today: { kind: 'no_routed_requests' } }); },
  'unavailable without a reason': (s) => { Object.assign(s, unavailable()); s.service.reason_code = null; },
};
for (const [name, change] of Object.entries(invalid)) {
  test(`rejects ${name}`, () => {
    const value = clone(fixture);
    change(value);
    assert.equal(parse(value), null);
  });
}

test('every token headline is accepted and presented without stale figures', () => {
  const headlines = [
    [{ kind: 'no_routed_requests' }, 'headline.no_routed_requests'],
    [{ kind: 'no_subscription_in_period' }, 'headline.no_subscription_in_period'],
    [{ kind: 'subscription_usage_unknown', unknown_usage_requests: 3 }, 'headline.subscription_usage_unknown'],
    [{ kind: 'no_comparable_usage' }, 'headline.no_comparable_usage'],
  ];
  for (const [headline, key] of headlines) {
    const value = clone(fixture);
    value.token_share_today = headline;
    const state = Summary.bar(parse(value));
    assert.equal(state.percent, -1);
    assert.equal(state.tooltipKey, key);
  }
  const share = Summary.bar(parse(fixture));
  assert.equal(share.percent, 42.5);
  assert.equal(share.decimals, 1);
  assert.equal(share.tooltipKey, 'bar.share');
  assert.equal(share.unknown, true);
  assert.equal(share.agentWarning, true);
  assert.equal(share.dot, true);
  const whole = clone(fixture);
  whole.token_share_today.outside_subscription_ratio_millis = 420;
  assert.equal(Summary.bar(parse(whole)).decimals, 0);
  const missing = Summary.bar(null);
  assert.equal(missing.percent, -1);
  assert.equal(missing.warning, true);
  assert.equal(missing.dot, false);
});

test('trust presentation follows the state and never invents a threshold', () => {
  const [uniform, none, branches] = parse(fixture).agents[1].slot_plans;
  assert.deepEqual({ ...Summary.trust(uniform) }, { key: 'trust.level.trusted', threshold: 650 });
  assert.deepEqual({ ...Summary.trust(none) }, { key: 'trust.not_applicable', threshold: -1 });
  assert.deepEqual({ ...Summary.trust(branches) }, { key: 'trust.branch_specific', threshold: -1 });
  assert.deepEqual([...Summary.slotKeys(uniform)], ['slot.opus']);
});

test('both catalogs translate every key the panel can show', () => {
  assert.deepEqual(Object.keys(catalogs.zh).sort(), Object.keys(catalogs.en).sort());
  const literal = [...qml.matchAll(/strings\.tn?\("([^"]+)"\s*[,)]/g)].map((m) => m[1]);
  assert.ok(literal.length > 15);
  const dynamic = [
    'headline.no_routed_requests', 'headline.no_subscription_in_period', 'headline.subscription_usage_unknown',
    'headline.no_comparable_usage', 'reason.service_unavailable', 'reason.summary_read_unavailable',
    'agent.codex', 'agent.claude-code', 'slot.default', 'slot.opus', 'slot.sonnet', 'slot.haiku',
    'trust.level.trial', 'trust.level.familiar', 'trust.level.trusted', 'trust.level.hands_off', 'trust.level.custom',
    'trust.not_applicable', 'trust.branch_specific',
  ];
  for (const key of [...literal, ...dynamic]) {
    for (const [code, catalog] of Object.entries(catalogs)) assert.ok(key in catalog, `${code} lacks ${key}`);
  }
  for (const catalog of Object.values(catalogs)) {
    for (const [key, value] of Object.entries(catalog)) {
      const forms = typeof value === 'string' ? [value] : Object.values(value);
      assert.ok(forms.length > 0 && forms.every((f) => typeof f === 'string' && f.length > 0), key);
      if (typeof value === 'object') assert.ok('other' in value, `${key} needs an other form`);
    }
  }
  assert.equal(catalogs.en['trust.not_applicable'], 'No trust level');
  assert.equal(catalogs.zh['trust.not_applicable'], '无信任档位');
  assert.equal(catalogs.en['trust.branch_specific'], 'Configured per branch');
  assert.equal(catalogs.zh['trust.branch_specific'], '按分支设置');
  assert.equal(catalogs.zh['suggestions.unavailable'].other, '%1 个 Plan 的建议暂不可用');
});

test('the panel renders text as plain text and starts processes only as argv', () => {
  const texts = [...qml.matchAll(/\bText \{([\s\S]*?)\n\s*\}/g)].map((m) => m[1]);
  assert.ok(texts.length > 0);
  for (const body of texts) assert.match(body, /textFormat: Text\.PlainText/);
  assert.doesNotMatch(qml, /bar\.run\(|"sh"|"bash"|"-c"|Qt\.openUrlExternally|XMLHttpRequest/);
  for (const m of qml.matchAll(/command:\s*([^\n]+)/g)) assert.match(m[1], /^\[root\.cli,/);
  assert.match(qml, /interval: 5000/);
  assert.match(qml, /interval: 60000/);
  assert.match(qml, /Summary\.MAX_BYTES/);
  const manifest = JSON.parse(readFileSync(join(plugin, 'manifest.json'), 'utf8'));
  assert.equal(manifest.id, 'hiroute.status');
  assert.deepEqual(manifest.kinds, ['bar-widget']);
  assert.equal(manifest.entryPoints.barWidget, 'Panel.qml');
});
