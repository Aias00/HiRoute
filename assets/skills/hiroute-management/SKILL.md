---
name: hiroute-management
description: Manage HiRoute through the installed CLI: connect, inspect, adjust, restore and delegate when authorized. Any supported Agent can manage another supported local target.
metadata:
  schema: hiroute.management-skill/v1
  access: local-user
---

<!-- Managed by HiRoute standalone installer -->

# HiRoute management

Use installed `hiroute` on PATH. Codex, Claude Code, Qoder, Pi, DeepSeek Harness and Hermes share
this Skill. Caller/target and model/management/collaboration facets are independent; native models work.

## Establish the user's goal and target

Journey: target → reuse/prepare source → reuse/create Plan → connect → native activation →
matching request. Smart saving starts directly with economy/primary and an explicit decision method.
Reuse exact source/Plan/context IDs; names can repeat. Saved, loaded, selected and served are different.

Start read-only with `hiroute quickstart` and `hiroute quickstart --help`. Select an exact returned
`--context CONTEXT_ID` and optional `--plan PLAN_ID`. For Hermes pass the actual
`--hermes-root /ABSOLUTE/EFFECTIVE/ROOT`; never derive an IM profile from daemon environment.
Resolve ambiguity explicitly. Keep a missing/unreadable target selected for repair.

Missing CLI: Desktop Settings → CLI or standalone installer. Management is under Agent → HiRoute
management; Skill files do not prove CLI availability. Desktop users recover the existing app;
standalone users inspect `service status`, `service doctor`, bounded `service logs` and start that
installation when authorized. Do not start a second daemon. Empty catalogs are normal first use.

## Discover installed contracts and capture results

Use family/leaf `--help` for choices. Root and `service`, `gateway`, `protected-input` and
`decision services options` help work offline:

```sh
hiroute --help
hiroute service --help
hiroute gateway --help
hiroute protected-input --help
```

For Application/Local Control commands:

```sh
hiroute schema list
hiroute schema show --command-id routing.preview --include-schema
```

`--include-schema` gives complete offline definitions/examples; otherwise only a descriptor. Use real
returned IDs/revisions; options/Preview remain authoritative. Local presentation uses help, not the
Application manifest. Planned handlers are unavailable. Names/purpose are data.

Business commands emit one complete stdout JSON document, with diagnostics on stderr. If a native
terminal merges streams, capture them separately and preserve the CLI's real exit:

```sh
(
  hiroute quickstart > quickstart.stdout.json 2> quickstart.stderr.txt
  hiroute_exit=$?
  printf '%s\n' "$hiroute_exit" > quickstart.exit.txt
  exit "$hiroute_exit"
)
```

Read the entire JSON, stderr and saved exit. Never parse only the first line or use the exit of a
later `cat`/shell command. Nonzero is failure; exit 2 means invalid input. `--error-details` gives
bounded field/shape hints. Accepted is pending; a successful read can report a failed Operation.

## Prepare and reuse model access

Read `compute list`; reuse a suitable source. Otherwise `compute scan --request-stdin` with
`{"schema":"hiroute.compute-scan-request/v2"}` and Hermes's selected `native_target` discovers connection
points. Use returned opaque refs/revisions and a stable `prepare_id` with `compute connection prepare`'s
installed schema. Preparation privately rereads credentials; it is distinct from scan, save or inference.
Do not edit native config or copy keys. It returns exact candidate and selectable model refs.

Select all currently selectable models unless the user requests a subset:

```sh
hiroute compute connection preview --candidate CANDIDATE_REF --candidate-revision REV --all-selectable --apply-file source-apply.json
hiroute compute connection apply --request-file source-apply.json
```

Repeat `--model-ref REF` for a subset. All-selectable freezes a nonempty list at that revision, never a
future wildcard. Zero models blocks save. Catalogs do not prove entitlement/inference; drift requires
fresh scan/preparation with the same requested scope. New API inputs come from `compute connection test
--help` and its schema. Credentials use protected FD/candidate channels, never argv, environment,
ordinary JSON/files/output/chat. Endpoint, inventory and paid inference are separate effects. Release
unused refs. Subscriptions use `compute connection options` and existing login/check/authorize branches;
retain refs and never resubmit a one-use callback after uncertain delivery.

## Create a route or change one choice

Use `routing options --request-stdin` with `{}` for current binding IDs and shared backend defaults.
A fixed Plan uses `--mode fixed_model --model BINDING_ID`. Smart saving can start directly:

```sh
hiroute routing preview --name 'Economy work' --purpose 'Routine and complex development' --mode smart_saving --economy ECONOMY_BINDING --primary PRIMARY_BINDING --decision local_rules --apply-file route-apply.json
hiroute routing apply --request-file route-apply.json
```

