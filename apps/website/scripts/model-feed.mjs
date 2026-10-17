import { createHash, createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';
import { appendFile, lstat, mkdir, open, rename, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const FEED_URL = 'https://hiroute.ai/model-feed/v1.json';
export const MAX_ENVELOPE_BYTES = 2 * 1024 * 1024;
const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const SPKI_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');
const U64_MAX = (1n << 64n) - 1n;
const recordIdFields = {
  model: 'model_configuration_id', model_endpoint_capability: 'capability_id',
  offer: 'offer_id', price_rate: 'price_rate_id',
};

function requireThat(condition, message) {
  if (!condition) throw new Error(message);
}

function object(value, label) {
  requireThat(value !== null && typeof value === 'object' && !Array.isArray(value), `${label}: expected an object`);
}

function fields(value, required, optional, label) {
  object(value, label);
  requireThat(required.every(key => Object.hasOwn(value, key))
    && Object.keys(value).every(key => required.includes(key) || optional.includes(key)), `${label}: invalid fields`);
}

function text(value, label, maximum = 1024) {
  requireThat(typeof value === 'string' && value.trim().length > 0
    && [...value].length <= maximum && !/[\u0000-\u001f\u007f]/u.test(value), `${label}: invalid text`);
}

function rejectDuplicateKeys(source) {
  // JSON.parse checks grammar first. Walk its lexical tokens to reject ambiguous
  // duplicate fields, including keys written with different Unicode escapes.
  const stack = [];
  for (const [token] of source.matchAll(/"(?:[^"\\]|\\.)*"|[{}[\],:]|[^{}[\],:\s]+/g)) {
    if (token === '{') stack.push({ keys: new Set(), wantsKey: true });
    else if (token === '[') stack.push({ keys: null });
    else if (token === '}' || token === ']') stack.pop();
    else {
      const frame = stack.at(-1);
      if (frame?.keys) {
        if (token === ',') frame.wantsKey = true;
        else if (frame.wantsKey && token.startsWith('"')) {
          const key = JSON.parse(token);
          requireThat(!frame.keys.has(key), 'duplicate JSON field');
          frame.keys.add(key);
          frame.wantsKey = false;
        }
      }
    }
  }
}

function parseJson(bytes, label) {
  requireThat(bytes.length <= MAX_ENVELOPE_BYTES, `${label}: exceeds 2 MiB`);
  let source;
  try { source = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes); }
  catch { throw new Error(`${label}: invalid UTF-8`); }
  try {
    const parsed = JSON.parse(source, (key, value, context) => {
      // Preserve the entire u64 range, including sequences beyond JS safe integers.
      if (key === 'sequence') {
        requireThat(typeof value === 'number' && context?.source?.length <= 20
          && /^[1-9]\d*$/.test(context.source), 'invalid sequence');
        const sequence = BigInt(context.source);
        requireThat(sequence <= U64_MAX, 'invalid sequence');
        return sequence;
      }
      return value;
    });
    rejectDuplicateKeys(source);
    return parsed;
  } catch { throw new Error(`${label}: invalid JSON or sequence`); }
}

function date(value, label) {
  requireThat(typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value)
    && Number.isFinite(Date.parse(`${value}T00:00:00Z`))
    && new Date(`${value}T00:00:00Z`).toISOString().slice(0, 10) === value, `${label}: invalid date`);
}

function publication(value) {
  requireThat(typeof value.sequence === 'bigint' && value.sequence >= 1n && value.sequence <= U64_MAX, 'publication: invalid sequence');
  requireThat(typeof value.issued_at === 'string'
    && /^\d{4}-\d{2}-\d{2}T(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d+)?(?:Z|\+00:00)$/.test(value.issued_at)
    && Number.isFinite(Date.parse(value.issued_at)), 'publication: issued_at must be RFC 3339 UTC');
  date(value.issued_at.slice(0, 10), 'publication issued_at');
}

function version(value, label) {
  const numeric = '(?:0|[1-9][0-9]*)';
  const prerelease = '(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)';
  requireThat(typeof value === 'string' && new RegExp(`^${numeric}\\.${numeric}\\.${numeric}(?:-${prerelease}(?:\\.${prerelease})*)?(?:\\+[0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*)?$`).test(value), `${label}: invalid version`);
}

