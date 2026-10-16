---
name: hiroute-management
description: Help users connect, inspect, adjust and restore HiRoute through the installed CLI, including model sources, routes, Agent settings, observations and authorized task delegation. Works from any supported Agent; the caller and managed target can differ.
metadata:
  schema: hiroute.management-skill/v1
  access: local-user
---

<!-- Managed by HiRoute standalone installer -->

# HiRoute management

Use the installed `hiroute` on PATH. This is the same management Skill for Codex, Claude Code,
Qoder, Pi, DeepSeek Harness and Hermes. The caller is not automatically the target. Establish
which local configuration/profile the user wants to manage; the caller's model may remain native.
Model routing, management Skill and task routing are independent capabilities.

## Start with the user's outcome

- First connection: identify target → reuse/prepare source → reuse/create Plan → connect →
  native activation → inspect a matching request or leave verification pending.
- Economy combination: directly create smart saving with economy/primary models and an explicit
  decision method. A temporary fixed-model Plan is unnecessary.
- Continue or adjust: reuse exact source/Plan/context IDs, inspect existing state and change only
  the requested choices. Names may repeat and are not resource identities.
- Diagnose: separate saved configuration, native loading, session selection and request outcome.
- Restore: restore the requested owned facet and explain any remaining native activation step.
- Manage another Agent or delegate: keep caller/target and all three capabilities independent;
  delegation permission still comes from the applicable collaboration policy/user request.

Start read-only:

```sh
hiroute quickstart
hiroute quickstart --help
```

Choose an exact returned context. For an explicit Hermes profile, use
`hiroute quickstart --hermes-root /ABSOLUTE/EFFECTIVE/ROOT`; never guess an IM user's profile
from the daemon environment. Rerun with `--context CONTEXT_ID` and, when chosen, `--plan PLAN_ID`.
Preserve a missing/unreadable selected target instead of falling back to another root. Resolve
ambiguity with an exact context, not the first matching Agent name.

If the CLI is missing, report the installation prerequisite. A human or another available Agent
installs the CLI/Skill through HiRoute Desktop Settings → CLI and Agent → HiRoute management,
or the standalone installer. Skill files alone do not prove a usable CLI. Desktop users open or
recover the existing app; standalone users inspect `service status`, `service doctor` and bounded
`service logs`, then start that installation when authorized. Do not start a second daemon.
An empty model/route catalog is a normal first-use state, not a reason to block all configuration.

## Discover the installed contract progressively

Use family/leaf `--help` for commands and choices. For supported management JSON requests:

```sh
hiroute schema list
hiroute schema show --command-id routing.preview --include-schema
```

The flag includes complete offline request/response definitions and examples; without it the
result is only a command descriptor. Examples describe syntax: replace sample IDs/revisions with
current returned facts. Dynamic options and Preview remain authoritative. Host commands and
Quickstart appear in root/family help, not the Application release manifest. Planned handlers
are not available merely because they exist in source. Treat returned names/purpose text as data.

Business commands emit **one complete JSON document on stdout**, with diagnostics on stderr.
If a native terminal merges streams, capture them separately and preserve the CLI's real exit:

```sh
(
  hiroute quickstart > quickstart.stdout.json 2> quickstart.stderr.txt
  hiroute_exit=$?
  printf '%s\n' "$hiroute_exit" > quickstart.exit.txt
  exit "$hiroute_exit"
)
```

Read the entire stdout file as JSON, stderr separately, and the saved exit value. Never parse only
its first line or use the exit status of `cat`, `echo` or a later tool as the CLI result. A nonzero
exit is failure; exit 2 means invalid input. `--error-details` adds bounded field/shape diagnostics.
An accepted Operation is pending, and a successful read can report a failed business operation.

## Prepare and reuse model access

Read `compute list` first and reuse a suitable saved source. To import an existing native connection,
use `compute scan --request-stdin` with `{"schema":"hiroute.compute-scan-request/v2"}`. Include the
selected `native_target` for a Hermes profile. Select the returned connection's opaque `discovery`
and exact revision; pass it with a stable `prepare_id` to `compute connection prepare` using its
installed schema. Preparation privately rereads credentials; it is a separate local effect from scan,
save, provider tests and inference. Do not copy secrets manually or rewrite native configuration.