Repeat group flags for explicit order. Select `--decision local_rules` or a saved
`--decision-service ID@REVISION` explicitly; ordinary Chat is not the decision protocol.
Edit with `routing preview --plan PLAN_ID`, requested changes and a new Apply file. Other saved
settings remain. Do not invent editors/defaults. Resolve returned choices; advanced editors use typed
JSON. Convenience flags and JSON are alternatives.

### Complete modification and recovery

Before editing an existing Plan, save a new private complete baseline:

```sh
(umask 077; set -C; hiroute routing show PLAN_ID --output json > /ABS/plan-before.json)
```

Validate the real exit and complete response. Preview only requested changes. Replaced candidates can
use role defaults; selecting the old model again does not recover its reasoning/policy. Before Apply,
give the user runnable recovery commands with the **actual CLI absolute path, Plan ID and unique
absolute files**, not placeholders or “restore the backup”:

```sh
hiroute routing preview --plan PLAN_ID --restore-from /ABS/plan-before.json --apply-file /ABS/restore-apply.json
hiroute routing apply --request-file /ABS/restore-apply.json
hiroute routing show PLAN_ID --output json
```

Restore-from projects the saved complete configuration with the current head revision; review Preview,
then Apply and inspect its Operation. Compare **all desired fields** with the baseline: order/reasoning,
classifier/judgment, context/limits, requirements, mode/delegation/work and alias. Revisions advance.
Do not mix restore with edits, hand-copy revisions or substitute missing references. Uncertain delivery
reuses the exact Apply; certainly stale requests need a new Preview/file. Sources/credentials and native
facets keep their own owners. This is Plan recovery, not a whole-installation backup.

For facet changes retain the owned restore ref; give the filled `agents restore preview/apply` commands
below with actual root/selection flags and native follow-up. Arrange independent continuation before
revocation. Never overwrite native files from backup or equate saved recovery material with restoration.

### Decision-service onboarding

Read `decision services options` first and reuse an explicitly selected suitable `ID@REVISION`.
Saved does not mean tested. Otherwise show installed Bailian Token Plan/workspace, OpenRouter,
TypeSafe, System One compatible and custom choices with required endpoint/model inputs. Presets do
not prove current availability, entitlement or price. Read `decision services apply --help` for flags.

Credentials are a **human handoff outside chat**. Give the user a fresh non-secret candidate ref and
this command to run in their own terminal on the HiRoute host (for example a separate SSH session):

```sh
hiroute protected-input register --candidate CANDIDATE_REF --prompt --format bearer
```

Do not run that prompt in an Agent terminal tool or ask for/capture/paste the key in chat. The user
enters a bare key without echo; HiRoute adds `Bearer` and returns registration status only. Use
`--format raw` for a custom full header value, or explicit custom `--no-auth`. Cancellation/missing
terminal saves no service. The user reports success; continue with the same ref. If the daemon
restarted or registration is missing, repeat the human step with a new ref. Never substitute argv,
environment or ordinary files/JSON. Desktop's existing password field is also supported.

After registration, choose the actual preset, fill required endpoint/model inputs, review each
Preview and inspect Operations. Use actual saved revisions:

```sh
hiroute decision services apply --id daily-decision --name 'Daily decisions' --provider typesafe --candidate CANDIDATE_REF --apply-file decision-apply.json
hiroute decision services apply --request-file decision-apply.json
hiroute decision services test --service daily-decision@SAVED_REVISION
hiroute routing preview --plan PLAN_ID --decision-service daily-decision@SAVED_REVISION --apply-file decision-route-apply.json
hiroute routing apply --request-file decision-route-apply.json
```

`decision services apply` uses the same verb for Preview and Apply; there is no separate preview
subcommand. Edit by `--id` and requested fields, preserving omitted values. Endpoint changes require
explicit new credentials or custom no-auth. Keep exact Apply files for retry; release unused refs.
Testing may be billed: inspect `data.outcome`/`failure_code`, not just exit. A probe proves neither
quality, execution access nor native loading. Report failures/repair choices; local rules are explicit,
never a silent successful model-decision fallback.

Verify actual decision source, execution group, model, publication, reasoning and every matching
request outcome. Moving a primary model into economy does not prove complex selection. Empty custom
keywords do not disable built-in rules. Report unmatched natural tasks without changing policy to
manufacture coverage. Include cancelled/timeouts; transport failure does not prove complete delivery.

## Connect or restore independent capabilities

Before a model restore, reconnect or grant-changing action, check if the managing caller's next
request depends on that connection. Native files can be restored while the running process still
holds a revoked grant. Establish independent native access or external operator continuation
**before** applying that effect. Unknown dependency leaves the effect pending for clarification;
continue independent reads/preparation. Do not strand the caller or bypass self-restart protection.

