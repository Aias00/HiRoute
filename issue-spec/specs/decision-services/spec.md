# Decision connections and custom branches

## Purpose

Saved decision connections supply task judgments and optional assessment of prior
execution. HiRoute owns category/group policy, candidate execution, and attribution.
Built-in System One connections and custom extensions share one current contract.

## Requirements

### Requirement: Connections are independently managed immutable revisions

Desktop's Models area MUST separate general models from decision models, including
clearly typed custom extensions, in the common list/detail flow. Built-in connection
configuration owns provider, model, full endpoint, auth, and timeout; category/degree/
competence instructions belong to the plan. OpenRouter, TypeSafe, Bailian Token Plan,
Bailian business-space, and compatible endpoints use the same System One codec.
Provider names alone do not define another protocol. Token Plan cannot be split
into individual/team eligibility paths. Known values are editable suggestions;
required business-space/custom endpoint/model fields remain visible.

Auth supports none, Bearer, or a declared header as appropriate. Plaintext uses
protected input/Secret transactions, never plan content, logs, argv, lists, or browser
storage. Existing auth shows configured state without guessing its type from the
header name. Changing provider or endpoint origin clears pending auth/old references.
Edits preserve input and do not reset a saved connection by changing its type.

Save uses service revision CAS and creates an immutable version; auth replacement
creates a new protected reference rather than modifying the old one. Plans may
explicitly select an available saved historical revision even after a newer save.
Publish verifies exact saved ID/revision/content and rejects unsaved or different
same-version copies. Connection/draft changes cannot reinterpret published plans.
Referenced versions/drafts prevent deletion. Inline legacy `rest` authoring is not
a current public alternative.

### Requirement: Testing is explicit, version-specific, and uses production transport

Save-and-test MUST save first, then test the exact saved version with synthetic
input through the actual transport/parser. Save-only makes no call. Test failure
reports that the configuration was saved but the test failed; it is not a rollback
or ongoing health/answer-quality verdict. Tests do not read real conversation history,
invoke a business model, or create competence samples.

HTTP/2 authority, total deadline, DNS/transport policy, authentication, and no-redirect
rules are shared with execution. Closed error codes distinguish recognized auth,
quota, and endpoint rejections without native text, credentials, account identifiers,
or request body. A generic rejection cannot be declared a bad Key.

### Requirement: A custom plan selects one category and at most two groups

Custom branches MUST contain 2–16 unordered categories, each with stable ID, name,
natural-language task condition, regular candidates, and optional primary candidates.
Primary presence enables the product's simple/complex degree question; a single-group
category needs none. `smart_saving` is reserved for the built-in task scope and is
rejected in custom authoring/materialization.

The protocol can express categorical choices and ordered levels, but the current
product offers only two degree levels. It does not expose arbitrary trees, parallel
category execution, category-confidence thresholds, priorities, or tool-selection
controls. Overlapping conditions select the current principal intent. No match
selects the explicit default category as a valid decision with its degree result;
this is different from decision-call failure.

Category/degree/rubric text must be legal nonempty text for publication without an
independent 4096-character field cap. Drafts can retain incomplete text. Whole-request
resource/transport/provider limits remain enforced without silent prompt truncation.

### Requirement: Group policy consumes only applicable current assessment

For a dual-group category, regular requires current simple probability at least its
threshold and no complete current assessment strictly below the competence floor.
Defaults are 0.8 and 0.5; finite 0..1 values and equality behavior follow
[smart saving](../smart-saving-routing/spec.md). Protection applies only to the
preceding actual stage when category, plan revision, and frozen policy/rubric agree.
Cross-category, partial, missing, malformed, mixed-model, or unbound assessment does
not drive escalation. Historical observation is not permission to reuse an old score.
Single-group categories record assessment without upgrading, and primary low scores
cannot create a third tier or change category.

Each new user turn rejudges category/degree; same-turn tool continuation/replay
inherits a frozen decision while history/authority remain continuous. A valid ContextHold
MUST always prefer its candidate within the newly selected group, independently
of any saved follow-up field; it cannot override a new category or group. Regular exhaustion may
relay into the same category's primary group; direct primary selection stays there.
No cross-category relay or separate failure list is added. Exact candidates already
failed in the same relay are not attempted twice. Model/provider failures do not
become competence scores. Actual category/group/index/Binding/profile identity is
recorded directly, including a model configured in both groups.

