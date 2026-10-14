# Durable SPEC maintenance

Use when updating repository durable specs for an implemented change or repairing a stage-review finding. Preserve the configured issue-spec workflow and confirmed decisions; this guidance does not authorize an unrelated specification rewrite or bypass canonical validation.

Follow the shared [Durable contract maintenance](../../issue-spec-workflow/references/durable-maintenance.md) guide for merging confirmed changes into complete current contracts, choosing `current-spec` versus `inline`, and the limits of projection checks. The implementation owner performs this maintenance with the code; Proposal authoring alone does not publish planned behavior as a current contract.

## What to preserve

- Use the [specification index](../../../../issue-spec/specs/README.md) and [code map](../../../../docs/code-map/README.md) to locate lasting product capabilities and their owners under `issue-spec/specs`. Reuse existing files and requirements before adding a new capability. A consumer integration may update Agent connections, shared clients and request routing; it does not require one new file per Proposal SPEC. Maintain the index when adding or moving a file.
- Extract confirmed requirements into testable behavior: user outcomes, authorization, data lifecycle, compatibility, meaningful failure behavior and necessary architectural invariants.
- Capture technical constraints only when they protect a lasting contract, not merely because the current implementation has a particular helper, database table or tuning value.
- Keep source Issue/typed-artifact references for traceability, separate from normative requirements. For a selected Proposal projection, declare complete resulting requirements and use preview/apply/check; MODIFIED replaces all scenarios inside the target Requirement. For a simple-Issue change, use the ordinary repository edit/review path without manufacturing Proposal artifacts.
- Keep HiRoute's readable document titles and independent `Implementation references` sections. The CLI identifies capabilities by their path slugs and preserves those sections outside Requirement operations; do not rename titles to slugs or move references into the last Requirement. Inspect the preview for both retained references and correctly placed additions. Resolve unsupported structure before applying rather than dropping content or declaring a changed contract `UNCHANGED`.

## What not to promote

Do not paste whole Design/TASK/PROCESS records into durable specs. Exclude stage schedules, model/owner assignments, branch/worktree paths, candidate SHAs, one-off test logs, temporary mocks/workarounds, unapproved future scope and “finish this MVP” instructions. Such information belongs in change records or handoff evidence.

## Reconcile before handoff

Check the affected requirements against confirmed Design/answers, existing specs, code and executable tests. Do not weaken a requirement merely to match incomplete code or claim completion because its SPEC exists. If a conflict requires a contract decision, pause the affected change and request amendment; identify implementation gaps separately from expected behavior.

Inspect the old and new Requirement blocks, not only newly added text. Preserve applicable configuration ownership, recovery and rollback, routing continuity/isolation, security and data-lifecycle guarantees. For compatibility-sensitive changes, follow [compatibility maintenance](../../hiroute-compatibility/SKILL.md) and its register; a new consumer does not silently retire existing guarantees. Confirm that any removed condition has an authorized contract change, that other capability files do not duplicate or contradict the updated definition, and that validation commands/results remain in TASK or PR records. A passing projection check proves equality to the declared result, not these semantic properties.

Preserve unrelated requirements and apply the agreed ownership/convergence protocol for shared SPEC files. In the normal handoff, list the updated paths and product behaviors, evidence locations and unresolved mismatches. Do not create a separate receipt or per-SPEC review gate. Human PR review checks the specification. A designated coordinator additionally checks cross-Proposal consistency only in explicitly coordinated work.
