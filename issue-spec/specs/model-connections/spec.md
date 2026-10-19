# Connecting and managing models

## Purpose

A saved connection owns a stable source identity, protocol endpoints, credentials,
and selected model bindings. Templates and metadata assist configuration; saved
source membership and current runtime authorization determine actual access.

## Requirements

### Requirement: The API wizard uses the current executable directory

Desktop MUST present service selection, connection configuration, and model
selection as separate steps. Provider/product counts, search, billing filters,
and branding come from the complete current executable catalog rather than a
fixed sample. A provider with several products requires an explicit product
choice; custom, reconnect, and dedicated free-resource entry points may start at
the appropriate configuration step without expanding their catalog scope.

Every connection has an editable display name, initially the chosen product name.
The name is a label, not supplier/account/endpoint/model identity. New Desktop
names must pass its nonempty, 60-character, and duplicate-name checks; existing
same-name sources remain readable and independent. Check, model selection, save,
and recheck preserve the user's valid name without replacing it with a template.

Back/forward within the wizard preserves each product's separate non-secret draft
and current-session credential input. Switching products cannot copy a Key into
another product. Closing, discarding, or saving releases temporary credentials;
ordinary persistence never stores them. Editing check-relevant facts invalidates
the previous check. Internal navigation is not abandonment; close uses the normal
unsaved-edit rule. None of these actions implicitly starts inference.

### Requirement: Connection identity survives supported lifecycle edits

One template may create multiple independent connections even with identical
provider/model names or Key strings. Source and model views retain the connection
label and separate product identity. Rename, recheck, and append retain source
and existing Binding identity. Append adds only explicitly selected new models,
without duplicates, replacing credentials, disabling existing models, or changing
old facts after a directory refresh. Refresh retains the currently selected source.

Desktop and CLI MUST share rename, append, remove-model, delete-source, endpoint,
Key, and enabled-state semantics. Removing one model preserves remaining members
and shared credentials. Removing the last model requires explicit whole-source
deletion; removing the last Key instead leaves a repairable source. Unreachable
sources may be saved inactive, and failed replacement cannot overwrite a working
configuration with partially prepared state.

#### Scenario: Recheck reveals additional models

- **WHEN** a saved registered source's current account directory contains models
  not yet imported
- **THEN** they MUST be selectable for explicit append, while existing members,
  Keys, enabled state, label, and bindings remain stable.

### Requirement: Deletion respects exact references and owned secrets

Deleting or narrowing membership MUST inspect exact Bindings, including active
and disabled retained routes, held execution/Continue versions, drafts, and
unreconciled recovery references. Unknown or failed reference reads block deletion.
Concurrent references are rechecked inside the writer; previews are not authority.
UI shows the blocking objects and does not silently rewrite their plans.
Historical usage is preserved.

An explicit publication checkpoint MAY rotate old recovery references only from
the verified installed active publication. Preview binds its exact digest and
workspace revisions; Apply rechecks under the writer and advances publication
revision once through normal prepare/install/activate/rollback/restart. It preserves
plans, heads/tombstones, grants, aliases, and permission facts, replacing the old
last-known-good only after success. Empty active state remains NoNewCalls. The
checkpoint neither deletes a source nor invokes inference; unresolved/failed
checkpoint is not permission to delete or manually edit recovery snapshots.

Source deletion MUST atomically manage the source and its HiRoute-owned secrets
through a recoverable Operation. Compensation and retry preserve exact secret/source
identity and cannot overwrite later configuration. Subscription removal preserves
native login/configuration; late checks, saves, and maintenance cannot recreate the
removed source. A future scan requires explicit approval again. Released legacy
save semantics remain replacement semantics; append/delete are explicit intents.

### Requirement: Raw upstream model IDs are opaque, not internal identifiers

CLI/API, persistence, compilation, publication, and credential authorization MUST
preserve an upstream ID exactly: nonempty, at most 512 UTF-8 bytes, no leading or
trailing Unicode White_Space, and no Unicode Cc controls. Unicode, punctuation,
and internal spaces are legal; U+FEFF is preserved because it is not White_Space.
No lowercasing, transliteration, normalization, or escaping may replace its identity.
Internal source/Binding/offer references retain their stricter identifier rules.

