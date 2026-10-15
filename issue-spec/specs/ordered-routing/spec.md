# Fixed candidate order

## Purpose

Fixed-model mode executes one explicitly ordered list. It is not a separate
custom-category mode; a list may contain one or many exact source/reasoning choices.

## Requirements

### Requirement: User order is the execution baseline

The plan MUST retain user-chosen source, model, reasoning, and relative order.
[Request eligibility](../request-routing/spec.md) excludes incompatible candidates
without re-ranking the rest. Without a valid ContextHold, the first eligible
candidate runs first. A valid hold may prioritize the exact candidate that fully
succeeded within this published route; the remainder keeps its published relative
order. A new content version cannot inherit a hold for an old route.

#### Scenario: First candidate is incompatible

- **WHEN** candidate A cannot express the request but B can
- **THEN** A MUST make no upstream call and B is tried in the existing order;
  prices or reference scores cannot select an unconfigured candidate.

#### Scenario: A fallback produced the complete answer

- **WHEN** B fully succeeds and the next request appends continuous authorized history
- **THEN** B MAY be preferred through the shared ContextHold boundary, without
  rewriting the saved A/B order or carrying that preference into an unrelated plan.

### Requirement: Exhaustion is confined to the declared list

Key relay, failure classification, budget, deadline, and commitment follow the
shared request contract. Exhausting the configured list MUST fail explicitly;
HiRoute cannot discover another model, add smart-saving/free groups, or retry after
irreversible output delivery. Reordering requires a user edit and new publication.

## Implementation references

- [Frozen ordering](../../../crates/domain/src/routing/ordering.rs).
- [Planner](../../../crates/gateway/src/planner.rs) and
  [ContextHold](../../../crates/gateway/src/context_hold/store.rs).
