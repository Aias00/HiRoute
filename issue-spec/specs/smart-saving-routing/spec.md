# Smart-saving routing

## Purpose

Smart saving chooses between economy and primary model groups for one task scope.
It uses the shared decision/competence policy without exposing category branches.

## Requirements

### Requirement: Two groups are ordered relay lists, not escalation ladders

The editor MUST present judgment source, economy group, and primary group. Each
group has ordered availability-relay candidates; it has no independent competence
upgrade chain. New configuration favors a decision-model source but requires the
user to select a saved connection before publication. Users may explicitly choose
a custom extension or deterministic heuristic; absence of a connection cannot
silently change the configured mode.

Heuristic classification uses the existing deterministic English/Chinese policy,
generates no model-derived competence score, and hides probability/rubric controls
while retaining their parked edits. Built-in and extension modes share
[decision policy](../decision-services/spec.md); provider-specific hidden thresholds
cannot override the published plan.

### Requirement: Degree and competence thresholds are independent

Default simple probability threshold is 0.8 and competence floor is 0.5, both
finite within 0..1. Economy is selected when `P(simple) >= simple_threshold` and
there is no currently applicable complete assessment strictly below the floor.
Equality meets either threshold. Otherwise primary is selected with a distinct
complex-task, low-competence, or degree-unavailable reason.

Degree instructions and competence standards are separate. The default rubric's
incompetent/partial/competent anchors map to 0/0.5/1. Primary below the floor remains
primary; there is no third group. Missing/partial scores are unrated, not zero.
Judgment settings may be collapsed with a threshold summary but invalid hidden
fields must expand on publish failure.

#### Scenario: Exactly at both thresholds

- **WHEN** simple probability is 0.8 and a complete applicable score is 0.5
- **THEN** default policy MUST allow economy; strict low-score protection cannot
  interpret equality as incompetence.

### Requirement: Every new user turn can return to economy

New user input MUST receive a new judgment, potentially assessing the preceding
actual execution stage. No cross-turn one-way upgrade cursor survives. A previous
primary outcome does not prevent economy on a later qualifying turn, and old
observed low scores cannot be repeatedly reapplied. Tool continuations/replays in
one continuous turn inherit the frozen decision. A valid ContextHold MUST always prefer the
previously successful candidate inside the newly selected group, independently of
any saved follow-up field; it cannot preserve a candidate across a group change.

#### Scenario: A primary turn is followed by an economy turn

- **WHEN** a new user message qualifies for economy after an actual primary execution
- **THEN** the Gateway MUST select economy and cannot reuse the primary candidate through its hold.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/133#issuecomment-6220133330

### Requirement: Decision failure and candidate failure have different effects

An invalid degree selects primary with a judgment-failed reason, not an invented
probability. Whole decision-call failure uses heuristic fallback; source cancellation,
deadline, and input-integrity failure terminate the request. Candidate unavailability
relays economy candidates, then permitted primary candidates, then fails. Direct
primary selection cannot downgrade to economy. Relay cannot manufacture competence,
retry committed output, or add another fallback list.

#### Scenario: Economy is rate limited

- **WHEN** all economy candidates exhaust their permitted Key/availability relay
- **THEN** the configured primary group MAY serve the request, recorded as
  availability relay without a synthetic low score or rewritten degree judgment.

### Requirement: Presentation distinguishes selection from later assessment

Plan quality and session stage views MUST preserve actual economy/primary group,
model, native reasoning, position, opening probability/threshold, protection target,
and any subsequent availability relay. Later assessment belongs to the earlier
stage and its frozen rubric. Current drafts cannot reinterpret history; aggregation,
coverage, and drill-down follow [session history](../session-history/spec.md).

## Implementation references

- [Two-group policy](../../../crates/gateway/src/core_runtime/classification/group_policy.rs)
  and [policy scenarios](../../../crates/gateway/src/core_runtime/classification/group_policy_tests.rs).
- [Real multi-turn listener scenarios](../../../tools/e2e-harness/tests/p0_gateway_runtime/decision_branches.rs).
