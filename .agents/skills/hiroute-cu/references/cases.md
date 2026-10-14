# User-confirmed regression cases

Read for recording CU findings, creating confirmed cases or running requested
regressions. A business scenario describes a capability; a case preserves one
confirmed failure and its repair acceptance without copying the whole happy path.

1. First record goal, state, shortest steps, expected/actual result, impact and
   local evidence. Mark suspected/reproduced/root-cause-unknown accurately; Jev
   cannot confirm a product defect.
2. Present a concise candidate list; create formal case files only after explicit
   user confirmation, which may cover several cases together. Unconfirmed items
   remain findings. Do not invent a confirmation date or person.
3. Saving a case does not authorize running it. Default to user-requested execution;
   do not add it to routine smoke, unrelated fix reruns or CI. “All confirmed
   regressions” covers all; a feature request selects that scope. Paid/system
   actions still follow current authorization.
4. Continue already authorized reproduction/fix validation without a new case gate,
   but do not use it to run unrelated history.

Save `cases/<goal>/<short-problem>.md` and link it from the relevant scenario's
on-demand regressions. Discovery is skill → index → scenario → selected case →
needed script. Do not create empty case directories or list/load every case.
Preserve failure evidence at its original revision; new results do not overwrite it.

A step may use a verified script or concrete Codex natural-language actions. State
action, parameter source, observable expectation and verification method. CLI and
readback may automate gradually but do not replace required real UI/model/task effects.

```markdown
# CASE-<goal>-<problem>: user-visible failure

## Confirmation and execution
Confirmation: actual message/reference and date
Execution: only when requested by the user
Scenario/Issue: links; failure revision and local evidence reference

## Goal, prerequisites and data
Original failure and required repaired business effect.
State, owned objects, parameter/credential references, authority and side effects.
Which prerequisites can be reused and how to verify them; missing means not executed.

## Steps and checkpoints
| Step | Execution | Action / parameter source | Expected result / verification |
| --- | --- | --- | --- |
| 1 | Verified script (entry/link) | Exact input and target | State/assertions |
| 2 | Codex CU | Concrete user action | Business result and evidence |

## Failure and recovery
Stop conditions, observe-before-retry and known cleanup/retention of owned objects.

## Evidence and automation
Script/manual status by step; actual revision/date/result/local trace.
Manual execution is not missing execution; a script pass cannot cover skipped steps.
```

Replace steps with scripts only after review and an independent rerun, preserving
case identity and business assertions. Extract ordinary scripts without introducing
a generic test DSL. More automation does not change user-requested execution policy.
