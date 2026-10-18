# Decision models for smart routing

[简体中文](smart-saving-model-classification.zh-CN.md)

Add a decision model or custom extension under **Models → Decision models**. Configure provider,
model, complete endpoint and credential, save and test, then select its saved revision in a route.
Saving/publishing a plan does not silently invoke the service. Credentials reuse protected input
and existing storage. Built-in providers need no separate Jev deployment.

Save-and-test checks the exact saved revision with fixed synthetic input, without reading real
conversations, executing a task or producing competence samples. Editing the connection creates
a new immutable revision; a published route keeps its selected revision until explicitly republished.
The list shows the latest revision, while an already saved historical revision remains publishable.

Smart saving defaults to the decision-model choice; users may explicitly choose heuristic rules or
a custom extension. Economy and primary groups retain the existing candidate picker, ordering and
reasoning controls. Judgment settings start collapsed: simple probability defaults to 0.8, competence
floor to 0.5, and both degree and assessment prompts are editable.

Every new user message decides again. Regular/economy requires this call's simple probability to meet
the threshold with no applicable complete low score. Complex or fresh low competence selects primary.
Only a fresh complete score bound to the preceding actual stage, in the same category and compatible
with the published configuration and assessment criteria, can trigger protection. Missing/partial
scores do not preserve old protection; new turns do not automatically reset to regular.
Tool continuation and replay reuse the frozen decision only when the same turn is identifiable,
history continues, and the decision remains reusable. Compaction or history reconstruction that
breaks continuity or prevents reuse triggers a fresh decision. Candidate hold stays inside the newly
selected group. Availability relay never downgrades from primary or crosses task categories.

Custom branches first classify task intent, then optionally judge degree within that category. Each branch
has regular candidates and optional primary candidates, and follows plan judgment or copies the complete
settings for independent editing. No primary means no degree question, while competence remains observable.
Category conditions, degree criteria and assessment criteria are separate inputs.

Existing plan/session views show actual group/model/profile, frozen thresholds and selection reasons
separately from latest competence. Draft edits cannot reinterpret published evidence.

For a built-in decision connection, **Decision protocol → C** explicitly enables
historical-problem applicability on that route. Existing routes and global creation
defaults retain their protocol. For each effective assessment scope, adopt the stock
C assessment or retain/edit a custom standard, expand its complete composed question
and three criteria, then confirm the review. Independent branch overrides are reviewed
separately. Adoption preserves degree/category text, candidates and numeric thresholds.
C still defaults to simple probability 0.8 and competence floor 0.5.

C supplies separate `prior_context`, `assessment_stage` and `latest_user` sections.
Background defaults to at most eight turns before the actual assessed stage; the
complete stage can be longer. Bounded interrupted/unknown background turns retain
their user requirements and recorded status; they do not certify a complete scored
stage. Published questions remain frozen when current defaults change. Zero background warns that earlier requirements and
references are absent. If the wire budget trims the target, the third question is
omitted and the partial score cannot trigger historical protection. Current input
is never silently truncated. Tool-result bodies are not added.

For an otherwise valid historical low-score protection opportunity, C retains that
protection when applicability is at least 0.5, and suppresses it below 0.5. An
unavailable answer retains protection conservatively. This does not override a
complex current task, transfer old scores or change same-turn availability relay.

**Test this route's C capability** sends one synthetic request using the exact saved
connection and frozen route content. Connection details also offer the full
nineteen-question shape. Missing evidence reads **Not tested in this session**;
changing the connection/content/shape invalidates it. The existing connection test
does not establish C support. Saving and publishing remain available without a
test, and neither action probes a provider.

Switching back requires a new assessment choice and review under the legacy wrapper:
restore an available retained pre-C rubric from the same stable scope, adopt the
legacy default, or review custom text. Unavailable/reclaimed history disables only
historical restoration. Rollback publishes a new revision and preserves old C evidence.
After rollback has completed, **Remove this review record** lets a later ordinary
legacy edit discard obsolete review provenance without changing the rubric or
thresholds. Keeping a review requires it to match the current composition. The
removal action cannot approve an unfinished rollback.
Plan/session quality includes opt-in C counts and coverage, using all retained matching
decisions rather than stage-opening samples. A retained decision with Plan identity
but no C metadata still leaves coverage incomplete; known counts remain visible,
while exact rates are unavailable. Narrow the revision/time scope when needed.
Categorical C requests use a custom question shape, even with stock base wording;
**Stock wording** does not claim the original experimental condition. These rates
describe behavior, not quality.

The CLI keeps `decision services list/apply/test` and the existing routing preview/apply workflow.
Editor data uses `smart.judgment`, or `branch_routing.judgment` with branch `primary_candidates` and optional
complete `judgment`. Custom extensions follow the supplied definition and rubric; their form provides
examples and the single current OpenAPI. No separate Secret-management UI is needed.

See [HiRoute extension API semantics](../decision-extensions/api/README.md), [optional self-hosted Jev deployment](../decision-extensions/extensions/jev-decider/README.md),
[System One mapping](../decision-extensions/api/system-one-design.md) and [code owners](code-map/decision-foundation.md).
Earlier development-only custom-extension shapes have no negotiation path; supported
released HiRoute data uses the registered admission/recovery readers.
Tool selection is documentation-only.