Desktop's manual entry policy currently accepts only ASCII for new or changed raw
IDs, with the same whitespace/control/byte bounds. Non-ASCII display names remain
separate. Existing, directory-selected, and metadata-prefilled non-ASCII IDs retain
their exact value when other fields are edited; replacing one with a different
non-ASCII manual value fails before check, inference, or save. A malformed legacy
model cannot break the entire management snapshot: retain readable state and
report that model's execution restriction.

#### Scenario: Rename a model whose existing ID is Unicode

- **WHEN** the user edits only its display name or capabilities
- **THEN** the original ID MUST survive unchanged and be sent exactly at execution;
  the manual ASCII policy cannot force a replacement.

### Requirement: Registered endpoint facts remain server-owned

Executable registered options MUST resolve from the client-version-bound
ReleaseFacts manifest/registry/model-data embedded in the same build. Digests,
cross-references, and product version are checked before use. A runtime cannot
select or copy a second catalog from its data directory or treat schema-number
suffixes as user-selectable metadata versions. Future independently downloadable
catalogs require their own trust/update contract.

A registered check submits option ID, expected provenance, edit correlation,
selected models, and protected input. Daemon resolves exact destination, protocol,
authentication, static headers, identity, and billing; client-supplied overrides of
registered facts MUST be rejected. Native API uses direct HiRoute adapters, not CPA.
A product missing exact authentication or a supported adapter is metadata-only,
not an executable option.

Saved provenance is historical evidence. New presentation, recheck, and publication
resolve stable `connection_option_id` against the current embedded catalog and
compare actual saved endpoint/auth/header/model/capability/credential-destination
facts. A catalog digest change alone cannot invalidate compatible sources, enlarge
saved endpoints/Key authority, or rewrite frozen publications/runs. An absent option
or incompatible fact blocks only that source's new use with a real reason.

### Requirement: Directory access and inference evidence are independent

Model directory reads are optional enhancements. They use the selected account,
retain valid items amid malformed entries, and distinguish unsupported, empty,
invalid, authentication, permission, quota, timeout, and connectivity outcomes.
A registered or manually declared supported endpoint can be saved without per-model
endpoint records, a directory, or inference probing. An unrecognized ID retains
raw/provider/source scope with no borrowed canonical configuration, score, price,
or free entitlement. Safe text fallback or user declarations are labeled untested.

Check/refresh is explicit and bounded, does not import automatically, perform paid
inference, cycle through unselected targets, or establish continuous health.
Cancellation stops further probe requests; late results cannot replace later edits.
A real model/tool test requires explicit invocation and proves only what it tested.
Protected save retains candidate correlation, version checks, and recoverable result
handling even if a check succeeded earlier.

#### Scenario: Directory is missing but the endpoint is configured correctly

- **WHEN** the account offers no directory or the directory operation fails
- **THEN** the user MUST be able to enter models/capabilities manually, preserve
  unknown fields, and save supported configuration without a synthetic inference.

### Requirement: Custom connections preserve protocol and transport scope

A Native API source may have at most one endpoint per Responses, Messages, and
Chat Completions protocol, sharing model selection and one Key pool. All endpoints
are unauthenticated or use that pool, though authenticated header forms may differ.
Each endpoint owns its full target, auth, and necessary static headers. Same-protocol
execution is preferred; an absent protocol may use only an expressible existing
conversion. Endpoint health is isolated; manual Key disable affects the source.

Custom configuration supports name, base URL/path prefix, None/Bearer/specified
Header authentication, and multiple manual models. A user may explicitly configure
HTTP or HTTPS for a loopback, LAN or remote endpoint, whether its host is a literal
address or a hostname. HTTP is not rejected solely because the destination is
non-loopback. HTTPS is recommended; an HTTP selection explains that API keys and
request/response content travel in plaintext without adding a mandatory confirmation.
Target normalization preserves the configured hostname; DNS and connectivity are
evaluated by bounded request transport, not by passive discovery or a syntactic check.
The same transport policy applies to durable source validation, publication and
actual Gateway execution. Registered catalog destinations remain bound to their
exact HTTPS authority; managed CPA transport retains its numeric-loopback scope.
Target changes require revalidation. Credentials cannot
be redirected to an unauthorized destination; TLS failure never falls back to plain
transport. Embedded URL passwords/query Keys must use protected input instead.

