# HiRoute Stage Review

Read at an authorized stage-review or integration checkpoint, not during routine dispatch and follow-up. This is a mode of the project coordinator, not a separate agent role. The coordinator performs review and reports to the human; the human need not participate in every inspection or repair iteration. Proposal owners implement and self-test, then receive consolidated repair requests.

Inspect the candidate read-only while forming findings. Later authorized integration, conflict resolution and assigned coordinator-owned repairs are allowed as separate operations; preserve the reviewed revision and recheck affected behavior on the resulting candidate. Do not edit a running owner's worktree under the guise of review.

## 1. Review the direction first

- Read the human-confirmed stage objective, each selected Proposal/current Design/effective answers, relevant durable specs and owner handoffs. Take the overall plan from the current assignment, selected Issues, and repository durable specs. Missing required intent needs an explicit amendment; do not depend on an external design archive.
- For each Proposal, ask: does it advance a required MVP user outcome; preserve agreed scope and cross-Proposal boundaries; leave the next stage usable; and avoid unnecessary implementation complexity? Check the stage collectively for missing responsibilities, duplicated ownership and conflicting contracts.
- Reject concrete scope drift or architecture that undermines the MVP. Do not demand hypothetical future generality, speculative rewrites or every possible edge case. A smaller implementation is preferable when its boundaries are sound and the current contract is satisfied.
- Resolve a material plan/Design conflict before accepting its affected implementation. Continue reviewing unaffected changes instead of blocking the entire stage.
- Review the combined durable SPEC deltas as a product contract: scenario-based organization, confirmed long-term behavior, coverage of changed user outcomes, no duplicate/conflicting requirements across Proposals, and a discoverable index. Keep necessary architectural invariants but reject stage logistics, TASK steps, temporary workarounds and unapproved future scope as normative requirements. Source links may retain history without turning it into product behavior.

## 2. Review correctness on the actual worktrees

- Identify each assignment base and exact candidate head; inspect the full change, not just the last commit. Record dirty work explicitly; a moving/uncommitted worktree may receive feedback but is not an exact final review candidate.
- Check the core production path, state/failure behavior, architecture and public interfaces, data/credential safety, resource bounds and direct regressions. Verify that Desktop and CLI reuse the intended shared Client Core, not duplicated product logic or test-only substitutions.
- Inspect supplied test evidence before rerunning suites. Verify actual revisions, non-zero selected cases, scenario states, production-vs-fixture claims and required gates. Rerun for missing/unreliable evidence, a concrete risk or a stated independent gate; do not duplicate every owner's full suite mechanically.
- Check changed SPEC scenarios against actual behavior and tests. A projected SPEC is not delivery evidence; code gaps do not justify weakening confirmed requirements. Return material omissions, contradictions or unsupported completion claims to the owner together with code findings, without a separate per-SPEC review loop.
- Cover the combined integration candidate after the coordinator assembles it. Individual branch passes do not prove cross-Proposal wiring, shared manifests, migrations or the final production path. Follow [HiRoute validation](../../hiroute-validation/SKILL.md) for evidence and affected generators.
- Check the stage DAG's promised user journeys on that combined candidate: identify remaining production-wiring owners, distinguish independent module delivery from stage acceptance, and release downstream work only when its actual prerequisites and phase authorization are satisfied. A deliberately deferred integration check remains an explicit gap, not a branch-level pass.

## 3. Converge findings

- P0/P1 must identify a concrete failure path, violated contract or harmful responsibility-boundary change. Include the Proposal/owner, reviewed revision, path/symbol or Issue anchor, impact, expected correction and focused verification. Speculative possibilities and style preferences do not become blockers.
- Report ordinary P2 briefly; do not enter repeated repair loops for polish. Real architecture defects remain blocking even for an MVP. Prioritize product direction, architecture and functional correctness rather than finding a quota of comments.
- The coordinator sends findings to the existing owner thread. Review its repair delta, unresolved findings and direct regressions; expand only for new material evidence. Preserve earlier evidence with its original revision and justified applicability.
- Finish with a stage-level verdict and per-Proposal status: direction, functional evidence, unresolved blockers, acceptable P2 and exact reviewed heads. State limitations rather than claiming untested behavior or treating `expected_red` as delivered functionality.
- Review does not grant merge/push authority. Use the user's existing authorization for routine repairs/integration and request decisions only where authority or material scope is missing; do not introduce an approval round for every TASK. If the coordinator authored substantive code, disclose the self-review limitation and its evidence in the stage verdict; do not call it independent review or automatically create another agent. The human decides whether additional review is needed.
