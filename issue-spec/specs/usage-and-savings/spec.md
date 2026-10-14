# Usage and value

## Purpose

HiRoute reports known consumption and valuation from real Attempts. Cash cost,
subscription API-equivalent value, cache-token ratio, and evidence completeness
are separate measures.

## Requirements

### Requirement: Usage follows actual accepted evidence, including failed Attempts

Request, model, session, and home queries MUST retain available input/output/cache
usage under trusted traffic classification and current authorized scope. Known
consumption is not erased by unknown price, missing body, cancellation, or a later
Attempt's success. Verified connection-probe traffic is excluded according to its
actual classified identity, including failed probes, rather than query labels.
Repeated events or alternate representations cannot double-count a request.
Missing producer metrics are unknown, not zero. Numeric overflow/unrepresentable
storage or aggregate capacity failure reports an explicit error/incompleteness,
not a truncated plausible value.

### Requirement: Aggregates use a stable scope and currency basis

Home, session, and authorized queries of the same range MUST agree on usage and
amount. Daily results declare timezone and natural-day boundaries. Different
currencies cannot be directly summed. Pending settlement, partially known amount,
and confirmed complete value remain distinct. Ordinary CLI parameter defaults and
continued pagination preserve the same evaluated time scope.

#### Scenario: One relay Attempt consumed tokens before failing

- **WHEN** a later Attempt delivers the answer
- **THEN** both Attempts' known consumption MUST be included once with their own
  model/source identity, without claiming the failed Attempt was free.

### Requirement: Price evidence is exact and frozen per request

Manual source-specific prices take precedence over defaults and survive data
updates until explicit restore-default. Equal model names across provider/endpoint/
product do not share billing conclusions. Each new request freezes its effective
basis under [price policy](../model-catalog-price/spec.md); historical value cannot
be recomputed using today's prices.

Subscription API-equivalent value is available only with sufficient usage/reference
price and MUST be labeled an estimate with inspectable source/basis. It is not a
subscription bill, cash paid, refundable amount, or demonstrated savings. Missing
price is unknown, not free.

### Requirement: Cache ratio is token-weighted over valid known samples

Input cache-hit ratio MUST be `sum(cache_read) / sum(total_input)` over the same
eligible known sample set. Producer protocol semantics are normalized before
aggregation; cache writes are not hits. It is not request hit rate, ContextHold rate,
or cost savings. Missing/inconsistent/excluded metrics, true zero, zero denominator,
and partial coverage remain distinguishable. Valid token totals are not averaged
per request or assumed from absent fields. UI and queries use the same numerator,
denominator, exclusions, and completeness.

#### Scenario: Two requests have different input sizes

- **WHEN** one small request is cached and a larger request is uncached
- **THEN** the displayed ratio MUST use total cached/input tokens, not an average
  of their per-request percentages.

### Requirement: Long-lived aggregates cannot restore expired detail

Permitted aggregate usage/value may outlive seven-day request/session detail without
body, competence samples, or reverse access to deleted sessions/runs. Whole-instance
aggregate retention cannot manufacture precise expired run/session slices. Unknown
unsettled/unrecorded history remains unknown. A query crossing the detail boundary
must expose its available scope, and new-session default views use the bounded
seven-day window without extending detail retention.

## Implementation references

- [Value projection](../../../crates/observation/src/value/mod.rs),
  [usage archive](../../../crates/observation/src/valuation/usage_archive.rs), and
  [value contract scenarios](../../../crates/observation/src/store/tests/value_contract.rs).
- [Source price types](../../../crates/domain/src/compute/source_price/mod.rs).