Vision, tools, context, output, streaming, native reasoning, and price may remain
unknown. Trusted facts, probe evidence, user declaration, and unknown are distinct.
Contradictory parameters or unrepresentable reasoning are rejected rather than
silently stripped. Missing canonical match, scores, or price alone cannot block
an otherwise expressible model. Cost hints without currency/unit/product/region/time
semantics are not PriceRate, free entitlement, or savings evidence. Metadata field
closure and reference pricing follow [model facts](../model-catalog-price/spec.md).

#### Scenario: Save an explicitly configured HTTP source without a directory

- **WHEN** the owner configures a valid LAN or remote HTTP endpoint, leaves the model
  directory path empty and supplies supported manual model declarations
- **THEN** connection check, save and saved-source recheck accept that target without
  DNS or directory/inference calls, preserve the exact target and authentication,
  and do not claim reachability, account access or inference has been verified.

#### Scenario: Correct an invalid connection target

- **WHEN** a connection check rejects an invalid URL, unsafe path, authentication
  header or protocol header, including an additional protocol endpoint
- **THEN** the daemon/API retains a closed, specific reason in the existing error
  envelope, with the released `INVALID_ARGUMENTS` machine code; Desktop presents
  a localized correction and retains input. Errors never expose URL credentials,
  protected keys or raw provider responses. Other URL/path/authentication checks,
  redirect restrictions and HTTPS certificate validation remain enforced.

Source Issue: https://github.com/higress-group/HiRoute/issues/154

### Requirement: Saved-source recheck reconstructs protected facts

Recheck submits source ID, exact expected revision, permitted candidate reference,
edit revision, and check ID. It MUST NOT accept caller-provided lineage, endpoint,
option, protocol, credential identity/value, or capability facts. Daemon reconstructs
registered sources from current catalog plus saved identity, and custom sources
from their private safe recheck descriptor. A legacy custom source lacking that
descriptor fails closed; connector-owned subscriptions use their reauthorization
workflow. Recheck shares deduplication, cancellation, candidate, save, and recovery
contracts and reveals no reconstruction secrets.

Native discovery/preparation/preview associations are process-local, not durable
permissions. The trusted process retains full effective configuration layers,
scanner/source revisions, credential descriptors, and catalog evidence. It rechecks
both complete evidence and protected credential fingerprint before Preview and,
after idempotency/version checks, before writer admission begins the Operation.
A changed high-priority endpoint/model invalidates a candidate even if the token
file is unchanged. Exact `prepare_id` replay returns the same candidate; reuse for
a different discovery conflicts. Prepare is synchronous with no cancellation
endpoint: closing drops UI continuation, and late results are discarded by edit
correlation. Restart requires new Scan/Prepare; admitted Apply/Result remains durable.

Executable resolution requires a regular executable and bounded process/version
handling, not a new owner/group-writable-parent trust rule. Discovery cannot run
native configuration helpers/hooks. Missing managed Codex runtime is
`runtime_unavailable`, not a successful empty scan; an available runtime without
native login may legitimately return a complete empty result.

### Requirement: Subscription modes retain their credential owner

Codex and Claude share check/select/save/disable/cancel/recovery workflows with
provider-specific readers. Each provider has at most one enabled account/mode.
Independent CPA sign-in is the default recommendation; NativeBorrowed is an explicit
option and saved choices do not migrate automatically. Login completion alone does
not enable routes or import all models.

Desktop subscription checks and model selection SHALL use subscription-specific
titles. Native discovery controls, including Hermes profile scanning, SHALL remain
on the device scan list and stay hidden in subscription details. Returning to the
scan list restores its controls; cancelling proxy editing retains the selected
subscription context.

