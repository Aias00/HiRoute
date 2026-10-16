# HiRoute workflow supplement

Read with the generated issue-spec skills. Shared artifact rules, bounded reads and QUESTION commands remain in the generated resources; this file owns HiRoute's project policy.

- Use GitHub `higress-group/HiRoute:main` and the repository-selected issue-spec backend. A normal change can remain a simple Issue; use Proposal/Design/TASK/PROCESS only when their planning or coordination is useful.
- The current session owns authorized planning, implementation and self-tests. Use `hiroute-implementation-writer` for an assigned change. Do not create other sessions or reviewers as a workflow side effect.
- A coordinator is optional, only when the user explicitly designates one for cross-change scheduling and convergence. Its Codex-specific dispatch capability is excluded from Claude; shared planning and implementation do not depend on loading that skill.
- Keep each change within its declared ownership. A designated convergence owner finalizes shared manifests, lockfiles and generated registries; ordinary single-change work needs no separate coordinator.
- The human and GitHub own review, approval and merge. Report the actual candidate, evidence and remaining gaps. If the user selected a coordinated stage, use its agreed handoff and stage boundary; otherwise hand off the current change directly.
- Use current Issues/Designs and answers, repository specs, code and tests as requirements. Current requirements must be self-contained.
- Follow `AGENTS.md` for repository boundaries and task routing; use [HiRoute validation](../../.agents/skills/hiroute-validation/SKILL.md) for checks, execution, failure repair and evidence. Follow config rules for public content and record placement. Public records use English.
- HTML projections remain disabled by `html_review.enabled: false`; load their recipes only when enabled.
- Ordinary GitHub user discussions can use `issue-spec-github`. Selected issue-spec planning and typed artifacts use issue-spec's discussion commands. The repository marker explicitly selects GitHub; an authentication failure must not silently select another backend.
- Capture concrete CLI/workflow friction with [workflow improvement](../../.agents/skills/hiroute-workflow-improvement/SKILL.md): route by the owning layer, deduplicate and label suggestion Issues for periodic analysis. Continue the original task; feedback does not add a phase or authorize automatic implementation.
