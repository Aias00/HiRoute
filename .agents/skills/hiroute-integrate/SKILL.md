---
name: hiroute-integrate
description: Integrate HiRoute task branches and converge their validation before handoff or an authorized merge. Use for combining branches or resuming integration after upstream updates; not ordinary feature implementation or standalone test runs.
---

# HiRoute integration

Use the repository's existing test planner and validation reports as the workflow state.
Run commands from the repository root. Read [test selection](../../../docs/test-selection.md)
and [validation routing](../../../docs/validation-routing.md) for scope and execution.

## Establish the candidate

Record the exact target baseline and requested source tips after fetching. Inspect the
checkout before branching or merging; preserve unrelated work. Resolve conflicts against
current requirements and both branches' consumers. Review each new upstream diff before
accepting it; an upstream update creates a new candidate, not evidence for the old one.
Use the configured code-host workflow for merges only within the user's authorization.
When the combined diff affects released data/configuration, recovery or public CLI/Gateway
behavior, use [compatibility maintenance](../hiroute-compatibility/SKILL.md) to check the
combined source-to-current path; individually passing branch tests may miss migration conflicts.

## Expose cheap integration failures early

For an SSH backend integration with repeated checks, retain the first managed
checkout with `--retention always` and a nonempty plan. Continue from the last
stopped run using `--continue-checkout LAST_RUN_ID`; keep its checkout owner for
final cleanup. This preserves Cargo artifacts across descendant candidate SHAs,
not earlier test verdicts. See the validation-routing guide for refusal conditions.

Run `python3 scripts/test-plan.py --base BASE_SHA --integration` after combining changes.
The top-level plan retains final obligations. `integration.commands` puts selected Python
gates and known coverage/protocol/runtime checks first, even when a lockfile selects full.
Inspect its paths and reasons: an empty early target list does not prove integration coverage.
Add missing consumers based on the diff; preserve required native/product acceptance cases.
Submit Cargo commands through `scripts/validation.py` on an exact committed candidate,
with a distinct `--plan` and `--phase focused`; this command only prints a plan.

Run the coverage gate before protocol/runtime checks. If generated data is stale, review
the source change and let the convergence owner regenerate it twice (second run zero diff).
A checksum refresh is not evidence that a changed assertion still represents the contract.
For affected Desktop journeys, check the configured Mac and required native interaction
capability before a costly build. Reuse an eligible managed Pilot build across scenarios.

## Diagnose once within the affected boundary

After failure, inspect the saved report/logs using [validation feedback](../../../docs/validation-feedback.md).
Record observed failures and unfinished checks. When multiple related failures may remain,
use `--integration --collect-failures` to propose bounded Cargo collection, review its scope,
and explicitly run it. It does not continue later commands or collect every workspace failure.
Do not add product smoke to diagnosis unless a product path or an unresolved consumer needs it.
If early coverage fails, repair it before continuing the behavior command on the new candidate.

Batch related repairs. Use `--since CHECKPOINT_SHA --integration` to obtain an iteration
preflight while retaining the original base plan and its Cargo package context. Keep original failures and linked run IDs.
Demonstrate the fix with the existing failing regression, or add a regression/selection mapping
when the earlier checks missed it. Preserve the failing package/feature context when relevant.
A same-SHA focused command may resolve different features from the full workspace command.

## Freeze and finish

After affected checks pass, freeze the code candidate and finish the selected final checks.
For queued SSH backend checks, use `backend wait RUN_ID --timeout 50` instead of
frequent `status` snapshots. If the shell tool yields a live session, continue that
same session to the bounded result within one orchestration call when available.
On `wait_state=timeout`, give a concise progress update and wait again; on failure,
read the report and relevant logs once. The wait summary is not a product scenario verdict.
When the selected final command is the full locked backend all-features test,
use `--schedule workspace-dag --exclusive` on the SSH runner. It prepares each
binary role once, runs ordinary targets alongside product preparation, and
checks the complete target and case-name set, doctests and three real smoke
cases. Include checkout, preparation, execution and reporting in the measured
time; preparation is not product success. A correctness pass over 300 seconds
still misses the performance target. `workspace-smoke` is an explicit rollback
entry, never an automatic retry after a DAG failure. Other commands keep their
ordinary execution path.

Link relevant runs with `--related-run`; do not automatically repeat the entire workspace
following each fix. Early checks are diagnostic and do not automatically waive final checks.
The small early boundary tests can recur in final workspace validation; expensive smoke
should not be scheduled in both phases without a concrete unresolved reason.

Keep a compact check ledger in the task's corresponding Issue or PR (raw logs stay in ignored local artifacts):

| Check / exact command | Source SHA | Run ID / evidence | Package, features, platform | Outcome / remaining obligation |
| --- | --- | --- | --- | --- |

Distinguish process exit from actual scenario states and unexecuted checks. Evidence reused
for unchanged code retains its original SHA and an explanation of the intervening diff;
SHA equality alone is insufficient. Docs/tooling follow-ups use their own checks. Assess
Top 3 timings once using the existing report; report unavailable timing rather than estimating.
Merge/push only when authorized, then verify the remote target contains the intended sources.
