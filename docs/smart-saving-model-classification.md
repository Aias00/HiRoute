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

For a built-in decision connection, choose **Historical problem applicability** on
that route. This temporary authoring selector changes the draft only; existing
routes and global creation defaults retain their behavior. On publication, Preview
lists every transitioning assessment scope with the complete server-composed
question and three criteria. Choose the destination default, review and retain
custom text, or restore a retained historical standard when rolling back. Check
the final Preview and confirm publication. Independent overrides have stable scope
IDs; inherited branches share the effective default. The server copies only the
selected assessment text/criteria, preserving thresholds and unrelated settings.
Simple probability defaults to 0.8 and competence floor to 0.5.

Review provenance belongs to that immutable published revision and is available
through Plan reads. Editor state carries no review record or removal action.
Ordinary edits within the same protocol need no protocol re-review. New scopes and
protocol rollback still require explicit choices; mode/draft changes cannot approve
them. Missing/reclaimed history disables only historical restoration. Incompatible
context references and dangling stock applicability text produce scope diagnostics.
Published execution remains frozen when current defaults change or the temporary
selector is eventually retired.

The applicability protocol supplies separate `prior_context`, `assessment_stage`
and `latest_user`. Background defaults to eight whole turns before the actual
assessed stage, preserving bounded interrupted/unknown turns and their status.
The complete stage can be longer. Zero background warns that earlier requirements
and references are absent. Wire-budget trimming marks the target partial and
omits the third question without refilling history; partial scores cannot trigger
historical protection. Current input is never silently truncated; tool-result bodies
are excluded.

For an otherwise eligible historical low-score protection opportunity, applicability
at least 0.5 retains protection and a lower value suppresses it. Unavailable answers
retain protection conservatively. Attribution, version, category, score/rubric
validity, same-turn inheritance and availability relay retain their existing rules.

Each context offers one explicit diagnostic action with a selected shape. A saved
revision can test connectivity or historical problem applicability, including the
nineteen-question maximum. Route tests use the exact frozen route content.
**Not tested in this session** remains visible without evidence; changing
connection/content/shape discards it. Results state the verified shapes. Each test
sends one synthetic request and may consume provider quota. Saving and publishing
remain available without testing and never probe implicitly.

Plan/session quality includes counts from all retained matching decisions,
independent of stage pages and score/model filters. Current events mark all four
classifier protocols; known non-applicability decisions are counted separately and
do not make mixed scopes partial. Unmarked old events, missing projections and
bounded scans remain coverage gaps: counts stay visible, while exact rates are
unavailable. Content-only deletion keeps safe evidence; full fact deletion/expiry
removes it. Categorical requests seal a custom standard before hashing; only the
exact stock ordinal question/criteria shape is labelled default. Rates describe
routing behavior and do not claim accuracy or quality improvement.

The CLI keeps `decision services list/apply/test` and the existing routing preview/apply workflow.
Editor data uses `smart.judgment`, or `branch_routing.judgment` with branch `primary_candidates` and optional
complete `judgment`. Custom extensions follow the supplied definition and rubric; their form provides
examples and the single current OpenAPI. No separate Secret-management UI is needed.

See [HiRoute extension API semantics](../decision-extensions/api/README.md), [optional self-hosted Jev deployment](../decision-extensions/extensions/jev-decider/README.md),
[System One mapping](../decision-extensions/api/system-one-design.md) and [code owners](code-map/decision-foundation.md).
Earlier development-only custom-extension shapes have no negotiation path; supported
released HiRoute data uses the registered admission/recovery readers.
Tool selection is documentation-only.