function featuredEntries(entries) {
  requireThat(Array.isArray(entries), 'featured: expected an array');
  const ids = new Set();
  for (const entry of entries) {
    fields(entry, ['id', 'kind', 'model_configuration_id', 'headline', 'note', 'links', 'included_on'], [], 'featured entry');
    text(entry.id, 'featured id');
    requireThat(!ids.has(entry.id), 'featured: duplicate id');
    ids.add(entry.id);
    requireThat(['new_model', 'model_update'].includes(entry.kind), 'featured: unknown kind');
    text(entry.model_configuration_id, 'featured model');
    for (const key of ['headline', 'note']) {
      fields(entry[key], ['en', 'zh'], [], `featured ${key}`);
      for (const locale of ['en', 'zh']) text(entry[key][locale], `featured ${key}.${locale}`);
    }
    date(entry.included_on, 'featured included_on');
    requireThat(Array.isArray(entry.links), 'featured links: expected an array');
    for (const link of entry.links) {
      text(link, 'featured link', 2048);
      let url;
      try { url = new URL(link); } catch { throw new Error('featured link: invalid URL'); }
      requireThat(link.startsWith('https://hiroute.ai/') && url.origin === 'https://hiroute.ai'
        && !url.username && !url.password && !link.includes('\\'), 'featured link: must stay on https://hiroute.ai/');
    }
  }
}

// These checks complement the Rust producer's shared admission validation. Nested
// model facts and release compatibility remain owned by that production validator.
export function checkPayload(bytes) {
  const payload = parseJson(bytes, 'payload');
  if (payload === null) return null;
  fields(payload, ['schema', 'sequence', 'issued_at', 'source_revision', 'records', 'featured'], [], 'payload');
  requireThat(payload.schema === 'hiroute.model-feed/v1', 'payload: unsupported schema');
  publication(payload);
  requireThat(typeof payload.source_revision === 'string' && /^[0-9a-f]{40}$/.test(payload.source_revision), 'payload: invalid source_revision');
  requireThat(Array.isArray(payload.records), 'records: expected an array');
  const ids = new Set();
  for (const entry of payload.records) {
    fields(entry, ['kind', 'min_client_version', 'included_on', 'record'], [], 'record');
    requireThat(Object.hasOwn(recordIdFields, entry.kind), 'record: forbidden or unknown kind');
    version(entry.min_client_version, 'record min_client_version');
    date(entry.included_on, 'record included_on');
    object(entry.record, 'record facts');
    if (entry.kind === 'model') {
      fields(entry.record, ['definition', 'canonical'], ['native_reasoning'], 'model facts');
      object(entry.record.definition, 'model definition');
      object(entry.record.canonical, 'model canonical metadata');
    }
    const id = (entry.kind === 'model' ? entry.record.definition : entry.record)[recordIdFields[entry.kind]];
    text(id, 'record id');
    const identity = `${entry.kind}:${id}`;
    requireThat(!ids.has(identity), 'records: duplicate id');
    ids.add(identity);
  }
  featuredEntries(payload.featured);
  return payload;
}

function base64(value, label, length) {
  requireThat(typeof value === 'string' && value.length > 0
    && /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value), `${label}: invalid base64`);
  const bytes = Buffer.from(value, 'base64');
  requireThat(bytes.toString('base64') === value && (length === undefined || bytes.length === length), `${label}: invalid bytes`);
  return bytes;
}

export function trustedKeys(document) {
  fields(document, ['keys'], [], 'public keys');
  requireThat(Array.isArray(document.keys), 'public keys: expected an array');
  const keys = new Map();
  for (const entry of document.keys) {
    fields(entry, ['key_id', 'ed25519'], [], 'public key');
    text(entry.key_id, 'public key id', 128);
    requireThat(!keys.has(entry.key_id), 'public keys: duplicate key id');
    const raw = base64(entry.ed25519, 'public key', 32);
    keys.set(entry.key_id, createPublicKey({ key: Buffer.concat([SPKI_PREFIX, raw]), format: 'der', type: 'spki' }));
  }
  return keys;
}

