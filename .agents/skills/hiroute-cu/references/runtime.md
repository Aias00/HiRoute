# Shared runtime: batch CU and read-only Jev

Load for running or maintaining scripts. Node 22+, no third-party npm dependency;
the host provides CUA. Use the same `loop.mjs` observation, checks, budget and trace.

## Stable batch execution

First read the currently available Computer Use plugin instructions and initialize
its documented entry. Missing `cua_repl` does not establish that CU is unavailable.
For a host providing Node REPL and `@oai/sky`, its documented entry is:

```js
globalThis.sky = (await import('@oai/sky')).sky;
nodeRepl.write((await sky.get_app_state({app: 'HiRoute'})).text);
```

Read the returned state before acting. Resolve `skillRoot` to this checkout's
canonical skill. All UI calls stay in the host REPL, not a separate terminal UI process.

```js
var loop = await import(`${skillRoot}/scripts/loop.mjs`);
var nav = await import(`${skillRoot}/scripts/nav-codex.mjs`);
var trace = await import(`${skillRoot}/scripts/trace.mjs`);
var budget = loop.createBudget({maxSteps: 3, maxCalls: 5, timeoutMs: 45000});
var options = {
  driver: loop.createSkyDriver(sky, 'HiRoute'), appName: 'HiRoute',
  goal: 'Open Agent then Codex details, without changing any configuration.',
  scenario: nav.scenario, budget, select: nav.select,
};
var preview = await loop.runTask(options); // dryRun defaults true; no API call
var previewPath = await trace.saveTrace(preview);
nodeRepl.write(JSON.stringify({status: preview.status, reason: preview.reason, trace: previewPath}));
// After host verification:
// var result = await loop.runTask({...options, dryRun: false});
// var resultPath = await trace.saveTrace(result);
```

One host call may run several steps, observing/checking each. `select` receives
the current projection and returns only `{action, targetIndex}` or null for a
missing/ambiguous target. It requires no invented model confidence. No Jev fallback.
For a host actually providing `cua_repl`, initialize `app = await cua.getApp('HiRoute')`
under its instructions and explicitly use `loop.createCuaDriver(app)`. The drivers
map the same interface, never auto-detect/retry/switch. Sky extracts full
`AppState.text` with `disableDiff: true`, not the screenshot as AX. An app name,
bundle ID or absolute app path can be used; observation may launch it.

## Explicit Jev exploration and screening

Read [Jev use](jev.md), remove `select`, and supply `decide: input => jev.decide(input)`
from `scripts/jev.mjs`. It reads `OPENROUTER_API_KEY` or the ignored skill-local
`.env`, or accepts explicit `apiKey`. Exactly one of `select`/`decide` is required.
Batch mode does not import Jev or require credentials. Jev permits only
`click_element`/`scroll`, and every projected candidate must be host-marked
`readOnly: true`. Batch may use `set_value` within authorization and checks.
Other actions, coordinates and free keystrokes remain direct Codex CU.

Add `scenario.review` explicitly for screening, e.g. `{...nav.scenario, review: nav.review}`.
Pure navigation without it cannot claim interaction-defect screening. Batch ignores
review without extra model cost. `review({ax, baseline})` returns only an
`expectation` (1–1500 characters) and `facts` (1–12, each 1–500 characters), checked
by Codex for external disclosure. The loop adds the latest actual semantic action
and preceding projected facts (null initially), never raw history. `f0` IDs refer
to current facts.

Assessment must pass before action: normal confidence at least 0.7, no anomaly;
suspicions require current fact IDs. Independent contradictory answers also stop.
The threshold is a takeover policy, not measured detection accuracy. Loading,
unknown or suspicion returns to Codex; no automatic effect retry. Deterministic
checks still take precedence. After the last action or an initial endpoint, one
assessment-only call checks fresh state; changes stop instead of returning stale
success. Screening therefore normally takes N steps/N+1 calls; reserve endpoint
budget. Insufficient budget means incomplete, not green. Refined attempts reuse
the original budget and retain each trace and rationale without changing the goal.

## Scenario interface

| Field | Contract |
| --- | --- |
| `allowedActions` | Selected click_element/set_value/scroll subset; Jev cannot set_value. |
| `project({ax, baseline})` | `{candidates, context}` with real current index/role and needed labels: 1–40 candidates, label ≤160 and context ≤1500 characters. Jev candidates require readOnly:true. |
| `resources({ax, element, kind})` | Optional host-owned exact text/direction; selectors cannot override values. |
| `beforeAction({ax, baseline, action, element})` | Literal true only for allowed stage, actual control and exact parameters. |
| `afterAction({before, ax, baseline, action, element})` | Literal true only after expected effect/invariants. |
| `verify({ax, baseline})` | Real endpoint check; model-reported done cannot override it. |
| `review({ax, baseline})` | Optional bounded expectation/facts for Jev, ignored by batch. |
| `progressKey(ax)` | Optional stable progress projection; default ignores AX indices/focus. |

Codex defines checks; Markdown does not generate them or prove read-only behavior.
`baseline` is this run's initial AX; cross-segment business snapshots belong in
scenario closures. Refresh after selection; a changed view stops. Observation and
native click are not an OS-atomic transaction.

## Budget, takeover and traces

Steps, API calls and absolute deadline accumulate together. Batch uses no API
calls but still obeys time/step/check limits. Jev's default 15-second API limit
includes response-body reading; the whole scenario also has a deadline. Two
no-progress rounds stop. `busy` prohibits reentry while the original call remains
active. Only Codex reconciliation may clear `needsReconcile`. No hidden budget
reset, rollback or retry after error/cancellation/timeout.

Each result contains mode, goal, appName, status, reason, verified, cumulative
counts and elapsedMs. Full local `observations` retain AX/stages without redaction
or truncation. `events` retain context, semantic target, exact parameters,
started/returned action and check results, including skipped/failed actions.
`assessments` retain categories/confidence, expectation/facts/evidence IDs,
last action and observation links; they are leads, not product verdicts.
Valid Jev usage retains input_tokens/output_tokens and latencyMs. Missing usage
is unknown, not zero; these values do not include Codex or the whole task cost.

`saveTrace(result)` writes a new private temporary directory/file (0600), or an
existing owned evidence directory, never the repository by default. Return compact
status/counts/path and read relevant portions rather than printing or sending full
results. Authentication and arbitrary external error bodies are not additionally
collected. Full trace means actual UI observations/actions, not unrelated processes.

Reset the host REPL after source changes; query-import cache bypass is unsupported.
Native macOS drivers have tests; report other platform/browser coverage from actual
runs. Offline checks: `node --test .agents/skills/hiroute-cu/tests/*.test.mjs`.
A fake driver pass does not replace a real NAV-02 run.
