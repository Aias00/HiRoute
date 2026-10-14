# Durable SPEC maintenance

Use when updating repository durable specs for an implemented change or repairing a stage-review finding. Preserve the configured issue-spec workflow and confirmed decisions; this guidance does not authorize an unrelated specification rewrite or bypass canonical validation.

## What to preserve

- Organize by lasting product scenario/capability under `issue-spec/specs`, reusing existing files before adding a new scene. Maintain the index when adding or moving a file.
- Extract confirmed requirements into testable behavior: user outcomes, authorization, data lifecycle, compatibility, meaningful failure behavior and necessary architectural invariants.
- Capture technical constraints only when they protect a lasting contract, not merely because the current implementation has a particular helper, database table or tuning value.
- Keep source Issue/typed-artifact references for traceability, separate from normative requirements. Use the workflow's durable-spec preview/apply/check facilities when applicable and inspect the resulting content; projection is not a substitute for semantic review.

## What not to promote

Do not paste whole Design/TASK/PROCESS records into durable specs. Exclude stage schedules, model/owner assignments, branch/worktree paths, candidate SHAs, one-off test logs, temporary mocks/workarounds, unapproved future scope and “finish this MVP” instructions. Such information belongs in change records or handoff evidence.

## Reconcile before handoff

Check the affected requirements against confirmed Design/answers, existing specs, code and executable tests. Do not weaken a requirement merely to match incomplete code or claim completion because its SPEC exists. If a conflict requires a contract decision, pause the affected change and request amendment; identify implementation gaps separately from expected behavior.

Preserve unrelated requirements and apply the agreed ownership/convergence protocol for shared SPEC files. In the normal handoff, list the updated paths and product behaviors, evidence locations and unresolved mismatches. Do not create a separate receipt or per-SPEC review gate. Human PR review checks the specification. A designated coordinator additionally checks cross-Proposal consistency only in explicitly coordinated work.