export function payloadDigest(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export function verifyEnvelope(bytes, keys) {
  const envelope = parseJson(bytes, 'envelope');
  fields(envelope, ['schema', 'payload', 'signatures'], [], 'envelope');
  requireThat(envelope.schema === 'hiroute.model-feed.envelope/v1', 'envelope: unsupported schema');
  requireThat(Array.isArray(envelope.signatures) && envelope.signatures.length >= 1
    && envelope.signatures.length <= 2, 'envelope: expected 1 or 2 signatures');
  const payloadBytes = base64(envelope.payload, 'envelope payload');
  const ids = new Set();
  let verified = false;
  for (const signature of envelope.signatures) {
    fields(signature, ['key_id', 'ed25519'], [], 'signature');
    text(signature.key_id, 'signature key id', 128);
    requireThat(!ids.has(signature.key_id), 'envelope: duplicate signing key');
    ids.add(signature.key_id);
    const signatureBytes = base64(signature.ed25519, 'signature', 64);
    const key = keys.get(signature.key_id);
    if (key && verify(null, payloadBytes, key, signatureBytes)) verified = true;
  }
  requireThat(verified, 'envelope: no trusted valid signature');
  // Never parse unauthenticated payload bytes.
  const payload = checkPayload(payloadBytes);
  requireThat(payload !== null, 'envelope: bootstrap payload cannot be published');
  return { payload, digest: payloadDigest(payloadBytes) };
}

export function signEnvelope(payloadBytes, secret, keys) {
  let privateKeys;
  try {
    privateKeys = JSON.parse(secret);
    object(privateKeys, 'signing keys');
    requireThat(Object.keys(privateKeys).length >= 1 && Object.keys(privateKeys).length <= 2, 'signing keys');
  } catch { throw new Error('signing: MODEL_FEED_SIGNING_KEYS must contain 1 or 2 PKCS#8 PEM keys'); }
  const signatures = [];
  for (const [keyId, pem] of Object.entries(privateKeys)) {
    let key;
    try {
      requireThat(typeof pem === 'string' && pem.startsWith('-----BEGIN PRIVATE KEY-----'), 'PKCS#8 PEM required');
      key = createPrivateKey({ key: pem, format: 'pem', type: 'pkcs8' });
      requireThat(key.asymmetricKeyType === 'ed25519', 'Ed25519 required');
      const trusted = keys.get(keyId);
      requireThat(trusted && createPublicKey(key).export({ type: 'spki', format: 'der' })
        .equals(trusted.export({ type: 'spki', format: 'der' })), 'public key mismatch');
      signatures.push({ key_id: keyId, ed25519: sign(null, payloadBytes, key).toString('base64') });
    } catch { throw new Error('signing: private key is invalid or does not match a trusted public key'); }
  }
  const bytes = Buffer.from(`${JSON.stringify({ schema: 'hiroute.model-feed.envelope/v1', payload: payloadBytes.toString('base64'), signatures })}\n`);
  verifyEnvelope(bytes, keys);
  return bytes;
}

async function readBounded(filename) {
  const file = await open(filename, 'r');
  try {
    const bytes = Buffer.alloc(MAX_ENVELOPE_BYTES + 1);
    let total = 0;
    while (total < bytes.length) {
      const { bytesRead } = await file.read(bytes, total, bytes.length - total, null);
      if (bytesRead === 0) break;
      total += bytesRead;
    }
    requireThat(total <= MAX_ENVELOPE_BYTES, 'feed input: exceeds 2 MiB');
    return bytes.subarray(0, total);
  } finally { await file.close(); }
}

export async function checkFeed({ sourceDirectory = path.join(repository, 'assets/model-feed'), payloadPath = path.join(sourceDirectory, 'payload.json') } = {}) {
  const payloadBytes = await readBounded(payloadPath);
  const payload = checkPayload(payloadBytes);
  const policy = parseJson(await readBounded(path.join(sourceDirectory, 'policy.json')), 'policy');
  fields(policy, ['minimum_feed_client'], [], 'policy');
  version(policy.minimum_feed_client, 'minimum_feed_client');
  const metadata = parseJson(await readBounded(path.join(sourceDirectory, 'publication.json')), 'publication');
  fields(metadata, ['sequence', 'issued_at'], [], 'publication');
  publication(metadata);
  const featured = parseJson(await readBounded(path.join(sourceDirectory, 'featured.json')), 'featured');
  featuredEntries(featured);
  const keys = trustedKeys(parseJson(await readBounded(path.join(sourceDirectory, 'public-keys.json')), 'public keys'));
  if (payload) {
    requireThat(payload.sequence === metadata.sequence && payload.issued_at === metadata.issued_at, 'payload: publication metadata differs; regenerate the feed');
    // Reserve room for both rotation signatures before accepting an unsigned artifact.
    const estimate = Buffer.byteLength(JSON.stringify({ schema: 'hiroute.model-feed.envelope/v1',
      payload: payloadBytes.toString('base64'), signatures: Array.from({ length: 2 }, () => ({ key_id: '𐀀'.repeat(128), ed25519: 'x'.repeat(88) })) }));
    requireThat(estimate <= MAX_ENVELOPE_BYTES, 'payload: signed envelope would exceed 2 MiB');
  }
  return { payloadBytes, payload, keys, digest: payloadDigest(payloadBytes) };
}

async function servedFeed(fetchImpl, keys) {
  let response;
  try {
    response = await fetchImpl(FEED_URL, { redirect: 'error', credentials: 'omit', cache: 'no-store',
      signal: AbortSignal.timeout(30_000), headers: { Accept: 'application/json', 'Cache-Control': 'no-cache' } });
  } catch { throw new Error('served feed: network request failed (redirects are forbidden)'); }
  if (response.status === 404) {
    await response.body?.cancel();
    return null;
  }
  requireThat(response.status === 200 && !response.redirected, `served feed: unexpected HTTP status ${response.status}`);
  const length = response.headers.get('content-length');
  requireThat(length === null || (/^\d+$/.test(length) && BigInt(length) <= BigInt(MAX_ENVELOPE_BYTES)), 'served feed: exceeds 2 MiB or invalid content length');
  requireThat(response.body, 'served feed: missing body');
  const reader = response.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.length;
      requireThat(total <= MAX_ENVELOPE_BYTES, 'served feed: exceeds 2 MiB');
      chunks.push(value);
    }
  } finally { await reader.cancel(); }
  return verifyEnvelope(Buffer.concat(chunks, total), keys);
}

