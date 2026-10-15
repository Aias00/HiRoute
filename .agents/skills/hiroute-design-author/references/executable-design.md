# Executable Design and TASKs

Use when preparing a detailed handoff for bounded implementation. The objective is fewer consequential decisions during implementation, not more pages or pseudocode for every private function. The contract and acceptance quality are independent of the model selected to execute them.

## Put information in its owning artifact

- Proposal: capability baseline, user outcomes, scope and non-goals; SPEC: the required product increment and observable behavior, following the shared [requirement scope guide](../../issue-spec-workflow/references/planning-scope.md).
- Design body: architecture, shared contracts, state/data ownership, significant failure behavior, settled decisions, overall verification strategy, and a short TASK/dependency index. Give each TASK a descriptive purpose and exact locator so discovery does not require opening every comment.
- TASK: one coherent implementation or verification unit with a verifiable result and coverage of the relevant product SPECs. Several TASKs can belong to the same owner; they do not imply additional agents, worktrees or reviews.
- QUESTION/ANSWER: genuine unresolved contract decisions and their resolution, not routine private coding choices.
- Repository fixtures/goldens: large executable examples, precisely linked by path and known revision. Copy required contract facts into Design/TASK; an external archive is not an implementation prerequisite.

## TASK content

Use these fields where relevant; omit inapplicable detail rather than manufacture work. Preserve the CLI's canonical TASK template and relationship fields.

1. **Outcome and boundary:** what must work, what is excluded, owned paths and shared-file writer.
2. **Starting evidence:** refine the Proposal's capability baseline into the inspected code revision, exact entrypoints/types, reusable capabilities and remaining gaps for this TASK. Do not invent symbols or require the owner to rediscover the architecture.
3. **Contract:** inputs, outputs, errors, state/persistence, compatibility and security invariants. Link the owning shared decision instead of maintaining duplicate definitions.
4. **Work sequence:** a short dependency-ordered sequence of changes; identify unavailable peer implementations and allowed contract-level test doubles.
5. **Verification:** concrete normal, failure and integration cases, required environments/platforms, relevant commands/test locations, and expected results/evidence, selected through [HiRoute validation](../../hiroute-validation/SKILL.md). Distinguish component evidence from production-path acceptance. Keep these execution requirements in TASKs rather than Proposal SPECs. A test that only confirms the proposed helper shape is insufficient.
6. **Escalation boundary:** which discoveries invalidate the design or require a shared decision. Routine helper/layout choices stay with the owner; only affected work pauses.

Split a TASK when it spans unrelated contracts or cannot be read and verified as one unit, not at an arbitrary word count. Do not introduce extra phases, reporting systems or per-TASK approval gates.

## Readiness for bounded implementation

Before handing off, verify that the actual code supports the proposed entrypoints, shared interfaces are coherent, acceptance can catch plausible wrong implementations, and production wiring has an explicit owner. Flag uncertainty instead of hiding architectural research in a supposedly mechanical step.

Describe whether the work is mostly frozen-contract implementation or still needs semantic/security/concurrency design. Report the required judgment and remaining uncertainty so the user or authorized coordinator can choose a model and effort within the user's preferences and budget. Complex work is not made mechanical merely by adding detail. Resolve consequential uncertainty in Design before the affected implementation, or bring the unresolved choice to the user or designated coordinator. Do not silently switch models or create a second implementation agent.
