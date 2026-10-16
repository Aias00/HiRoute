# HiRoute CLI

HiRoute CLI is the official terminal entry to the HiRoute local service. It can connect to Desktop or serve as the complete management interface for Linux headless. Model sources, smart routes, Agent connections, session observation, and Worker tasks reuse the same production paths as Desktop; there is no second headless control plane.

## Install and inspect the CLI

- macOS Desktop: open Settings → CLI → Terminal entry and select Install.
- Linux headless: follow [Run HiRoute headless on Linux](/en/docs/install-linux/) to use the website's one-line installer, then start the service explicitly.

If the entry directory is not on PATH, add this line to your shell configuration and open a new terminal:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Inspect the entry and released commands:

```sh
hiroute --help
hiroute schema list --output json
hiroute schema show --command-id worker.exec --output json
```

The CLI entry and local-service availability are separate states. If a command reports that the service is unavailable, Desktop users should open the app. Standalone users should run `hiroute service status`, then `hiroute service start` when needed. The CLI never starts the service or replays a failed request automatically.

## CLI Quickstart: connect, inspect, and adjust

The same path works from a terminal or any Agent with the HiRoute management Skill. The caller can manage a different Agent; its own model need not use HiRoute. Model routing, management and task delegation remain independent.

```sh
hiroute quickstart
hiroute quickstart --context CONTEXT_ID --plan PLAN_ID
```

Quickstart is read-only: it shows the current service, exact targets, reusable sources/Plans, missing choices and next commands. It does not start a service, import credentials, save changes, restart an Agent or call a model. For a specific Hermes configuration, add `--hermes-root /ABSOLUTE/EFFECTIVE/ROOT`. Preserve a missing selected root; do not silently substitute another profile.

1. Reuse a saved source from `compute list`, or scan a connection point and explicitly prepare its credentials with `compute connection prepare`. Use `compute scan --request-stdin` with `{"schema":"hiroute.compute-scan-request/v2"}`; add the selected typed `native_target` for Hermes. The returned candidate contains its exact reference/revision and selectable models.
2. Preview the selected models, then save the generated request:

   ```sh
   hiroute compute connection preview --candidate CANDIDATE_REF --candidate-revision REV --all-selectable --apply-file source-apply.json
   hiroute compute connection apply --request-file source-apply.json
   ```

   Use repeated `--model-ref REF` for a subset. `--all-selectable` freezes concrete current refs; zero selectable models blocks saving. Preparation, save, provider checks and real native requests are different effects.
3. Select returned binding IDs with `routing options --request-stdin` and `{}`. Create a fixed route with `--mode fixed_model --model BINDING_ID`, or go directly to smart saving:

   ```sh
   hiroute routing preview --name 'Economy work' --purpose 'Routine and complex development' --mode smart_saving --economy ECONOMY_BINDING --primary PRIMARY_BINDING --decision local_rules --apply-file route-apply.json
   hiroute routing apply --request-file route-apply.json
   ```

   Replace all example IDs with current returned IDs. Repeat group flags for explicit order. Choose `--decision local_rules` or `--decision-service ID@REVISION` explicitly; an ordinary Chat model is not a decision-service connection. Shared backend defaults fill the editor. To change one choice, use `routing preview --plan PLAN_ID` with only that change and a new Apply file; other saved settings are preserved.
4. Run `agents connect preview --context CONTEXT_ID --plan PLAN_ID` with the native choices from `--help`, and add `--apply-file connect-apply.json`. Hermes requires an explicit protocol; Codex requires native-model/default choices; Claude requires selected presets; Qoder/Pi/DSH add entries without claiming a global default. `--management enable` and `--collaboration explicit|delegate_by_default` configure independent Skills; neither needs caller model routing. Review the result, then run `agents connect apply --request-file connect-apply.json`.
5. Inspect the Operation and `agents connect status --effective --context CONTEXT_ID --plan PLAN_ID`. Follow the returned native activation/selection guidance. Saved files do not prove loading, and a provider check does not prove the Agent used the route. Without consent for a model call, verification remains pending. A matching request identifies its route, actual model, outcome and reported usage only for that request/session. Unknown price is not zero cost.