async function assertMissing(filename) {
  try { await lstat(filename); }
  catch (error) { if (error.code === 'ENOENT') return; throw error; }
  throw new Error('publication: dist already contains a feed; use the unsigned website build artifact');
}

function resultFor(status, checked) {
  return { status, sequence: checked.payload?.sequence.toString() ?? null,
    payload_digest: checked.payload ? checked.digest : null, source_revision: checked.payload?.source_revision ?? null };
}

export async function publishFeed({ sourceDirectory, payloadPath, distDirectory = path.join(repository, 'apps/website/dist'),
  env = process.env, fetchImpl = fetch } = {}) {
  requireThat(env.GITHUB_EVENT_NAME === 'push' || env.GITHUB_EVENT_NAME === 'workflow_dispatch', 'publication: only a production push or dispatch may sign');
  requireThat(env.GITHUB_REF === 'refs/heads/main', 'publication: only main may sign');
  const destination = path.join(distDirectory, 'model-feed/v1.json');
  await assertMissing(destination);
  const checked = await checkFeed({ sourceDirectory, payloadPath });
  let result;
  let envelope;
  if (!checked.payload) result = resultFor('bootstrap', checked);
  else {
    const served = await servedFeed(fetchImpl, checked.keys);
    if (served?.digest === checked.digest) result = resultFor('already_current', checked);
    else {
      requireThat(!served || checked.payload.sequence > served.payload.sequence,
        'publication: proposed sequence must exceed the served sequence when payload content differs');
      envelope = signEnvelope(checked.payloadBytes, env.MODEL_FEED_SIGNING_KEYS, checked.keys);
      result = resultFor(served ? 'published' : 'first_publication', checked);
    }
  }
  if (env.GITHUB_STEP_SUMMARY) {
    await appendFile(env.GITHUB_STEP_SUMMARY, `\n### Model feed\n\n- Status: ${result.status}\n- Sequence: ${result.sequence ?? 'none'}\n- Payload SHA-256: ${result.payload_digest ?? 'none'}\n- Source revision: ${result.source_revision ?? 'none'}\n`);
  }
  if (envelope) {
    await mkdir(path.dirname(destination), { recursive: true });
    const temporary = `${destination}.${process.pid}.tmp`;
    try {
      const file = await open(temporary, 'wx', 0o644);
      try { await file.writeFile(envelope); await file.sync(); } finally { await file.close(); }
      await rename(temporary, destination);
    } finally { await rm(temporary, { force: true }); }
  }
  return result;
}

async function main() {
  const [command, ...args] = process.argv.slice(2);
  requireThat(['check', 'publish'].includes(command), 'usage: model-feed.mjs check|publish [--payload PATH]');
  requireThat(args.length === 0 || (args.length === 2 && args[0] === '--payload'), 'usage: model-feed.mjs check|publish [--payload PATH]');
  const options = args.length ? { payloadPath: path.resolve(args[1]) } : {};
  const result = command === 'publish' ? await publishFeed(options)
    : resultFor('checked', await checkFeed(options));
  if (command === 'check' && result.sequence === null) result.status = 'bootstrap';
  console.log(JSON.stringify(result));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(`Model feed: ${error.message}`); process.exitCode = 1; });
}