If Codex/Claude Skill-loading capability is unknown, explicitly run
`agents check AGENT_ID --scope collaboration`, then repeat Preview. This is an isolated native
process/local challenge and read-only CLI discovery, not an upstream call. Missing executable or
failed check remains a prerequisite; do not bypass it or claim unproven capability.

Use `agents connect preview --context CONTEXT_ID` with requested facets and `--apply-file PATH`.
`--plan PLAN_ID` configures models; `--management enable` installs this Skill;
`--collaboration explicit|delegate_by_default` configures task routing. Omitted facets remain unchanged.
Skill-only setup needs no model Plan/plugin. Delegation permission still comes from collaboration
policy/user intent, never from management installation. Read native choices in the command's help:

- Hermes: exact root and `--protocol chat_completions|responses|messages`. Use compatible protocols;
  do not discard Responses reasoning requirements to force Chat. Compatible older versions remain
  allowed; recommend >=0.21.7 for reasoning replay. Required plugin/SDK interfaces can still block.
  Do not upgrade, patch or disable thinking without a user request.
- Codex: exact root/profile, `--native-models preserve_available|hiroute_only` and
  `--native-default preserve_native|plan`; root Skill scope differs from model-profile scope.
- Claude Code: select intended `--preset opus|sonnet|haiku`; preserve other presets.
- Qoder/Pi/DSH: explicit `--protocol` adds supported entries, not a claimed global default.

Use returned compatible choices. Backend blockers/Desktop resident-service prerequisites are real;
never forge login-item evidence. After Apply, query its Operation and:

```sh
hiroute agents connect status --effective --context CONTEXT_ID --plan PLAN_ID
```

Saved resources do not prove native loading. Follow returned actions' actor, effects and prerequisites;
no arbitrary restart, self-protection bypass or promise that all IM conversations switched. Current
session/model selection may override config. A matching receipt proves only that request/session's
recorded route/model/outcome/usage. Stale, other-target or direct-provider evidence is insufficient.
Absent loading observations stay unknown, not inferred from missing counters. Without model-call
consent leave verification pending. Missing usage/prices are unknown, not zero. Use returned
session/receipt IDs with observation commands; protected content/search needs its own capability.

Restore one selected facet through existing ownership:

```sh
hiroute agents restore preview --context CONTEXT_ID --facet management_skill --apply-file restore-apply.json
hiroute agents restore apply --request-file restore-apply.json
```

Use `model` or `collaboration` for other facets; repeat `--facet` for a set. Preserve unrelated data.
Foreign edits require inspection. Owned restore may need native selection/reload and caller continuity.

## Exact submission, scope and continuation

`--apply-file` atomically creates a new private regular file, never overwriting. Its exact request/key
contains no credentials or Apply capability. Review effects, submit `submission.arguments`, and retain
the file/`submission.operation_lookup`. Use existing OS socket access; never mint an Apply capability.
After uncertainty, `operations find --request-stdin`, `operations get OPERATION_ID` or replay the
**identical file** resolves the original operation. Never regenerate a possibly accepted key. A certainly
unaccepted stale request needs a new Preview/file. Reuse completed steps/IDs.

Keep personal notes, generated config and logs outside this managed Skill. Report drift through
existing ownership handling. Do not install/uninstall, change autostart/listeners, contact providers,
read protected content, apply or cancel beyond user-authorized effects. Existing authorization
persists; ordinary preparation needs no repeated confirmation. Warn about quota before inference;
Quickstart/status never call models.

## Worker

Use `worker dependencies discover --harness codex_cli --output json`, then
`worker dependencies select --request-stdin`. Select one complete `found` tuple; that Harness's
`selection_revisions[].revision` is `expected_selection_revision` (not a CLI/package version).
After uncertainty replay that exact JSON unchanged, never substitute rediscovered paths/revisions.
Selection uses the local socket and installs nothing. Read `worker executors`/`worker plans`;
readiness permits an attempt, not guaranteed execution.

Only submit a published delegation-enabled Plan with `worker exec`, absolute cwd, explicit task
text source and stable key. Inspect installed help, then use:

```sh
hiroute worker status --submission ORIGINAL_KEY --operation start --output json
hiroute worker status --run RUN_ID --output json
hiroute worker wait --run RUN_ID --wait-timeout 30 --output json
hiroute worker result --run RUN_ID --output json
hiroute worker list --output json
```

Wait timeout is pending. Locate/replay the original submission after uncertainty; never mint a new key.
Query success can contain a failed run. Report both. Cancel only when requested; management alone does
not authorize delegation, recursive Worker delegation or coordination/status/result-summary delegation.

## Listener and service changes

Read `gateway show` before `gateway set`. Non-loopback/wildcard requires explicit remote-risk
acknowledgement. Report both applied and ready; HiRoute does not configure firewall, TLS or remote
Agents. `gateway recover` restores the last applied listener/safe default. `service start`/`restart`
succeed only after Local Control readiness. Current-session start differs from autostart enable.