A successful subscription save ends its Desktop check interaction. A later explicit
check, including another provider or account, MUST proceed without releasing or
invalidating the saved access. Running checks and verified checks that have not
completed saving retain their existing interaction ownership.

| Mode | Credential authority | Repair action |
| --- | --- | --- |
| NativeBorrowed | Native client owns login/refresh; CPA receives current access leases only | Update native login and explicitly recheck |
| CpaManaged | CPA owns an isolated private auth directory and independent OAuth refresh | Repeat independent sign-in as required |

Stable provider account identity, not email, filename, or auth-index, determines
matching. A same-account mode switch requires check/save and retains selected source
and unremoved models. Another account requires explicit check/selection. Pending
login is isolated from the enabled instance; saving reuses its auth directory with
one refresh writer. OAuth callback binds exact session/state/provider through
protected input, not logs or ordinary Operations. Cancel, expiry, restart maintenance,
and removal fence late callbacks and unsaved credentials. Disable revokes admission
and stops refresh while retaining credentials; remove-login first revokes saved
route access, then stops the exact instance and deletes its owned credentials.
Neither claims provider-global OAuth revocation.

### Requirement: Independent sign-in receives browser callbacks locally

CPA owns temporary browser callback listeners for the existing Codex
`http://localhost:1455/auth/callback` and Claude `http://localhost:54545/callback`
redirects. Bind only available IPv4/IPv6 loopback families. Each provider has at
most one pending sign-in; saved accounts do not retain a callback listener.

Browser and protected manual input MUST share atomic, one-use admission for the
exact provider/session/state. Invalid, stale and duplicate input cannot replace
an accepted callback. CPA reception permits retry after failed callback-file
publication. HiRoute does not resend an admitted manual code after a failed or
uncertain submission: observe status, or cancel and start a new sign-in. The browser
acknowledges reception without claiming token exchange or sign-in success.
Release listeners after reception, cancellation, expiry or failure, before slow
token exchange. Do not redirect or reflect callback secrets in browser responses.

A port conflict MUST close partial listeners and retain the same pending session
for manual completion. Desktop explains this mode before the user explicitly opens
the browser, and accepts the full return URL through the existing protected input.
Automatic reception opens the browser normally and retains manual completion as
a fallback. Successful sign-in still requires explicit Check/Save; it does not
change enabled-account or routing limits.

### Requirement: Borrowed subscription refresh cannot enlarge saved membership

Only approved, saved, enabled NativeBorrowed sources may maintain their association
to trusted CPA current in the same native context. Current access-token rotation
and capability revocation stay inside CPA; source qualification, Gateway Attempts,
and audit do not persist/compare that token generation. Stable source, selected
models, Bindings, and route references remain intact.

The latest successful pinned account directory constrains original selected models;
removed eligibility retains configuration as unavailable, and newly visible models
are not auto-imported. One independently sealed Codex connection may preserve native
models only when the user explicitly enabled it and the saved account, native
directory, and CPA pinned snapshot all prove those models. This connection-only grant
cannot enlarge general source selection, plan candidates, or another Agent's access.
Absent proof prevents a global provider switch.

Qualification converges through protected checks, management reads, or maintenance;
Attempt issuance does not discover online directories. Unobserved provider permission
changes may reach upstream and use its final rejection, without enlarging selection.
Unsafe credential reads, deletion, or stable account mismatch fail closed before
replacement credentials or capability issuance. Revision conflicts and transient
CPA/network failures retain configuration and report retry/repair. Deleted/disabled
sources cannot be revived by late work, and sent requests cannot be recalled.

### Requirement: Borrowed Codex auth follows its supported file semantics

An explicit `HIROUTE_CODEX_AUTH_SOURCE` selects that file; otherwise borrowing uses
the selected CODEX_HOME file store. keyring, auto, ephemeral, and unknown store modes
cannot fall back to stale `auth.json`; recommend independent sign-in. This path does
not read Codex Keychain, decrypt native stores, or reconstruct arbitrary launch layers.
Claude retains its supported file/macOS Keychain adapter; Keychain interaction is
limited to explicit checking rather than passive discovery.

