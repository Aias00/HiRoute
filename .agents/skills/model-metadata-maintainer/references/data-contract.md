# Current model metadata source contract

This reference defines the sole current source-data shape maintained by the skill. Generated runtime files are deterministic projections of this source, not independent catalog generations. This source shape is not a runtime wire schema and carries no compatibility promise.

## Entity graph

```text
evidence_source ─┬─> model
                 ├─> access_product ──> interface
                 ├─> endpoint_binding ──> model + product + interface candidates
                 ├─> pricing_fact ──> model + product + upstream ID
                 └─> dynamic_route ──> product

provenance source ─┬─> provider_metadata_record
                   └─> model_metadata_record ──> provider metadata record
```

## Top-level invariants

- `artifact_kind` is `hiroute.model-metadata-current-source`.
- `scope.catalog_generation` is `current`; it names the sole mutable catalog, not an edition selector.
- Delivery is `client-bundled` for MVP, this source feeds the runtime projection, and the future refresh transport boundary is HTTPS.
- Legacy projections and compatibility catalogs are absent.
- Every entity key is unique and stable within this dataset.
- Cross-entity relationships use keys, never display names.
- Arrays are sorted by their primary key; evidence refs and interface candidates are sorted and deduplicated.
- `models` normalize publisher identities; provider-scoped metadata records describe callable IDs and connection hints. This is a scope distinction, not a confidence tier.
- A record's source never determines whether it is usable. `metadata_completeness` and the scenario requirements in `metadata_usage_policy` determine `usable_for` mechanically.

## Evidence source

Required fields: `source_key`, `authority`, `source_kind`, `locator`, `collected_on`, `bytes_digest`, `digest_state`.

- Locator must be HTTPS and allowed by `source-registry.json`.
- `digest_state=captured` requires `bytes_digest=sha256:<64 lowercase hex>`.
- `digest_state=not-captured` requires `bytes_digest=null`.
- Provider pages can establish public identity/capability claims. Account availability requires authenticated metadata.

## Model

Required identity fields: `model_key`, `publisher`, `display_name`, `canonical_identity`, `upstream_ids`, `identity_stability`, `lifecycle`, `evidence_refs`.

Model-level capability fields describe the publisher model, not every hosted endpoint. Product-specific caps, aliases, parameter rendering, or availability belong on an endpoint binding.

Token rules:

- Positive integer without a state is exact for the record's declared scope.
- Null requires an explicit state.
- `shared-total-budget` means input and output consume one exact total budget.
- `entitlement-dependent` and `runtime-required` never supply an executable static limit.

## Access product and interface

An access product owns provider, product kind, region, billing, restrictions, credential kind, interfaces, evidence, and disposition. Never store secret values.

Each interface has a unique `interface_key` under its product plus protocol and URL/path or template. An interface describes a possible wire surface; it does not prove every documented model works on it.

## Endpoint binding

An endpoint binding connects exactly one `product_key`, one `model_key`, and one `upstream_model_id`. `interface_candidates` must belong to that product.

`availability.state=conditional` plus `protocol_qualification=runtime-required` is the default for documentation-only associations. Promote a binding only after the target account, region, entitlement, request shape, stream framing, tools, errors, and usage semantics have the required evidence.

An upstream ID can be a canonical callable ID, provider-stable ID, versioned ID, rolling alias, or legacy routing alias. Optional binding-level `lifecycle` and `replaced_by_upstream_id` preserve alias retirement without retiring the served model identity. The replacement must belong to the same model record. Endpoint-specific limit, capability, reasoning, or rendering overrides belong here rather than changing the publisher model globally.

`capability_overrides` may narrow a binding's context/output limits or tool/vision/streaming booleans when the access product publishes a different contract from the publisher model. These fields are optional, and absence means use the model-level fact. Each override must have binding evidence; it does not promote a conditional binding to executable qualification.

## Pricing fact

A pricing fact connects one access product, one model, and one upstream ID. It never attaches a provider's price to the same model on another host or subscription.

Required fields: `pricing_key`, `product_key`, `model_key`, `upstream_model_id`, `currency`, `unit`, `price_state`, non-empty `schedules`, and `evidence_refs`.

- Monetary rates are non-negative decimal strings, avoiding binary floating-point ambiguity.
- `per-1m-tokens` schedules name components such as input, output, cache read/write, or cache-hit/cache-miss input.
- Time windows, prompt thresholds, service tiers, regions, and entitlements remain explicit conditions. Do not flatten them into one blended rate.
- `application` distinguishes marginal, whole-request, provider-defined, and unknown behavior. Do not guess how a tier applies.
- Credits, request fees, subscription inclusion, and missing price contracts are distinct from a zero token rate.
- Legacy aliases normally resolve through binding replacement metadata to a priced canonical upstream ID; do not duplicate the same price row unless the provider prices the alias differently.

## Dynamic route

A dynamic route is not a model. It has no rated model identity and must require actual response model and provider attribution. Stable model scores or global caps must not be attached to it.

## Completeness-gated provider and model metadata

`provider_metadata_records` and `model_metadata_records` use one source-independent schema. The current population comes from registered Hermes Agent and OpenClaw profiles pinned to exact commits; later maintainers may add new source adapters without creating another confidence layer.

Every record contains:

