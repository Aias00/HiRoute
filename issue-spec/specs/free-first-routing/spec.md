# Free-first routing

## Purpose

Free-first mode tries explicitly free, usable sources before an optional configured
primary group. Subscription entitlement and unknown price are not evidence of free use.

## Requirements

### Requirement: Suggestions require eligibility before reference-score ordering

Automatic suggestions MUST start with connected sources that have explicit free
entitlement, usable credentials, protocol support, and required capabilities.
An unready source, missing Key, unknown price, or subscription alone is not free.
Users may instead select and order candidates manually.

Suggestions query one [reference-score snapshot](../model-catalog-price/spec.md)
for each model's actual native configuration. Known aggregate scores sort descending;
unknown scores remain unknown at the end, with stable identity resolving ties.
Discrete reasoning may suggest a real lowest setting; budgets/toggles cannot be
invented. Adoption is explicit. A later score/catalog refresh does not rewrite
saved order or include newly connected models automatically.

#### Scenario: Free model has no reference score

- **WHEN** it meets all hard source/entitlement/capability requirements
- **THEN** it MAY remain an eligible suggestion with unknown score, rather than a
  fabricated zero or the score for another reasoning setting.

### Requirement: Paid fallback is an explicit plan boundary

Without a valid hold, execution starts in the free group without task-complexity
classification. Disabling primary fallback produces a FreeOnly boundary:
exhaustion or request incompatibility fails with no primary/paid call. Enabling
fallback permits only the configured primary group under shared relay rules.

After an authorized primary candidate fully succeeds, continuous history may retain
that exact candidate while the plan still permits fallback. A prior hold cannot
bypass FreeOnly after fallback is disabled or authority/content changes.

#### Scenario: Free candidates cannot express the request

- **WHEN** a request needs a capability absent from every configured free candidate
- **THEN** it MUST fail in FreeOnly mode or use the explicitly permitted primary
  group; it cannot mislabel a paid request as free or silently discover candidates.

## Implementation references

- [Suggestion policy and scenarios](../../../crates/application/src/routing/suggestions/tests.rs).
- [Planner](../../../crates/gateway/src/planner.rs) and
  [ContextHold](../../../crates/gateway/src/context_hold/store.rs).
