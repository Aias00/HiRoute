# Routing plans

## Purpose

An AgentPlan is a reusable named model route and, optionally, a delegated-work
configuration. It has a durable identity independent of any main Agent.

## Requirements

### Requirement: Plan identity is stable across presentation changes

A plan MUST have a display name, required purpose, and unique model invocation name.
The same published name serves model catalogs, Agent configuration, Gateway dispatch,
and Worker discovery. Display-name edits do not change an established invocation
name. New names are readable editable ASCII slugs derived locally from usable text
and deterministic unaccented transliteration, prefixed with `hiroute-`; only text
without usable fragments falls back to readable numbering. Default collisions add
a numeric suffix, manual collisions fail explicitly, and manual edits cannot be
overwritten by later display-name changes. A name race requires fresh confirmation.
Released `hiroute/<16hex>` names remain readable/callable without bulk renaming.

Agent model grants/defaults belong to [Agent connections](../agent-connections/spec.md).
Worker delegation has no caller-Agent allowlist. Enabling delegation requires one
selected Harness but does not decide which main Agent may delegate. Calling a model
alias MUST NOT implicitly spawn a Worker.

### Requirement: Authoring exposes four distinct modes

The editor MUST offer, in order, fixed model, smart saving, custom branches, and
free first, defaulting to fixed model. Fixed model supports one or more user-ordered
candidates without a second duplicate ordered template. A single candidate uses
the same identity, publication, and observation as a larger plan. A user may create
any mode directly without publishing an intermediate single-model plan.

Fixed and free-first execution use ordered relay. Smart saving and custom branches
share judgment/group policy but retain distinct task semantics. Candidate positions,
source, native reasoning, branch IDs/conditions, group selection, and failure
boundaries are explicit. Catalog, score, or price updates MUST NOT reorder published
choices. Suggestions require adoption; they are not runtime selection authority.

#### Scenario: Same model in two groups

- **WHEN** one Binding is configured in regular/primary groups with different
  reasoning or positions
- **THEN** authoring, reopening, execution, and evidence MUST retain each exact
  configuration rather than merge by model name.

### Requirement: Drafts preserve full edits and use exact revisions

All four modes retain their candidate arrays/settings when parked. Only the active
mode compiles. Legacy single/ordered editing adapts to one fixed ordered list,
without rebuilding order from scores, prices, or an automatic pool. Incomplete
text/budgets may remain in a draft but cannot publish.

Shared Application draft save/discard/consumption MUST use CAS on draft revision
and base plan head. Closing an unsaved new plan creates no empty plan, alias, or
publication. Publication consumes only its confirmed exact draft revision; a newer
draft survives. Direct publish may atomically consume that revision using the user's
updated input, without requiring old draft content to equal the submitted content.
Ordinary publication follows [control-plane admission](../control-plane-changes/spec.md)
without a redundant full Preview or diff dialog.

#### Scenario: Another client edits the draft

- **WHEN** Desktop publishes/discards a stale draft after CLI has saved a newer one
- **THEN** the stale action MUST fail or preserve the newer draft; it cannot consume
  a different editing intent under an old confirmation.

### Requirement: Native reasoning is an explicit plan choice

The editor MUST distinguish fixed behavior, toggles, discrete native settings, and
user-entered token budgets. It cannot invent a universal `ultra` level or silently
map between native scales. Suggested discrete settings may choose the actual
highest setting for primary work and lowest for economy/free work. A budget must
be supplied when the selected native capability requires it.

At execution, a plan overrides caller effort with that candidate's exact native
choice. Responses toggles render `reasoning.effort=low/none`; Messages manual toggles
render enabled with 1024 tokens or disabled without a budget. Explicit budget and
adaptive settings retain their own rules in [request routing](../request-routing/spec.md).
The registered Haiku 4.5 subscription toggle subset does not promise arbitrary
manual budgets: enabled is exactly 1024 and `max_tokens <= 1024` must fail rather
than grow the output budget. Released toggle records retain meaning through the
current conversion boundary without rewriting historical digests.

### Requirement: Published content and lifecycle heads are independent

An immutable content version MUST cover name, purpose, mode, every active candidate,
native settings, Worker/protocol, limits, and compiled content; a route-only digest
is insufficient. Publication is atomic: new requests use the new complete version,
in-flight requests and admitted tasks keep their old exact version/Harness.
Failure preserves the previous effective version and editable draft.

Disable/restore advances the lifecycle head while retaining content and invocation
identity. Content edits advance content revision and retain referenced versions.
Disabling the last callable plan produces durable NoNewCalls, including after
restart. Closing delegation rejects new exec/Continue, while model routing, accepted
runs, query/result/cancel remain available under current permissions.

Deletion rechecks Agent defaults/model scopes, version holds, preparation, and
recovery references under the writer/admission gate. Unknown references are not zero.
Deleted names are tombstoned and cannot be reused. Supported old plans adapt from
frozen compiled arrays/exact settings; the first protected save binds original
content and a recoverable migration record. An old writer unable to express the
current ordered semantics must request the current editing protocol.

