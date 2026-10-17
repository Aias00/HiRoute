import assert from 'node:assert/strict';
import { generateKeyPairSync, sign } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { checkFeed, checkPayload, FEED_URL, MAX_ENVELOPE_BYTES, payloadDigest, publishFeed, signEnvelope, trustedKeys, verifyEnvelope } from '../scripts/model-feed.mjs';

const repository = path.resolve(import.meta.dirname, '../../..');
const script = path.join(repository, 'apps/website/scripts/model-feed.mjs');
const pair = generateKeyPairSync('ed25519');
const otherPair = generateKeyPairSync('ed25519');
const publicEntry = (key, id = 'test-only') => ({ key_id: id,
  ed25519: key.export({ type: 'spki', format: 'der' }).subarray(-32).toString('base64') });
const publicDocument = { keys: [publicEntry(pair.publicKey)] };
const keys = trustedKeys(publicDocument);
const pem = key => key.export({ type: 'pkcs8', format: 'pem' });
const secret = JSON.stringify({ 'test-only': pem(pair.privateKey) });
const production = { GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/heads/main', MODEL_FEED_SIGNING_KEYS: secret };
const payload = (sequence = 2, extra = {}) => ({ schema: 'hiroute.model-feed/v1', sequence,
  issued_at: '2026-10-18T08:00:00Z', source_revision: 'a'.repeat(40), records: [], featured: [], ...extra });
const json = value => Buffer.from(`${JSON.stringify(value)}\n`);
const envelope = value => signEnvelope(json(value), secret, keys);
const response = bytes => new Response(bytes, { status: 200 });
const firstPublication = async () => new Response(null, { status: 404 });
const noRequest = async () => assert.fail('This path must not make a network request');

async function fixture(t, value = payload()) {
  const root = await mkdtemp(path.join(os.tmpdir(), 'hiroute-feed-test-'));
  t.after(() => rm(root, { force: true, recursive: true }));
  const sourceDirectory = path.join(root, 'source');
  const distDirectory = path.join(root, 'dist');
  await mkdir(sourceDirectory);
  await mkdir(distDirectory);
  await writeFile(path.join(distDirectory, 'index.html'), '<h1>Website</h1>');
  for (const [name, content] of Object.entries({ 'payload.json': value, 'public-keys.json': publicDocument,
    'policy.json': { minimum_feed_client: '0.3.2' },
    'publication.json': { sequence: value?.sequence ?? 1, issued_at: value?.issued_at ?? '2026-10-18T08:00:00Z' },
    'featured.json': value?.featured ?? [] })) await writeFile(path.join(sourceDirectory, name), json(content));
  return { sourceDirectory, distDirectory, destination: path.join(distDirectory, 'model-feed/v1.json'),
    summary: path.join(root, 'summary.md'), env: production };
}

async function noOutput(f) {
  await assert.rejects(readFile(f.destination), { code: 'ENOENT' });
  assert.equal(await readFile(path.join(f.distDirectory, 'index.html'), 'utf8'), '<h1>Website</h1>');
}

test('bootstrap works without keys, secrets or network and leaves the ordinary website deployable', async t => {
  const f = await fixture(t, null);
  await writeFile(path.join(f.sourceDirectory, 'public-keys.json'), json({ keys: [] }));
  const checked = await checkFeed(f);
  assert.equal(checked.payload, null);
  const result = await publishFeed({ ...f, env: { ...production, MODEL_FEED_SIGNING_KEYS: undefined }, fetchImpl: noRequest });
  assert.deepEqual(result, { status: 'bootstrap', sequence: null, payload_digest: null, source_revision: null });
  await noOutput(f);
});

test('PR checking requires no secrets; publishing rejects PR and non-main contexts before fetch', async t => {
  const f = await fixture(t);
  assert.equal((await checkFeed(f)).payload.sequence, 2n);
  for (const env of [{ GITHUB_EVENT_NAME: 'pull_request', GITHUB_REF: 'refs/pull/1/merge' },
    { GITHUB_EVENT_NAME: 'pull_request_target', GITHUB_REF: 'refs/heads/main' },
    { GITHUB_EVENT_NAME: 'workflow_dispatch', GITHUB_REF: 'refs/heads/feature' }, {}]) {
    await assert.rejects(publishFeed({ ...f, env, fetchImpl: noRequest }), /only/);
    await noOutput(f);
  }
  const cli = spawnSync(process.execPath, [script, 'publish'], { encoding: 'utf8',
    env: { ...process.env, GITHUB_EVENT_NAME: 'pull_request', MODEL_FEED_SIGNING_KEYS: '' } });
  assert.equal(cli.status, 1);
  assert.match(cli.stderr, /only a production/);
});

test('first 404 signs exact payload bytes, verifies them and reports publication provenance', async t => {
  const f = await fixture(t);
  let calls = 0;
  const result = await publishFeed({ ...f, env: { ...production, GITHUB_STEP_SUMMARY: f.summary },
    fetchImpl: async (url, options) => {
      calls += 1;
      assert.equal(url, FEED_URL);
      assert.equal(options.redirect, 'error');
      assert.equal(options.credentials, 'omit');
      assert.equal(options.cache, 'no-store');
      assert.equal(options.headers['Cache-Control'], 'no-cache');
      assert.deepEqual(Object.keys(options.headers).sort(), ['Accept', 'Cache-Control']);
      return firstPublication();
    } });
  assert.equal(calls, 1);
  assert.equal(result.status, 'first_publication');
  const bytes = await readFile(f.destination);
  const verified = verifyEnvelope(bytes, keys);
  assert.equal(verified.payload.sequence, 2n);
  assert.equal(verified.digest, payloadDigest(json(payload())));
  assert.equal(Buffer.from(JSON.parse(bytes).payload, 'base64').compare(json(payload())), 0);
  const summary = await readFile(f.summary, 'utf8');
  assert.match(summary, /Sequence: 2/);
  assert.ok(summary.includes(result.payload_digest));
  assert.ok(summary.includes('a'.repeat(40)));
  assert.doesNotMatch(summary, /PRIVATE KEY/);
});

test('a lower served sequence accepts a newer publication', async t => {
  const f = await fixture(t);
  const result = await publishFeed({ ...f, fetchImpl: async () => response(envelope(payload(1))) });
  assert.equal(result.status, 'published');
  assert.equal(verifyEnvelope(await readFile(f.destination), keys).payload.sequence, 2n);
});

test('matching payload digest is a no-op without a signing secret and does not rewrite the served feed', async t => {
  const f = await fixture(t);
  // Noncanonical envelope formatting is legitimate: preserve the served object itself.
  const served = Buffer.from(JSON.stringify(JSON.parse(envelope(payload())), null, 2));
  const before = Buffer.from(served);
  const result = await publishFeed({ ...f, env: { ...production, MODEL_FEED_SIGNING_KEYS: undefined },
    fetchImpl: async () => response(served) });
  assert.equal(result.status, 'already_current');
  assert.deepEqual(served, before);
  await noOutput(f);
});

test('older proposed sequence and equal sequence with different content never write an output', async t => {
  for (const served of [payload(3), payload(2, { source_revision: 'b'.repeat(40) })]) {
    const f = await fixture(t);
    await assert.rejects(publishFeed({ ...f, fetchImpl: async () => response(envelope(served)) }), /sequence must exceed/);
    await noOutput(f);
  }
});

test('signature checking precedes parsing the served payload', () => {
  const invalidJson = Buffer.from('this is not JSON');
  const bytes = json({ schema: 'hiroute.model-feed.envelope/v1', payload: invalidJson.toString('base64'),
    signatures: [{ key_id: 'test-only', ed25519: Buffer.alloc(64).toString('base64') }] });
  assert.throws(() => verifyEnvelope(bytes, keys), /no trusted valid signature/);
});

test('tampered and unknown-key served envelopes fail before signing', async t => {
  const tampered = JSON.parse(envelope(payload(1)));
  tampered.payload = json(payload(1, { source_revision: 'c'.repeat(40) })).toString('base64');
  const unknown = JSON.parse(envelope(payload(1)));
  unknown.signatures[0].key_id = 'not-trusted';
  for (const served of [tampered, unknown]) {
    const f = await fixture(t);
    await assert.rejects(publishFeed({ ...f, fetchImpl: async () => response(json(served)) }), /no trusted valid signature/);
    await noOutput(f);
  }
});

test('only initial 404 permits first publication; other HTTP, redirect, and network failures stop it', async t => {
  for (const fetchImpl of [async () => new Response(null, { status: 403 }),
    async () => new Response(null, { status: 500 }), async () => new Response(null, { status: 304 }),
    async () => new Response(null, { status: 302, headers: { Location: 'https://example.com/feed' } }),
    async () => { throw new Error('network offline'); }]) {
    const f = await fixture(t);
    await assert.rejects(publishFeed({ ...f, fetchImpl }), /served feed:/);
    await noOutput(f);
  }
});

test('signing absence, invalid material, unknown ID and mismatched keys leave no output or secret in errors', async t => {
  const marker = 'PRIVATE-SECRET-MARKER';
  for (const material of [undefined, marker, '{}', JSON.stringify({ 'test-only': marker }),
    JSON.stringify({ unknown: pem(pair.privateKey) }), JSON.stringify({ 'test-only': pem(otherPair.privateKey) })]) {
    const f = await fixture(t);
    await assert.rejects(publishFeed({ ...f, env: { ...production, MODEL_FEED_SIGNING_KEYS: material }, fetchImpl: firstPublication }), error => {
      assert.match(error.message, /signing:/);
      assert.ok(!error.message.includes(marker));
      assert.ok(!error.message.includes('BEGIN PRIVATE KEY'));
      return true;
    });
    await noOutput(f);
  }
});

test('signing failure exits nonzero in a process before any dist feed file exists', async t => {
  const f = await fixture(t);
  const options = { sourceDirectory: f.sourceDirectory, distDirectory: f.distDirectory,
    env: { ...production, MODEL_FEED_SIGNING_KEYS: 'SECRET-MUST-NOT-LEAK' } };
  const program = `import { publishFeed } from ${JSON.stringify(new URL('../scripts/model-feed.mjs', import.meta.url).href)};
    publishFeed({ ...${JSON.stringify(options)}, fetchImpl: async () => new Response(null, { status: 404 }) })
      .catch(error => { console.error(error.message); process.exitCode = 1; });`;
  const result = spawnSync(process.execPath, ['--input-type=module', '--eval', program], { encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /signing:/);
  assert.doesNotMatch(result.stdout + result.stderr, /SECRET-MUST-NOT-LEAK/);
  await noOutput(f);
});

test('rotation permits 2 signatures and any trusted valid signature, but rejects malformed envelopes', () => {
  const rotating = trustedKeys({ keys: [...publicDocument.keys, publicEntry(otherPair.publicKey, 'test-next')] });
  const bytes = signEnvelope(json(payload()), JSON.stringify({ 'test-only': pem(pair.privateKey), 'test-next': pem(otherPair.privateKey) }), rotating);
  assert.equal(JSON.parse(bytes).signatures.length, 2);
  assert.equal(verifyEnvelope(bytes, keys).payload.sequence, 2n);
  assert.equal(verifyEnvelope(bytes, trustedKeys({ keys: [publicEntry(otherPair.publicKey, 'test-next')] })).payload.sequence, 2n);
  for (const change of [value => { value.signatures = []; }, value => { value.signatures.push(value.signatures[0]); },
    value => { value.signatures = [value.signatures[0], value.signatures[0]]; },
    value => { value.extra = true; }, value => { value.payload += '\n'; },
    value => { value.signatures[0].ed25519 = 'AA=='; }, value => { value.schema = 'hiroute.model-feed.envelope/v2'; }]) {
    const invalid = JSON.parse(bytes);
    change(invalid);
    assert.throws(() => verifyEnvelope(json(invalid), rotating));
  }
});

test('envelope and streaming response enforce the 2 MiB bound even without content length', async t => {
  assert.throws(() => verifyEnvelope(Buffer.alloc(MAX_ENVELOPE_BYTES + 1), keys), /exceeds 2 MiB/);
  for (const withHeader of [false, true]) {
    const f = await fixture(t);
    const result = new Response(new ReadableStream({ start(controller) {
      controller.enqueue(new Uint8Array(MAX_ENVELOPE_BYTES));
      controller.enqueue(new Uint8Array(1));
      controller.close();
    } }), { headers: withHeader ? { 'Content-Length': String(MAX_ENVELOPE_BYTES + 1) } : {} });
    await assert.rejects(publishFeed({ ...f, fetchImpl: async () => result }), /exceeds 2 MiB/);
    await noOutput(f);
  }
});

test('a build artifact containing a feed is refused even for a bootstrap/no-op deployment', async t => {
  const f = await fixture(t, null);
  await mkdir(path.dirname(f.destination));
  await writeFile(f.destination, 'unexpected artifact');
  await assert.rejects(publishFeed({ ...f, fetchImpl: noRequest }), /dist already contains a feed/);
  assert.equal(await readFile(f.destination, 'utf8'), 'unexpected artifact');
});

test('sequence values retain the full u64 range without rounding or accepting non-integers', () => {
  for (const sequence of ['9007199254740993', '18446744073709551615']) {
    const bytes = Buffer.from(json(payload()).toString().replace('"sequence":2', `"sequence":${sequence}`));
    assert.equal(checkPayload(bytes).sequence, BigInt(sequence));
    assert.equal(verifyEnvelope(signEnvelope(bytes, secret, keys), keys).payload.sequence, BigInt(sequence));
  }
  for (const sequence of ['0', '-1', '1.5', '1e3', '18446744073709551616', '"2"']) {
    assert.throws(() => checkPayload(Buffer.from(json(payload()).toString().replace('"sequence":2', `"sequence":${sequence}`))), /sequence/);
  }
});

test('payload structure rejects invalid dates, kinds, duplicate IDs, and unsafe featured links', () => {
  const featured = { id: 'test-entry', kind: 'new_model', model_configuration_id: 'model.test',
    headline: { en: 'Test model', zh: '测试模型' }, note: { en: 'Test announcement', zh: '测试公告' },
    links: ['https://hiroute.ai/en/docs/'], included_on: '2026-10-18' };
  const record = { kind: 'price_rate', min_client_version: '0.3.2', included_on: '2026-10-18',
    record: { price_rate_id: 'price.test' } };
  assert.equal(checkPayload(json(payload(2, { featured: [featured], records: [record] }))).sequence, 2n);
  for (const extra of [{ schema: 'unknown' }, { source_revision: 'main' }, { issued_at: '2026-02-30T00:00:00Z' },
    { records: [{ ...record, kind: 'connector' }] }, { records: [{ ...record, min_client_version: '0.03.2' }] },
    { records: [record, record] }, { featured: [featured, featured] }, { unknown: true },
    ...['https://example.com/', 'https://hiroute.ai.evil/', 'https://hiroute.ai@evil/', 'http://hiroute.ai/',
      'https://hiroute.ai/\\evil', 'https://hiroute.ai/\n@evil/'].map(link => ({ featured: [{ ...featured, links: [link] }] }))]) {
    assert.throws(() => checkPayload(json(payload(2, extra))));
  }
});

test('source metadata mismatch and oversized future envelopes fail in the unsigned build check', async t => {
  const f = await fixture(t);
  await writeFile(path.join(f.sourceDirectory, 'publication.json'), json({ sequence: 3, issued_at: payload().issued_at }));
  await assert.rejects(checkFeed(f), /publication metadata differs/);
  await writeFile(path.join(f.sourceDirectory, 'publication.json'), json({ sequence: 2, issued_at: payload().issued_at }));
  const bytes = Buffer.concat([json(payload()), Buffer.alloc(Math.floor(MAX_ENVELOPE_BYTES * 0.8), ' ')]);
  await writeFile(path.join(f.sourceDirectory, 'payload.json'), bytes);
  await assert.rejects(checkFeed(f), /signed envelope would exceed 2 MiB/);
});

test('duplicate JSON fields are rejected, including escaped spellings in authenticated payloads', () => {
  const duplicate = Buffer.from(json(payload()).toString().replace('"sequence":2', '"sequence":1,"seque\\u006ece":2'));
  assert.throws(() => checkPayload(duplicate), /invalid JSON/);
  const signed = json({ schema: 'hiroute.model-feed.envelope/v1', payload: duplicate.toString('base64'),
    signatures: [{ key_id: 'test-only', ed25519: sign(null, duplicate, pair.privateKey).toString('base64') }] });
  assert.throws(() => verifyEnvelope(signed, keys), /invalid JSON/);
  const valid = envelope(payload()).toString();
  const duplicateEnvelope = Buffer.from(valid.replace('"schema":', '"schema":"discarded","schema":'));
  assert.throws(() => verifyEnvelope(duplicateEnvelope, keys), /invalid JSON/);
});

test('exact envelope size is accepted; invalid UTF-8 and BOM bytes are refused', () => {
  const signed = envelope(payload());
  assert.equal(verifyEnvelope(Buffer.concat([signed, Buffer.alloc(MAX_ENVELOPE_BYTES - signed.length, ' ')]), keys).payload.sequence, 2n);
  assert.throws(() => checkPayload(Buffer.from([0xff])), /invalid UTF-8/);
  assert.throws(() => checkPayload(Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), json(payload())])), /invalid JSON/);
});