Apply files are new private files, never overwritten, and contain exact requests and stable keys. Keep `submission.operation_lookup` for `operations find`, or replay the identical Apply file after uncertain delivery. Do not reconstruct a new request/key to retry accepted work. Rerun Quickstart to reuse completed steps. To restore only one capability, use `agents restore preview --context CONTEXT_ID --facet management_skill --apply-file restore-apply.json`, then `agents restore apply --request-file restore-apply.json`; `model` and `collaboration` select the other facets. Restore preserves unrelated data and may leave native loading/selection to the user.

Use family/leaf help for branches and `schema show --command-id routing.preview --include-schema` for complete installed definitions. The unflagged schema command remains a descriptor. JSON input and convenience flags are alternatives; `--apply-file` may accompany either supported Preview form. Desktop resident-service prerequisites must be completed through Desktop, not a fabricated CLI declaration.

Business output is one complete JSON document on stdout. Capture stderr separately and save the real exit code; an accepted Operation is not completed work. In a terminal tool that merges streams:

```sh
(
  hiroute quickstart > quickstart.stdout.json 2> quickstart.stderr.txt
  hiroute_exit=$?
  printf '%s\n' "$hiroute_exit" > quickstart.exit.txt
  exit "$hiroute_exit"
)
```

Read the whole JSON file, diagnostics and saved code separately. Never parse only the first line or use a later shell command's status. Exit 2 is invalid input; `--error-details` adds bounded field diagnostics. Keep personal notes outside the managed Skill file.

## Two command contracts

Discover Host management commands through root and family help:

```sh
hiroute --help
hiroute service --help
hiroute gateway --help
hiroute protected-input --help
```

Discover Application / Local Control commands through the Released schema and complete leaf help:

```sh
hiroute schema list --output json
hiroute schema show --command-id routing.apply --include-schema --output json
hiroute routing apply --help
```

Do not expect `schema list` to contain Host commands, and do not infer fields for the installed version from a website snippet.

## Manage models, routes, and Agents

The Released CLI now supports the complete headless loop:

- Discover and inspect model sources with `compute scan/list/show`; check and save connections with `compute connection options/test/preview/apply/authorize`.
- Inspect candidate model capabilities with `models show`.
- Manage saved decision model or custom extension revisions with `decision services list/apply/test`.
- Create, update, and publish smart routes with `routing options/list/show/preview/apply`.
- Discover local Agents with `agents scan/list/check`; connect with `agents connect preview/apply/status` and recover with `agents restore preview/apply`.
- If a write response is lost or uncertain, query the operation in its original idempotency domain with `operations find/get`.

Configuration writes preview first, then apply with the complete request, preview digest, exact revisions, and idempotency key. Models, routes, and Agents use their own `preview/apply` commands. Decision connections use `decision services apply` for both preview and save. Pass passwords and API keys through `protected-input`, never through ordinary JSON, arguments, or logs. Obtain exact fields from the matching `schema show`, `options`, and leaf `--help`.

## Manage decision connections

The CLI keeps the `decision services` command name; Desktop places these connections under Models → Decision models. Built-in decision models and custom extensions are independent of general models that execute tasks:

```sh
hiroute decision services list --output json
hiroute schema show --command-id decision.services.apply --output json
hiroute decision services apply --help
hiroute decision services apply --request-stdin --output json < decision-preview-request.json
hiroute decision services apply --request-stdin --output json < decision-apply-request.json
hiroute decision services test --request-stdin --output json < decision-test-request.json
```

There is no separate `decision services preview` command. The first `apply` accepts `{schema_version, spec}` and only previews. To save, send the same `schema_version`, returned `data.normalized_spec`, `accept_digest` from `data.change_digest`, exact `data.expected_revisions`, and a stable idempotency key. `spec.desired_state` contains the connection ID, `expected_revision`, complete `service`, and a protected `input_slot` when supplying a new credential. `service: null` deletes an unreferenced connection.

The list returns each connection's latest revision. A route selects a complete saved revision, checked by ID, revision, and content; historical revisions remain publishable. Smart saving uses editor `smart.classifier` and `smart.judgment`. Custom branches uses `branch_routing.classifier`, plan `branch_routing.judgment`, and each branch's regular `candidates`, `primary_candidates` (an empty array when no primary group is configured), and optional complete `judgment` override. An omitted override follows the plan; copying the whole set makes it independent, and clearing it restores defaults.