### Requirement: Frozen model facts do not freeze compiler implementation

Durable content MUST retain candidate order, raw model/account/protocol, exact
endpoint/credential destination, capability evidence, and native reasoning.
Rebuild uses those choices, not today's catalog suggestions. Current compiler owns
conversion fidelity, streaming refusal handling, estimators, and reasoning
accounting/reservations; these implementation details do not redefine business
identity. Unrepresentable frozen intent fails explicitly instead of reselecting.

### Requirement: Candidate queries separate empty, unavailable, and failed reads

Routing options MUST allow a valid empty snapshot and expose unavailable selections
by stable Binding with safe display names/closed reasons: invalid ID, unready source,
missing qualification, incompatible catalog, credentials, or runtime preparation.
One excluded model cannot hide healthy siblings. Unavailable models are unselectable;
existing selections remain visible for repair. Successful reads grant no execution
rights, and publication still rejects empty/invalid configurations.

The released response remains unchanged unless `include_unavailable` is explicitly
requested, default false. Empty detail adds no field; current readers accept older
responses. Diagnostic detail may be capped at 256 with total/truncation declared,
without making available results fail. Subscription credential failure is source/
provider-specific and agrees with sign-in repair, even when saved status is Ready.
Runtime failure remains distinct. Classify outside the storage lock and never
persist query diagnostics as execution facts or mutate mode/Binding/revisions.

### Requirement: Context and request limits are visible, frozen settings

The common usable context upper bound MUST account for every active group's main
and fallback candidates, exact protocol limits, output, and reasoning reservation.
Without `limits.context_window_tokens`, the client window is
`min(272000, common_usable_upper_bound)`; explicit values are positive integers no
larger than that bound. This is a client window, not a rewrite of model capability.
Editor and generated Codex catalog MUST use the same aggregation and identify which
candidate narrows context/vision. Serial-tool limitations are separate product
constraints, not attributed to a candidate. Unknown required window/protocol facts
cannot produce a falsely complete catalog; expressible runtime forwarding does
not itself prove catalog vision support.

Context auto/custom and request-total-timeout controls remain visible. Switching
context to automatic and back retains the current custom edit; an empty custom
value fails rather than silently selecting automatic. UI shows actual window,
default, and common bound when known. Shared saved content includes these settings.

New request total timeout is 3,600,000 ms; stored plans retain their values. It is a
finite integer from 1,000 through 3,600,000 ms and not shorter than the attempt field.
`attempt_timeout_ms` retains its 30,000 ms default but does not currently control
Gateway write, first-byte, and stream-idle stages. Each stage defaults to ten
minutes and stays bounded by the total deadline; TCP connect is separate. Changing
this setting changes neither client timeout nor model output limits.

### Requirement: Delegation configuration remains independently usable

Delegation starts off. Enabling reveals the [supported Harnesses](../agent-delegation/spec.md)
with readable names/logos and required installation components. Selection is single;
Qoder/DSH need their native CLI, Pi needs selected CLI/Node, and adapter Harnesses
retain their own rules. Disabling hides but retains selection and must not start
new discovery from stale UI callbacks. Missing Worker dependencies MUST NOT prevent
saving/publishing an otherwise valid model route. Installation guidance is explicit,
copyable, platform-specific, and never auto-runs.

#### Scenario: Missing Worker dependencies do not block model routing

- **WHEN** Worker dependencies are missing
- **THEN** the user MUST still be able to save or publish an otherwise valid model route.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/133#issuecomment-6220134774

### Requirement: Installed consumer budgets are protected at publication

Current Qoder, Pi, and DSH supplementary model grants that survive the next
publication MUST retain their declared context/output budgets. Admission and the
sealed install checkpoint compare successful configuration operations, exact
currently active grant generation, retained routes, and authoritative next budgets.
Missing plan, uncomputable budget, or either smaller capacity blocks publication.
Equal/larger budgets pass. Historical or removed grants and collaboration-only
configuration cannot create consumer blockers.

`AgentModelBudgetConflictV1` MUST sort/deduplicate the actual Agent/context/plan/alias,
declared/proposed capacities, and capacity-reduced/plan-missing/budget-unavailable
reasons. All-Qoder blockers retain `QODER_MODEL_BUDGET_CONFLICT`; any other combination
uses `AGENT_MODEL_BUDGET_CONFLICT`. Brand affects presentation, not policy. Safe facts
survive compensation, terminal journal, and restart query. Error/Operation detail
is opt-in via default-false `include_model_budget_conflict`; released fields/position
arrays remain compatible and this flag enters no confirmation digest or authority.

#### Scenario: A Pi model still declares a larger window

- **WHEN** publication would reduce its surviving current grant's plan capacity
- **THEN** publication MUST fail with Pi's actual conflict and retain edits, with
  guidance to remove the model route, update the plan, and reconnect. Once the last
  effective consumer is restored/removed, stale historical configuration cannot
  continue blocking publication.

