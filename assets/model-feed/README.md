# Signed model feed

The service retrieves `https://hiroute.ai/model-feed/v1.json`. This is a signed,
versioned projection of the existing model metadata source, with editorial
announcements. It does not introduce a second editable model catalog.

## Maintained inputs and generation

| File | Owner and meaning |
| --- | --- |
| `featured.json` | Editorial array of `new_model` and `model_update` announcements; plain bilingual text and links restricted to `https://hiroute.ai/` |
| `publication.json` | Publication `sequence` and UTC `issued_at`; increase the sequence whenever payload bytes change |
| `policy.json` | `minimum_feed_client`, the first feed-capable release |
| `public-keys.json` | Reviewed production trust roots as `{ "keys": [{ "key_id": "…", "ed25519": "…" }] }`; `ed25519` is standard padded base64 of the raw 32-byte public key |
| `payload.json` | Generated unsigned payload; never hand-edit model facts here |

The Desktop client also bundles the maintained `featured.json` editorial input.
When no feed announcements are available, Home and quick start use only these
resolvable announcements, rather than labeling the first directory entries as new.
Bundled announcements keep the model's built-in origin and never claim signature
verification. An update headline may name an announced release while its note
distinguishes that announcement from the version served by a connection. The
underlying model identity, connection checks, prices and published routing remain
owned by the ordinary catalog and saved connection. An unresolved announcement
is hidden in both sources.

Change model facts in
[`metadata-catalog.json`](../model-data/current/inputs/metadata-catalog.json), then
follow the [metadata maintainer workflow](../../.agents/skills/model-metadata-maintainer/SKILL.md).
Regenerate the current model data and release bundle before running:

```sh
cargo run -p hiroute-release-facts -- feed build
cargo run -p hiroute-release-facts -- feed build --check
node apps/website/scripts/model-feed.mjs check
```

The Rust producer uses the client's admission code and exact supported release
bundles from repository history. It computes the minimum admitting client version,
carries inclusion dates forward from the committed payload, and omits models waiting
for a compatible release. A model's minimum version requires its executable capability
and offer together. CI passes `--base <revision>` to reject changed publication bytes
without a higher sequence and changed first-inclusion dates for retained records.
The payload's `source_revision` is the Git tree ID of the committed
`assets/release-facts/current/bundle` directory. It survives squash merges and
reproduces a file with `git show <tree-id>:model-data.json`; it is provenance,
not each website deployment's commit SHA.
One sequence identifies one immutable payload digest and source revision. Keep the
referenced bundle tree reachable from main history. Node structural checks
and signature checks complement, and never replace, Rust admission validation.

Before a feed-capable release is recorded, `payload.json` contains `null` followed
by a newline. Both build checks report bootstrap; deployment skips network access
and signing and still uploads the ordinary website. An empty production key list
is valid during development. Bootstrap is not a production feed publication.

## Production keys and publication

The owner must review and embed the production public key **before releasing the
first feed-capable client**. Provision the matching secret in the
`website-production` GitHub environment before the first feed publication. Never
commit private keys or use development/test keys as production trust roots.

`MODEL_FEED_SIGNING_KEYS` is a JSON object mapping one or two trusted `key_id`
strings to **unencrypted PKCS#8 PEM Ed25519 private keys**. Each value begins with
`-----BEGIN PRIVATE KEY-----`; PEM line breaks are JSON `\n` escapes. It is not a
base64-encoded PEM string, an OpenSSH key, or a raw seed. Store that complete JSON
object only in the environment secret. The publisher derives each public key and
requires it to match the repository's trusted public key before signing. Errors
never include key material.

The [website workflow](../../.github/workflows/website.yml) runs the shared Rust
check and Node check on the same source revision before uploading the unsigned
payload as a separate artifact. PR jobs have no signing secret and never publish.
Only `main` production pushes or dispatches enter the production environment. The
deploy job downloads that workflow revision's payload and runs:

```sh
node apps/website/scripts/model-feed.mjs publish --payload <validated-payload-path>
```

The publisher requests the fixed feed URL without redirects or cached-response
reuse. Only a real 404 permits first publication. Every existing envelope must
verify against a trusted key before its sequence or payload digest can be used.
Other HTTP responses, network errors, malformed envelopes, and invalid signatures
fail deployment before any upload.

An identical payload digest is a no-op: no feed file is written, and the existing
OSS object is preserved by the non-deleting upload. Different content requires a
strictly greater sequence. The script signs the exact payload bytes, self-verifies
the envelope, enforces the 2 MiB envelope limit, and atomically writes
`apps/website/dist/model-feed/v1.json`. It refuses a website artifact that already
contains a feed. Signing or validation failure leaves the existing served website
and feed intact. The job summary records status, sequence, payload SHA-256 and
source revision.

For rotation, include both public keys and sign with both matching private keys
until the oldest supported client trusts the new key. Clients accept a publication
when any trusted signature verifies. Keep the endpoint version stable for its
supported clients; introduce a new versioned endpoint and compatibility-register
entry if the contract changes incompatibly.

Ensure infrastructure serves missing `/model-feed/*` objects as real 404s, without
an HTML fallback or redirect. The existing short-cache OSS upload remains in use.
After the first feed-capable release and production key provisioning, verify a real
publication through both a standalone Linux service and macOS Desktop. Local tests
with ephemeral keys do not establish production signing or live retrieval.