Explicit `auth_mode=chatgpt` takes precedence even when an API Key is present.
Absent/null mode implies ChatGPT only when API Key/PAT/Bedrock login fields do not
select another mode. Current access token must be nonempty, bounded, and usable as a
request credential. `id_token` and `last_refresh` are not required for borrowing.
Account ID comes first from `tokens.account_id`, then the supported account claim
in ID/access tokens. Missing account identity requests re-login rather than using
email, user ID, or token hash. Parsing claims establishes local association, not
signature validation or upstream authentication. CPA-owned refresh credentials
retain their own required fields.

### Requirement: CPA isolation and revocation remain available during slow I/O

The packaged CPA locator binds the adjacent executable to its final signed-byte
identity; missing, changed, or nonexecutable CPA affects subscriptions only and
cannot silently use a user's different CPA. Native API and control remain usable.
Provider failures are isolated from each other.

Credential/profile/file/Keychain/management-HTTP/process I/O MUST NOT synchronously
block Gateway async workers or hold their blocking locks. Bounded executors limit
running and queued work using the original absolute deadline/cancellation. Cancel
cannot release a slot while its blocking work still runs. Admission revocation is
separate from slow lifecycle serialization: disable/account/source switch revokes
old epochs before stopping old writers, and failed cleanup remains retryable.
Prepared results recheck source/account/token version/admission epoch; stale success
or failure cannot overwrite newer state. Final send rechecks capability epoch.

CPA-managed authentication may refresh and retry once for the same account/model
on 401 before response commitment. NativeBorrowed supplies no refresh token.
HiRoute does not implement or stack another OAuth refresh chain.

### Requirement: Proxy configuration is scoped and reports actual application

Subscription proxy modes are inherited startup environment, direct, and manual
HTTP(S). Desired configuration persists separately from the applied process state.
Manual mode consistently overrides both cases of inherited proxy variables; direct
clears them; inherited mode preserves the host's startup contract. Manual URLs
cannot contain credentials and inherited secrets cannot enter status/errors.

Only managed subscriptions and Claude's subscription remote checks use this policy;
Native API, Worker, and system settings do not. Local management stays direct and
proxy failure cannot fall back to direct internet. Editing during scan retains
results, cancel returns to them, and explicit save/restart creates a fresh scan.
A still-running old process or invalid configuration cannot be reported as applied.

### Requirement: Model management joins facts without creating a second authority

Search/filter/detail and operation actions MUST use shared exact source/Binding,
capability, reference-score, price, and subscription facts. Active route references
include regular and primary candidates in smart saving/custom branches, match the
Binding rather than name, and open the exact plan; disabled routes are not active use.

Saved subscriptions distinguish updating authorization, sign-in needed, connector/
network failure, and model exclusion from the latest pinned directory. They never
ask for an API Key to repair OAuth. Explicit recheck uses the complete existing
check/save/activation chain without reselecting membership or claiming inference.

Desktop uses `ListComputeV3`/`GetComputeV3` and
`hiroute.compute-management-snapshot/v3`; mode joins by source ID/revision from the
committed selection even offline. Unknown selection does not guess a mode.
Published `ListCompute`/`GetCompute` keep their strict V2 response fields from the
same projection; released CLI stays on that allowlist. The extension changes no
request envelope or persisted source and requires no implicit mode migration.

## Implementation references

- [Connection form ownership](../../../apps/desktop/src/features/model-connections/README.md)
  and [shared lifecycle planner](../../../crates/application/src/compute_management/mutations/lifecycle.rs).
- [Registered check](../../../crates/daemon/src/control/runtime/model_connections/registered.rs)
  and [source/reference persistence](../../../crates/local-storage/src/control/compute/management).
- [Subscription assembly](../../../crates/daemon/src/control/runtime/subscriptions.rs),
  [versioned query contract](../../../crates/application-api/src/compute_management_v3.rs), and
  [slow-I/O/revocation scenarios](../../../crates/cpa-bridge/src/runtime/tests/request_io.rs).