Saving r2 does not change a route pinned to r1. Select the new revision and publish through `routing preview/apply`. A test request uses `hiroute.classifier-decision-test/v1` with `classifier: {kind: "decision_service", service: <complete saved revision>}`. It sends fixed synthetic input to that exact connection revision, reads no real conversation, and produces no quality sample. Check `data.outcome` and `data.failure_code`; command success alone does not mean the test passed.

Custom extensions implement the [Custom extension API](/en/docs/decision-api/). See [Connect decision models](/en/docs/decision-extensions/) for built-in provider configuration and interface mapping. The optional [self-hosted Jev reference extension](/en/docs/jev-decider/) provides deployment examples. See [Use smart model routing](/en/docs/model-routing/) for the Desktop configuration steps.

## Inspect sessions and runtime performance

```sh
hiroute sessions list --include-unlinked --limit 50 --output json
hiroute sessions show <SESSION_ID> --output json
hiroute sessions receipt <RECEIPT_ID> --output json
hiroute sessions status --output json
hiroute value show --routing <PLAN_ID> --session <SESSION_ID> --output json
hiroute observation plan-quality samples --plan-id <PLAN_ID> --output json
hiroute observation plan-quality samples --session-id <SESSION_ID> --limit 50 --output json
```

Session queries return facts and a timeline by default, not conversation bodies. A receipt reports the actual route, model, and upstream-reported tokens. When no trustworthy price evidence exists, monetary value remains unknown instead of being fabricated as zero.

`observation plan-quality samples` requires at least a plan or session scope and returns the stage facts used by runtime performance. `branch_execution` records the actual task branch, model group, candidate position, and judgment policy at execution. The turn's selection reason and later stage assessment are stored separately. Use `--competence below-floor|meets-floor` to compare with the saved competence floor, or `--unrated` for missing or partial scores; unrated is not zero. Use the returned cursor for more pages. This query calls no model and returns no protected conversation bodies.

## Discover executors and plans

```sh
hiroute worker executors
hiroute worker plans
```

`worker executors` lists execution agents and their availability on this device. `worker plans` lists published plans allowed for the current agent. Choose an agent that passes dependency checks and use the returned plan ID, not its display name.

## Start a task

```sh
hiroute worker exec \
  --plan <PLAN_ID> \
  --cwd /absolute/path/to/project \
  --title "Investigate failing tests" \
  --submission-key my-check-001 \
  -- "Find the cause, propose the smallest fix, and run the relevant checks"
```

Supply exactly one input source: text after `--`, a `--file`, or standard input. `--cwd` is converted to an absolute path, but it is not directory authorization or a concurrency lock.

`--submission-key` is a caller-selected idempotency key. If a connection or process interruption leaves acceptance uncertain, keep the same key and query it. Do not generate a new key and retry blindly:

```sh
hiroute worker status --submission my-check-001 --operation start
```

## Read progress and results

Use the run ID returned at admission:

```sh
hiroute worker status --run <RUN_ID>
hiroute worker wait --run <RUN_ID>
hiroute worker result --run <RUN_ID>
```

`wait` is bounded and never cancels a task that is still running. `result` supports offset and maximum-byte options for paging through a large result.

## Continue or cancel

Continuation requires both the task ID and its exact latest run ID:

```sh
hiroute worker continue \
  --task <TASK_ID> \
  --expected-latest-run <RUN_ID> \
  --submission-key my-check-002 \
  -- "Finish the fix based on the test result"
```

Cancel one exact run:

```sh
hiroute worker cancel --run <RUN_ID> --reason user-requested
```

Cancellation does not undo files or external effects already produced.

## Machine-readable output

Management business commands default to JSON and accept JSON output. Worker commands support their documented `text|json|quiet` modes and normally default to text; use `--output json` for automation. Check each installed leaf help for its actual modes. `schema show --include-schema` adds full definitions for supported management commands; the unflagged form is a descriptor.

CLI and daemon accept only the currently Released business commands and continue to reject Planned commands. Automation should discover the runtime schema instead of hard-coding the command count or unreleased capabilities.
