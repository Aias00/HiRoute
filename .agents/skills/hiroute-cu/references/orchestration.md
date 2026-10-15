# Orchestration and iteration

## Select the authorized workflow

| User goal | Workflow |
| --- | --- |
| Plan | Index → existing knowledge/gaps → scenarios, data, criteria and order; no app or Jev execution. |
| Mature smoke | Verify prerequisites → stable script → required business readback/call → current evidence; normally no Jev. |
| Immature feature / CU coverage | User goal and transitions → known direct/batch steps → unfamiliar read-only Jev exploration → Codex writes and business checks → stable extraction. |
| Changed feature | Select existing smoke by consumers → explore failure/missing branch → authorized repair → original failure and affected smoke. |
| Reproduce a report | Shortest steps and expectation → real entry → distinguish product, locator and environment; diagnosis alone does not authorize a fix. |
| Real E2E / release | Exact candidate, chain and authorization → smoke plus needed extensions → actual effects → findings and coverage; independent paths can continue after failure. |
| Extract a script | Review trace → stable segment → independent rerun from explicit prerequisites → update maturity. |
| Confirmed regression | User-requested scope → selected confirmed case → scripts plus explicit Codex steps → business verification; not automatic smoke/CI. See [cases](cases.md). |

Select by the requested goal, changed behavior, consumers and prior failures.
Save/authentication/enable/recovery changes retain relevant retry, disable and
actual-access assertions. Calls/tools/task changes retain real execution and
artifacts. Explain uncertain scope and expand the affected chain; do not assume
full-product coverage or arbitrarily shrink acceptance.

## Cost and evidence

Split each chain into preparation, stable UI, write/system boundaries and effect
checks. Reuse unchanged, verified prerequisites instead of reinstalling or paying
for the same call in every scenario. Current upstream artifacts can support
current downstream observation with recorded links; old-version fragments do not
prove a current complete chain.

- Batch multiple stable steps in one host call, still observing and checking each
  step. Return at business checkpoints, errors and authorization boundaries.
- Validated writes may batch only within current authorization, exact parameters
  and before/after checks. Codex handles unknown writes; Jev remains read-only.
- Use UI for the user action; configuration readback, logs, real model output,
  files and tests establish its effect. A toast, HTTP 200, exit code or mock is
  insufficient and CLI/API does not replace the required UI.
- After save/publish, wait for the matching Operation terminal state and read
  back the target revision/effective configuration. Dialog closure is not success.
  Check request/submission timing for a protected publishing window. Observe an
  unknown submission before retrying. Continue uses the task's frozen route and
  native session; do not change the current plan to bypass that contract.
- Return status, counts, elapsed time, suspected issues and local trace paths
  first. Read relevant AX/screenshots on failure; visual claims require images.
- Record preparation/execution cost, Codex tool round trips, Jev calls/available
  usage, retries and takeovers, including failed exploration and script work.
  Round trips are not tokens and account usage is not task cost. Mark unavailable
  measurements unknown; no invented saving percentage.
- Compare only the same version, prerequisites, assertions and completed scope;
  distinguish cold start from reuse. A navigation trial is not an E2E benchmark.

## Scenario ownership

Use one ordinary Markdown file per independently testable user goal. No new
schema, registry or DSL. Include goal/scope, start state, parameter/data sources
and authorization; transitions and observable exits; script/Codex/Jev boundaries;
minimum smoke and change-selected extensions; stop/recovery behavior; current
run evidence or an explicit not-executed statement. Product semantics reference
durable specs instead of creating a second contract. Do not freeze personal
HOME/accounts/ports/coordinates or dynamic AX indices into scenarios.

## Prepare the actual environment

Read the host's current Computer Use documentation before controlling the app.
Check checkout, packaged revision, live app/daemon and data roots; package time
alone is not source identity. Read relevant installation/troubleshooting guides,
and validation/Pilot guides only for that execution mode. Missing environment is
a gap, not permission to replace UI with API or fixtures.

For macOS firewall blocking at new build paths, use
[fixed-path launch](../../hiroute-macos-launch/SKILL.md), including the user's
initial allow prompt and actual Desktop/daemon/CLI/CLIProxyAPI checks. Real HOME,
paid and system actions stay within current authorization. Protect the main
agent's control connection and prepare a verified recovery entry when modifying
it. Pilot retains its restriction against real secrets.

Use files/CLI/logs for synthetic preparation, diagnosis and cross-checks. Editing
a database to bypass a failed save is not acceptance. Preserve a short failure
reproduction and raw local evidence, continue independent scenarios, and repair
only within authorized scope. Reconcile SPEC conflicts at their single owner.

## Maturity and results

Needs exploration means no complete real-path result yet. Explored means one
stated scope has been executed; repeatable means explicit data/prerequisites,
criteria, stop/recovery and an independent rerun exist. Downgrade affected
criteria when UI/contracts invalidate them, preserving original-revision evidence;
do not downgrade unchanged scenarios without cause.

Each run separately records green, product failure, verified expected rejection,
environment blocked or not executed, plus unresolved suspicion and script/locator
failures. Navigation maturity does not apply to model or worker behavior.
Keep trace files local and put concise revision-bound results in the Issue/PR.
Never splice earlier package runs into current core acceptance. Documentation-only
changes use link/skill/selection checks; execution scripts require behavior tests.
