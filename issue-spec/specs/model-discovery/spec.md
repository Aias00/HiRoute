# Discovering local model sources

## Purpose

Discovery identifies model configuration that HiRoute can safely prepare for
import. Discovery, explicit approval, saved source state, and real inference are
separate stages.

## Requirements

### Requirement: Discovery is passive and partial failures are local

A scan MUST leave native Agent configuration unchanged, avoid inference, and
return supported findings independently of unreadable paths or malformed items.
Executable trust and recognizable configuration are separate facts. Unsupported
endpoints, unavailable credentials, and helper/OAuth-dependent sources MUST carry
a precise importability state rather than a claim of verified access.

Codex subscription discovery uses the configured managed runtime. Registered
Claude configuration can be recognized independently of executable trust. The
supported Zhipu Coding Plan / GLM-5.3 Messages configuration must resolve its exact
Messages capability binding. An optional import to the corresponding official
Responses product is a distinct, explicitly selected source with exact source and
target facts; a second protocol must not make the original binding ambiguous.
Other API configurations use the custom connection path.

#### Scenario: One Agent's configuration is unreadable

- **WHEN** an independent source is valid but another scan item cannot be parsed
- **THEN** the valid source MUST remain discoverable; the failed item reports its
  own state and scanning does not execute its authentication helper.

### Requirement: Approval precedes credential materialization and saving

Only explicit user selection/approval may prepare or save a discovered source.
Protected preparation MUST bind the source and credential facts needed for the
operation; normal output cannot reveal recoverable credentials. Subscription
approval authorizes the stable local source to use the trusted CPA runtime, rather
than a single access-token generation. Scan, expansion, and cancellation do not
create credential material or maintenance rights. Disabled, deleted, and never
approved sources cannot be automatically revived. The current discovery workflow
does not add multi-account management.

A save MUST report actual per-source success. A source's common save unit is atomic;
failed replacement preserves the previous effective source. Retry of an admitted
Apply follows the original Operation and its idempotency/versions.

#### Scenario: Native token rotates after approval

- **WHEN** the approved native account refreshes an access token without changing
  the approved source/account identity
- **THEN** maintenance MAY refresh the same authorized source; it MUST NOT treat
  scan-only or disabled sources as having equivalent permission.

### Requirement: Codex subscription membership comes from the account directory

The managed CPA connection MUST discover account models from Codex's actual models
endpoint with its account, proxy, prefix, and exclusion rules. Static metadata may
fill fields for an exact returned ID but cannot introduce absent account members.
While creating an access lease, HiRoute MUST obtain the selected local Codex
engine's bounded version result and pass its major.minor.patch as
`hiroute_client_version`; Desktop's located engine and CLI's resolved executable
must not be replaced with another installation or an assumed latest version.

Registration/update, not ordinary inference, owns this lookup. Refresh has an
independent bounded 20-second control budget covering remote discovery and readback.
Missing version, timeout, or invalid directory cannot manufacture remote results:
a prior registered directory remains available, while a first failed lookup creates
no static substitute candidates. Discovery success does not prove inference.

#### Scenario: Directory lookup fails and later recovers

- **WHEN** an approved lease temporarily cannot obtain a client version or directory
- **THEN** failure MUST remain explicit and bounded; a later refresh uses the actual
  selected engine and current account results without inventing membership.

### Requirement: Native static API imports retain native precedence and evidence

Pi discovery MUST passively read effective user models/auth configuration and the
selected official package's static catalog. BOM/JSONC and native credential/template
precedence are preserved. A model-level `baseUrl` overrides its provider address;
provider defaults apply only where native semantics permit. Saved API credentials,
environment templates, and provider fallback are interpreted without running shell
helpers. OAuth, extra unsupported authentication headers, or unsupported overrides
remain non-importable. Provider and model identities remain visible separately.
DSH's static composition uses its own native reader through the same prepare/save
workflow; it must not be guessed from Pi rules.

The frontend submits opaque discovery/preparation identities. Daemon preparation
re-reads the native source and binds endpoint, raw model ID, credential, and native
source/catalog snapshot. Save rechecks those facts and rejects a stale source or
credential. Observed context/output/vision retain their observed basis; unverified
tool, streaming, or reasoning constants cannot be promoted to verified capability.

#### Scenario: Source changes between preparation and save

- **WHEN** the native effective address, model, credential, or relevant source
  snapshot changes after preparation
- **THEN** save MUST reject the stale candidate without modifying the source,
  and the user can rescan/prepare through the same protected path.

## Implementation references

- [Native discovery assembly](../../../crates/daemon/src/control/runtime/model_connections/pi_discovered.rs).
- [Pi filesystem scenarios](../../../crates/integrations/src/agents/filesystem_tests/pi.rs).
- [Subscription lifecycle](../../../crates/daemon/src/control/runtime/subscriptions.rs) and
  [borrowed Codex lease tests](../../../crates/cpa-bridge/src/borrowed_codex/tests.rs).