### Requirement: Follow-up continuity is automatic within the selected group

Desktop and CLI authoring MUST NOT offer a follow-up model-switching preference for
smart-saving or custom-branch Plans. Each new user message still rejudges the group;
a valid held candidate is always preferred only inside that selected group. The
custom default category remains a separate setting. Existing hold invalidation,
authorization, eligibility and availability relay follow request-routing.

Current editor and authoring contracts MUST accept an omitted
`reselect_on_user_message`. Supported released Plans, drafts, materialized
publications and CLI requests containing either boolean value MUST continue to
load, preview, save and publish. The legacy value has no routing effect and MUST
remain round-trippable where sealed digests or version consistency depend on it;
stores are not rewritten solely to drop it. The field is optional and deprecated
in current CLI schemas. Fixed-model, ordered and free-first behavior is unchanged.

#### Scenario: A route is edited without a follow-up control

- **WHEN** a user creates or edits smart saving or custom branches
- **THEN** Desktop MUST show no follow-up switching control or empty follow-up section, and new input can omit the legacy field.

#### Scenario: A supported old Plan is published again

- **WHEN** a supported released editor or draft containing the legacy field as true or false is reopened and published
- **THEN** the operation MUST succeed, preserve required sealed-data consistency and apply unconditional within-group continuity.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/133#issuecomment-6220134774

### Requirement: Native protocol adoption and rollback require effective-scope review

Native C MUST be an explicit route-level selection for smart-saving or custom
branches, never a connection/branch-judgment setting or global creation default.
Absent native_protocol means legacy. New optional protocol/review/detail members
MUST default and skip omission, preserving supported legacy drafts, authoring,
sealed identities and bytes without a bulk rewrite. C nested content requires
matching current consumers; unsupported content fails explicitly rather than
silently becoming legacy. Existing released admission/recovery adapters remain.

Every effective assessment scope MUST explicitly adopt stock C suffix/three
criteria or retain/edit a custom standard and review the full composed question.
Bind review to destination protocol and composition digest outside the C object.
Inherited branches may share their plan-default scope; independent whole-setting
overrides retain stable branch identity. Adoption changes only selected assessment
text/criteria and preserves degree/category text, numeric thresholds, candidate
order, unselected overrides and parked modes. Reject known incompatible history
field references with the affected scope/field. Save/publish freezes actual
effective text, thresholds, content identity and context cap, not a preset name.
Custom-extension/local effective modes omit parked native semantics; incompatible
effective C objects fail rather than changing those codecs. Restoring native mode
restores parked edits for review.

Rollback MUST also select and review each effective scope under the legacy wrapper.
A protocol toggle or C review cannot approve the C suffix under that wrapper.
Choices are: restore the latest retained published non-C assessment from the same
Plan before adoption, matched by stable scope; explicitly adopt the legacy default;
or retain/edit custom text and review its new composition. Historical selection
pins the exact source PlanExecutionRef/digest and scope in Preview/Apply; missing,
reclaimed or mismatched sources fail stale rather than substituting. A route born
in C, a new/removed scope or reclaimed history may have no source; disable that
choice with explanation while keeping default/custom choices. Copy only accepted
assessment instructions/criteria into a new current revision. No permanent hold,
backup promise, whole-Plan restoration or runtime dependency on the source row is
introduced. An already-published copied review remains usable after reclamation.

After rollback has completed, later ordinary legacy edits MAY explicitly remove
retained assessment-review provenance without changing judgment text or thresholds.
A present review still MUST match the current protocol and complete composition;
removal MUST NOT bypass any scope's adoption/rollback transition review obligation.
Desktop exposes removal for a retained published legacy review and omits an empty
review map after its last scope is removed.

#### Scenario: Edit after completed rollback

- **WHEN** a legacy route with a retained review later edits its assessment
- **THEN** A stale review MUST fail validation; explicit removal permits an ordinary legacy edit after completed rollback, but cannot approve a C-to-legacy transition.


#### Scenario: Bare rollback

- **WHEN** a published C route switches to legacy without a new scope choice and review
- **THEN** Preview/Apply MUST reject the transition while preserving editable input and the last effective publication.

#### Scenario: Exact retained restoration

- **WHEN** a user chooses a retained matching non-C source assessment and reviews its legacy composition
- **THEN** The new revision MUST copy only that assessment while preserving thresholds and unrelated settings.

#### Scenario: Reclaimed source

- **WHEN** the selected historical source is removed before Apply
- **THEN** Apply MUST fail stale/unavailable without substituting a different version or extending retention.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/139#issuecomment-6235110836

## Implementation references

- [Plan domain](../../../crates/domain/src/routing/authoring.rs),
  [draft workflow](../../../crates/application/src/routing/drafts.rs), and
  [editor](../../../apps/desktop/src/plan-editor.tsx).
- [Installed-budget guard](../../../crates/daemon/src/control/runtime/additional_model_budget.rs).
- [Publication/restart scenarios](../../../crates/daemon/tests/publication_process.rs).
