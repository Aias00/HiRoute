# Exact-commit client discovery

Client repositories can accelerate model-metadata maintenance. This mode produces reproducible source records whose fields are usable wherever their completeness satisfies a scenario; the repository's source type is not a confidence tier.

## Trust boundary

- The scanner reads only objects reachable from the supplied 40-character commit using `git ls-tree` and `git show`.
- It does not import, build, run, or install code from the target repository.
- A dirty checkout is acceptable because working-tree bytes are ignored. The requested commit must already exist locally.
- Every extracted assertion retains its repository-relative location and source-blob identity.
- `evidence_eligible` is false because discovery runs are tracked as provenance rather than catalog `evidence_sources`; this does not restrict field-based usage.
- Embedded third-party aggregates, mirrors, defaults, fallbacks, tests, fixtures, and comments remain discovery-only.
- Assertions are retained verbatim in `provider_metadata_records` and `model_metadata_records`. Each record derives its own completeness and usage scenarios; execution qualification and full pricing facts have additional operational/schema requirements.

## Identity normalization path

1. Scan the selected client at an exact commit.
2. Review lexically unmatched model IDs, provider profiles, endpoint locators, lifecycle hints, and metadata-shape signals.
3. Select only candidates relevant to HiRoute's current catalog.
4. Resolve identity and scope with enough identity fields to distinguish publisher model, hosted offer, rolling alias, and immutable revision.
5. Add the authoritative locator to `source-registry.json` and `catalog.evidence_sources` when needed.
6. Add or amend the normalized entity with evidence refs. Preserve unknown, conflict, conditional, and runtime-required states.
7. Re-run catalog validation. Identity normalization does not alter the provider-scoped record's completeness-derived usage.

## Metadata materialization path

1. Scan each client at an exact commit and review excluded inputs.
2. Run `materialize_client_metadata.py` with all selected scans and the current catalog.
3. Keep provider and model records scoped by client profile plus client provider ID; do not deduplicate them into a guessed global identity.
4. Preserve raw source assertions and Git object locations alongside normalized fields.
5. Derive provider completeness for identity, endpoint, authentication, and discovery; derive model completeness for identity, limits, modalities, capabilities, reasoning rendering, lifecycle, and cost.
6. Derive `usable_for` from those domains. Do not add or remove a scenario because the source feels more or less trustworthy.
7. Keep execution qualification and fully scoped pricing facts separate because they require operational results or additional product/currency/unit/schedule fields, not because the source has a lower confidence label.

## Profiles

Profiles live in `client-source-profiles.json` and bind a repository identity to narrow parsers:

- `openclaw-plugin-manifests` parses only `modelCatalog` sections from provider plugin manifests. It extracts provider/model assertions, dynamic discovery modes, suppressions, and model-related HTTPS locators.
- `hermes-python-profiles` parses Python syntax without executing it. It extracts literal `ProviderProfile`-style arguments including `api_mode`, literal fallback/default model IDs, and model-related HTTPS locators from the named paths.

Exclude bridges whose only role is relaying another aggregate. If a repository changes its layout or schema, update the profile or parser deliberately; do not silently broaden the scan to the whole tree.

## Output contract

The scanner emits `hiroute.model-metadata-client-discovery` with:

- exact repository URL, commit and commit tree;
- the profile and scanned blob inventory;
- provider, model, lifecycle, locator, and excluded-input candidates;
- lexical matches against the current HiRoute catalog, when supplied;
- a policy block that declares completeness-based scenario eligibility and prevents unreviewed identity inference;
- a canonical SHA-256 digest over the artifact excluding the digest field.

The output is deterministic for the same repository objects, profile file, catalog bytes, and `--as-of` value. A changed assertion is a review signal. Materialization preserves it in the unified metadata-record schema and recomputes field completeness and scenario eligibility.