- a provider-scoped stable key and sorted `provenance_refs`;
- normalized fields plus raw `source_assertions` and exact `source_locations`;
- `metadata_completeness`, with each domain set to `complete`, `partial`, `missing`, or `not-applicable` from the fields themselves;
- sorted `usable_for`, derived from `metadata_usage_policy.scenario_requirements`.

A provider record covers identity, endpoint, authentication, and discovery. Endpoint prefill requires exactly one safe HTTP(S) Base URL and one supported protocol without a conflicting adapter signal. Partial endpoint information can still support adapter planning; identity, credential, and discovery fields independently unlock their own scenarios.

A model record covers identity, limits, modalities, capabilities, reasoning rendering, lifecycle, and cost. An upstream ID is sufficient for directory and editable model-ID prefill. Known context or output fields unlock their respective limit prefill, both unlock request-limit prefill, and explicit capability/rendering/lifecycle/cost fields unlock only the corresponding scenario. Conflict affects the relevant domain, not the whole record.

`reasoning_rendering=complete` means the source-provided effort/profile mapping hints are structurally complete for `reasoning-rendering-hint`; it does not mean an executable protocol parameter path or HiRoute render convention is known. Likewise, generic tool or streaming claims do not establish endpoint-scoped protocol fidelity. Runtime use must retain unknown fields until an exact user declaration, connector verification, or qualified endpoint binding supplies the missing facts.

`normalized_model_matches` links a provider-scoped record to possible publisher identities for navigation. It neither proves identity nor gates the provider-scoped record's `usable_for` scenarios.

Each provenance ref is `{kind, source_key}`. `kind=client-discovery-run` points to an exact-commit run; `kind=evidence-source` points to catalog evidence. Adding a source changes provenance adapters/refs, not the metadata-record shape.

Provenance is used to reproduce, refresh, audit, and resolve conflicts. It does not label a record high- or low-confidence. Descriptive completeness also does not prove operational availability: actual execution still requires endpoint, account, protocol, and entitlement qualification. Cost hints remain distinct from `pricing_facts` until currency, unit, product, and schedule are fully modeled.

## Determinate-outcome closure

The current source is closed before its runtime projection is generated. Every provider-scoped field must land on one of three determinate outcomes:

1. an observed or searched fact, kept as it is;
2. a conservative inference, written together with a per-field `field_provenance` entry `{basis: "inferred", rule_key}` that names the reviewed rule in `inference_rules`;
3. an explicit `unsupported` or `not-applicable` outcome, which is distinct from both `false` and `unknown`.

`unknown` must not survive closure in provider-scoped records. The closure is deterministic and idempotent: `tools/model-metadata/close_model_metadata.py` reads `tools/model-metadata/inference-rules.json`, closes every indeterminate field, re-derives `metadata_completeness` and `usable_for` from the closed fields, drops a recorded rule the moment it stops matching the record (re-closing the field), canonicalizes list-valued assignments, and fails when no rule can close a field. `--check` proves the catalog is a fixed point.

Pipeline order in a refresh: scan client repositories, materialize records, **close**, then validate. `scripts/validate_catalog.py` re-runs the closure and rejects any catalog that is not a fixed point, so an unclosed or hand-edited indeterminate field fails the gate.

Record fields that carry the closure:

- `field_provenance`: per-field `{basis, rule_key}`; absent means the field is observed. `field_provenance` never contains `basis: "observed"`, and a value with provenance is never presented as provider-verified.
- `execution_fit`: `native-text-representable`, `unsupported`, or `not-applicable`, with a `reason` for every non-representable outcome. It states the record's fit against the native text contract; it never grants runtime eligibility.
- `reasoning_rendering_state=not-applicable`: present exactly when the rendering domain does not apply, in which case the three hint lists stay empty.
- `cost_hint_state`: `recorded` or `not-recorded`, derived from the hints, never rule-assigned.
- `inference_rules`: the `rule_key`, `record_kind`, `basis`, `reason`, `evidence_refs`, and `collected_on` of every rule that closed at least one field. The deterministic match/assignment table stays in `tools/model-metadata/inference-rules.json`; the projected subset keeps each rule inspectable next to the values it closed.

Token limits in records may be `not-applicable` (the field does not apply, e.g. an image-generation model) but never `unknown`; `unknown` remains valid for canonical publisher models whose scope genuinely has no published limit.

A promoted executable slice entry (for example a Codex subscription model) is not implied by closure. `assets/model-data/current/generate.py` re-derives promoted limits, capabilities, and reasoning profiles from the closed provider-scoped record and refuses any promotion whose record is not `native-text-representable` or disagrees with the reviewed current definition.

Ordinary Local Control uses a shared 10 MiB bounded frame and carries the complete client-bundled current catalog. It must retain the inference-rule audit payload and every per-field provenance entry; a transport projection must not silently turn inferred values into apparently observed facts. `source_catalog_digest` binds the transported catalog to its sole current source artifact.

## Data-quality decision

Use a decision when a fact cannot be encoded as ordinary exact data. Allowed statuses are defined by `semantics.data_quality_status`. Every decision states the subject and the deterministic product behavior.

## Integration boundary

Metadata refresh may regenerate the existing current projections, but does not authorize runtime loader, compiler, persistence, or consumer changes. Issue #96 selected that integration contract. No old-data migration, compatibility catalog, edition selector, or dual-write implementation is implied.
