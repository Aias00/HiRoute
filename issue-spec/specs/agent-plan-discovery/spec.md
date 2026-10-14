# Plan discovery for Agents

## Purpose

Agents discover named model routes or delegate explicit work by published purpose.
Model access and local Worker discovery use different authorization scopes.

## Requirements

### Requirement: Published invocation names select routes on shared service addresses

HiRoute MUST expose published plan invocation names as model names on the common
Gateway address, with protocol-specific paths rather than a separate address per
plan. Where the ingress protocol permits omission, an Agent's configured default
may resolve an unspecified plan. Explicit selection MUST validate current grant
membership and availability and cannot fall back to the default when unauthorized.

### Requirement: Model and Worker directories apply their respective authority

Model directories include only currently published routes authorized by that model
grant and reachable through the ingress protocol. Entries provide stable invocation
name and purpose; internal candidates cannot masquerade as permitted public routes.

Worker discovery is instance-scoped: every published enabled plan with delegation
on and one selected Harness is eligible for directory presentation, without caller-
Agent ID/allowlist or main-Agent model-protocol filtering. Entries carry published
purpose, frozen version, Harness, and ready/unavailable/unknown execution facts.
Missing installation may keep the plan visible with a reason; presence is not
execution readiness. Draft purpose is not published purpose, and purpose text is
selection guidance, not authority. Actual Start/Continue rechecks current eligibility.

#### Scenario: Revocation follows discovery

- **WHEN** a plan becomes disabled or closes delegation after a directory query
- **THEN** a new submit MUST fail its current admission check rather than treating
  the cached directory or Skill instruction as authorization.

### Requirement: Collaboration guidance queries current plans through public CLI

A user may enable the shared collaboration Skill on a capable main Agent while
retaining its native model provider. It teaches current plan lookup, purpose-based
selection, goal/context/constraints/acceptance input, submission, wait/read/result,
and exact Continue. It cannot hardcode long-lived candidate details, require Ultra
or native sub-Agent selection, or claim a text instruction technically intercepts
all tasks. Installation/removal uses the same owned reversible settings workflow in
[Agent connections](../agent-connections/spec.md); explicit/default delegation
trigger mode does not allow recursive Worker/coordinator delegation.

## Implementation references

- [Published grants](../../../crates/domain/src/agents/model_grant.rs).
- [Delegation admission](../../../crates/daemon/src/control/runtime/delegation_tasks.rs).
- [Public CLI collaboration workflow](../../../crates/cli/src/collaboration.rs).