#### Scenario: A new user turn keeps the selected group

- **WHEN** a custom-branch follow-up selects the same group and its held candidate remains valid
- **THEN** the Gateway MUST prefer that candidate while retaining the fresh category/degree decision.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/133#issuecomment-6220133330

### Requirement: System One compiles independent typed questions in one call

Built-in request shape uses `model`, `state`, and `questions`. Categorical selection
is a Choice, each applicable degree is an independent Score, and prior-stage
competence is another Score. Instructions define each question's target without
relying on question-ID semantics or another answer. Only the selected category's
degree is consumed; invalid answers on unused paths cannot poison the chosen path.
Degree probabilities map by level ID. Competence is the raw 0..2 score divided by
2, not reconstructed from confidence or rounded probabilities. The three rubric
anchors remain 0/0.5/1. No absent free-text explanation is invented.

The call transmits current user input and the bounded safe visible history needed
for judgment/assessment to the explicitly selected endpoint. Current user content
cannot be silently truncated. History trimming marks the affected evidence partial;
full-history gaps and assessed-stage gaps remain independent. Missing pre-integration
prefix may set `history_partial` without making a completely captured target stage
partial; missing target response or target trimming still invalidates protection.

### Requirement: Custom extensions have one constrained v1 request/response

The current `/v1/decisions` contract MUST use `decision`, `latest_user`,
`visible_conversation`, `history_partial`, and `assessment_target`. The last field
is null or `{from, instructions, criteria}` selecting the retained suffix of one
actual stage. HiRoute binds internal execution identity without sending internal
model/stage IDs. Responses contain `decision` and optional `assessment`, constrained
to the published allowed choices. The extension implements its provider integration,
assessment, and context handling, never selects arbitrary models, branches, or tools.

The typed producer, parser, official extension, examples, and OpenAPI MUST remain
one current v1 shape with no old-format negotiation/dual path. Earlier development
structures are not supported migration inputs. Supported released data still uses
[registered compatibility](../../../contracts/compatibility-support.v1.json) rather
than treating a historical fresh-data exercise as permission to discard user data.
The API guide/OpenAPI supply complete wire schemas; the durable policy is here.

### Requirement: Conservative fallback preserves a valid selected category

A valid category with invalid degree uses that category's primary group with an
explicit degree-unavailable reason, not probability 1 or another default category.
A wholly failed call uses heuristic policy for smart saving, or the explicit default
category's primary group when present, otherwise regular. Valid decision plus
invalid assessment keeps the decision and discards only assessment. Cancellation,
source deadline, and replay integrity failures terminate instead of fallback.
One decision call uses the shared total deadline without a new retry/state machine.

#### Scenario: Review selected but degree is missing

- **WHEN** the response selects review, while writing is the plan default
- **THEN** HiRoute MUST select review's primary group and report degree failure;
  it cannot switch to writing or manufacture a probability.

### Requirement: Overrides, route editing, and protocol help preserve user intent

New connection flow from a route MUST preserve its draft, return to the exact saved
version, and never publish implicitly. Custom-category overrides copy the entire
current effective judgment settings, including thresholds, degree, rubric, anchors,
and supplemental assessment requirements. They become independent until restore-
plan-default clears the whole override without changing category/name/models.
Single-group categories hide inapplicable degree controls. Hidden invalid fields
expand on publication failure, and switching judgment mode preserves parked edits.

Extension help MUST show first-decision and prior-stage-assessment examples, request
copy, and the current OpenAPI download, without future tool-selection entries or
another Secret-management surface. The first response need not contain assessment;
later scoring remains attributed to the old stage. Plan/session presentation freezes
probability, thresholds, selection/protection/relay reasons, and actual positions,
using the same [quality queries](../session-history/spec.md).

## Implementation references

- [Decision domain](../../../crates/domain/src/routing/decision.rs) and
  [definition types](../../../crates/domain/src/routing/decision_definition.rs).
- [System One codec](../../../crates/gateway/src/core_runtime/classification/system_one.rs),
  [custom codec](../../../crates/gateway/src/core_runtime/classification/protocol.rs), and
  [policy scenarios](../../../crates/gateway/src/core_runtime/classification/group_policy_tests.rs).
- [Current API/OpenAPI](../../../decision-extensions/api/README.md) and
  [Desktop connection owner](../../../apps/desktop/src/features/decision-services/DecisionServicesPage.tsx).