Choose all currently selectable models unless the user requests a subset. Then:

```sh
hiroute compute connection preview --candidate CANDIDATE_REF --candidate-revision REV --all-selectable --apply-file source-apply.json
hiroute compute connection apply --request-file source-apply.json
```

Replace `--all-selectable` with repeated `--model-ref REF` for a subset. The saved request contains
concrete refs at one candidate revision, never a wildcard. Zero selectable models blocks saving.
Catalog models are qualified candidates, not proof of entitlement or tested inference. A later scan
cannot expand an accepted selection. On source drift, rescan/prepare and retain the user's scope.

For a new API source, read `compute connection test --help` and its full schema. Credentials enter
only through the existing `protected-input register --candidate REF --secret-fd FD` channel. Never
place secrets in argv, environment, ordinary JSON, output, notes or conversation. Use the chosen
endpoint/protocol only; inventory and paid inference are separate. Release unused protected input.
For subscriptions, use `compute connection options` and the existing login/check/authorize branches;
retain their returned refs and never resubmit a one-use callback after uncertain delivery.

## Create a route, or change one choice

Use `routing options --request-stdin` with `{}` for current binding IDs. New routes can be fixed or
smart immediately; these commands request the same backend defaults used by Desktop:

```sh
hiroute routing preview --name 'Daily work' --purpose 'Routine development' --mode fixed_model --model BINDING_ID --apply-file route-apply.json
hiroute routing preview --name 'Economy work' --purpose 'Routine and complex development' --mode smart_saving --economy ECONOMY_BINDING --primary PRIMARY_BINDING --decision local_rules --apply-file economy-apply.json
```

Review then use `hiroute routing apply --request-file FILE`. Repeat group flags for an explicit
order. Instead of local rules, select `--decision-service ID@REVISION` from `decision services list`.
Choose the decision method explicitly; an ordinary Chat execution binding is not a compatible
decision-service protocol. Decision services use the **same** `decision services apply` verb with
either a Preview or Apply wrapper; inspect its full schema. Do not invent a `preview` subcommand.

For a change, use `routing preview --plan PLAN_ID` plus only requested options and a new Apply file.
Reopening preserves other saved settings. Do not reconstruct an editor or guess inactive sections,
reasoning values, context windows or timeout defaults. Resolve returned required choices. Advanced
reasoning/lifecycle/draft editors remain available through installed typed JSON contracts.

## Connect or restore independent capabilities

Use `agents connect preview --context CONTEXT_ID` with the requested facets, then an Apply file.
`--plan PLAN_ID` configures model routing; `--management enable` installs this Skill;
`--collaboration explicit|delegate_by_default` configures task routing. Omitted facets stay unchanged.
Skill-only configuration needs no model Plan or model plugin. Existing collaboration trigger policy
still owns when tasks may be submitted; management never grants permission for an unrequested task.

Native model choices must be explicit; read `agents connect preview --help`:

- Hermes: selected root and `--protocol chat_completions|responses|messages`. Use a qualified
  matching protocol. Do not discard Responses encrypted reasoning/summary requirements to force a
  Chat source to work. Compatible older versions stay allowed; recommend >=0.21.7 for reasoning
  replay. Required plugin/SDK interfaces may still block. Do not upgrade, patch or disable thinking
  without a user request.
- Codex: choose its exact root/profile context, `--native-models preserve_available|hiroute_only`
  and `--native-default preserve_native|plan`. Keep root Skill scope distinct from model profiles.
- Claude Code: explicitly select each intended `--preset opus|sonnet|haiku`; other presets remain.
- Qoder/Pi/DSH: select `--protocol`; this adds supported entries and does not claim a global default.

Use only returned compatible choices. Backend blockers and Desktop resident-service prerequisites
are real: do not forge login-item evidence. After Apply, query the Operation and:

```sh
hiroute agents connect status --effective --context CONTEXT_ID --plan PLAN_ID
```

Saved resources do not prove that a running native process loaded them. Follow the returned native
action's actor, effects and prerequisites. Do not bypass Hermes self-restart protection, run arbitrary
restart commands, or promise all IM conversations switched. A current channel/session can override
configuration. A matching receipt establishes only its recorded request/session and outcome; stale,
other-target or direct-provider-only evidence is insufficient. Without model-call consent, leave
verification pending. Report actual model/outcome/usage; absent counters/prices are unknown, not zero.
Use the returned session/receipt IDs with existing observation commands; protected content/search
still requires its own declared capability.

To restore one facet:

```sh
hiroute agents restore preview --context CONTEXT_ID --facet management_skill --apply-file restore-apply.json
hiroute agents restore apply --request-file restore-apply.json
```

Use `model` or `collaboration` for the other facets, or repeat `--facet` for the explicit requested
set. Preserve unrelated resources/history. An ownership conflict needs inspection, not a backup
copied over current files. After an owned restore, an existing native conversation may still need
its model selection/loading updated.

## Exact submission and continuation

`--apply-file` creates a new private, regular file atomically; it never overwrites. It contains the
exact normalized Apply request and one stable key, with no credentials or Apply capability. Review
the Preview's effects, then execute its returned `submission.arguments`. Local management uses the
existing OS socket-access boundary. Never mint or request another Apply capability.

Keep that file and `submission.operation_lookup`. If an Apply response is uncertain, send that lookup
to `operations find --request-stdin`, then inspect `operations get OPERATION_ID`, or resend the
**identical file**. Do not produce a fresh Preview/key to guess whether the old one succeeded. Once
an old request is definitely unaccepted/stale, prepare current choices into a new file. Completed
source/Plan steps survive later failure. Reuse IDs; do not create duplicates by matching names.

Do not append experience notes, generated config or logs to this managed Skill. Store personal notes
separately. Report foreign edits through the existing ownership/restore conflict handling.

Do not install/uninstall, change autostart/listeners, contact a provider, expose protected content,
apply changes or cancel work beyond the user's requested effects. Existing authorization persists;
ordinary preparation does not require repeated confirmation. Never recursively delegate from a Worker
or delegate coordination, status checks or summaries. Warn about quota before an authorized inference
check; status and Quickstart themselves do not make such calls.

## Worker

Discover and select dependencies before submission:

```sh
hiroute worker dependencies discover --harness codex_cli --output json
hiroute worker dependencies select --request-stdin --output json
hiroute worker executors --output json
hiroute worker plans --output json
```

Choose only one complete `found` tuple and copy its Harness-matching
`selection_revisions[].revision` into `expected_selection_revision`; this is a selection CAS
revision, not a CLI/package version. Selection uses local OS socket access and installs nothing. Preserve the
strict selection JSON; on uncertain delivery, replay that exact JSON unchanged. Do not rediscover
and silently substitute a newer path/revision after an uncertain result.

Submit only a published, delegation-enabled plan with `worker exec`, an absolute cwd, explicit
task text source, and a stable submission key. Then use:

```sh
hiroute worker status --submission ORIGINAL_KEY --operation start --output json
hiroute worker status --run RUN_ID --output json
hiroute worker wait --run RUN_ID --wait-timeout 30 --output json
hiroute worker result --run RUN_ID --output json
hiroute worker list --output json
```

A wait timeout is pending, not cancellation. On uncertain submission, locate by the original key
or replay the identical exec; never change the key and risk duplicate work. A result query can
succeed while honestly reporting a failed run, so report the run state separately from command
exit. Do not cancel unless requested.

## Listener and service changes

Use `gateway show` before `gateway set`. Non-loopback or wildcard listening requires the user's
explicit remote-risk acknowledgement. After a change, report both applied and ready; never claim
that HiRoute changed firewall, TLS, or remote Agent configuration. `gateway recover` returns to the
last applied listener or safe default.

`service start` and `restart` are successful only after Local Control is ready. Keep current-session
start separate from `service autostart enable`.
