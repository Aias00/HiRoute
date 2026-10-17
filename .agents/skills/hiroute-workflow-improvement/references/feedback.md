# Feedback records and labels

## Stable taxonomy

Every feedback Issue carries `improvement/feedback` and exactly one category:

| Label | Description | Suggested color |
| --- | --- | --- |
| `improvement/feedback` | Evidence-backed CLI or workflow feedback for periodic analysis | `5319e7` |
| `improvement/cli` | issue-spec CLI behavior, capability or command usability | `0052cc` |
| `improvement/workflow-generic` | Reusable issue-spec workflow or generated instruction improvement | `1d76db` |
| `improvement/workflow-project` | Project-specific workflow, policy or skill improvement | `0e8a16` |

Keep these names stable across repositories and dates. Use the umbrella for the
whole feedback set and the category for grouping; do not add per-session, per-agent
or per-week labels. GitHub timestamps and the dated observations below support
interval analysis. Existing unrelated labels remain intact.

## Body fields

Use these fields in ordinary Markdown; they are analysis conventions, not typed
issue-spec artifacts. Omit unavailable revisions with an explicit reason.

- **Category:** `cli`, `workflow-generic`, or `workflow-project`.
- **Observed on:** ISO date with timezone; distinguish observation from filing time.
- **Source:** originating project, public Issue/PR or repository-relative instruction
  path, and applicable project commit, CLI version/revision and workflow source revision.
- **Owning layer and routing reason:** why the correction belongs in this repository;
  for workflow reports, explain the project-independent or project-specific boundary.
- **Observation:** minimal sanitized command/task sequence, expected versus actual
  behavior, and concrete impact. Mark hypotheses explicitly.
- **Recurrence:** observed count and distinct task contexts, or `unknown`; never infer
  frequency from repeated tool retries or count one report per label as occurrences.
- **Suggested improvement:** smallest useful correction and any known workaround.
- **Acceptance and resumption:** observable outcome, minimal sanitized scenario,
  necessary prerequisites and stable evidence references for a future session.
  Record known missing context; do not require a complete environment archive.
- **Related:** matching prior discussions and linked reports for independent causes.

For new evidence on an existing Issue, append the observation date, source revision,
new reproduction/impact and what differs. Do not overwrite its original evidence.
Record a recurrence only for a distinct observed occurrence, not another search or
retry of the same event.

When a correction becomes available, append its public fix/release link and revision.
When HiRoute adopts it, append the adoption PR/commit, installed CLI or generated
workflow revision and, when feasible, the comparable scenario and observed result.
Otherwise record why benefit remains unverified or the scenario no longer applies. Keep
these distinct from occurrence counts. A useful review separates reported findings,
upstream fixes available, adopted fixes, verified benefit and remaining friction;
Issue open/closed state alone cannot represent that progression.

## Resume after a gap

The Issue is the durable handoff, not the original chat, agent or temporary checkout.
Keep the problem, expected outcome and minimal scenario understandable from its body
and dated comments; use public commit permalinks for historical paths that may move.
Summarize essential evidence in sanitized text. Raw logs can stay in ignored local
artifacts, but future understanding must not depend on those artifacts surviving.

On a later relevant task or requested review, read the selected Issue and linked
fix/adoption records, then check today's commands, versions and project requirements.
Distinguish an upstream fix being available, a local adoption, verified benefit,
unverified benefit, and a scenario that no longer applies. These are plain-language
observations in dated comments, not a new typed workflow, label set or mandatory
state machine. Write only when new evidence or a material context change exists.

If context is sufficient, use the smallest comparable current scenario and explain
any differences from the original. If prerequisites or baseline evidence are missing,
record what cannot be concluded and stop that validation attempt. If requirements
have changed, explain why the old scenario no longer applies instead of forcing a
reproduction. Missing baseline measurements cannot establish a quantitative gain.
A known future trigger can be recorded without creating a reminder or recurring job.

## Commands and partial failures

Read exact command help before unfamiliar options. Search/read and recurrence
comments can use the configured issue-spec discussion commands:

```sh
issue-spec search issues --repo OWNER/REPO --query 'specific symptom' --scope discussion --state all --limit 10 --page 1 --json
issue-spec read issue --repo OWNER/REPO --issue NUMBER --comments
issue-spec comment create --repo OWNER/REPO --issue NUMBER --body-file occurrence.md --json
```

For ordinary GitHub feedback Issues, HiRoute explicitly selects
[the GitHub skill](../../issue-spec-github/SKILL.md) and `gh issue create` with
labels. Native issue-spec label support is not a prerequisite, and its absence
alone does not warrant a CLI improvement Issue when gh meets the task's needs.
Prepare the body, ensure the umbrella and chosen category exist, then create:

```sh
gh label list --repo OWNER/REPO --limit 100 --json name,description,color
gh label create 'improvement/feedback' --repo OWNER/REPO --color 5319e7 --description 'Evidence-backed CLI or workflow feedback for periodic analysis'
gh label create 'improvement/cli' --repo OWNER/REPO --color 0052cc --description 'issue-spec CLI behavior, capability or command usability'
gh issue create --repo OWNER/REPO --title 'Concrete improvement' --body-file report.md --label 'improvement/feedback' --label 'improvement/cli'
gh issue view NUMBER --repo OWNER/REPO --json url,body,labels
```

The CLI category above is an example; use the category actually selected by the
routing table. Create only missing labels and do not use `--force`. Paginate a
larger label inventory before concluding a label is absent. For a reused matching
Issue, attach missing labels with `gh issue edit --add-label`, preserving unrelated
labels. Inspect the Issue before retrying an uncertain create; if it exists, repair
only missing labels rather than submitting again.

This provider choice covers ordinary feedback, not issue-spec planning artifacts.
For a non-GitHub target use its supported issue-spec Issue/label operations. If
labeling is unavailable, keep the feedback draft or report an already-created Issue
as incompletely labeled. Do not switch to GitHub or raw APIs after a backend denial.

## Interval analysis

For GitHub, query each verified repository with the umbrella label, `--state all`
and an explicit date range. For example, replace the dates with the requested window:

```sh
gh issue list --repo OWNER/REPO --label 'improvement/feedback' --state all --search 'created:2026-10-01..2026-10-31' --limit 100 --json number,title,url,labels,createdAt,updatedAt,state
```

This selects new Issues, not all occurrences. Also discover older Issues updated in
the interval (`updated:START..END`), then read their discussions through issue-spec
and count only explicitly dated observations in the interval. An update can be a
label edit or closure and is not evidence of recurrence. Page through results when
limits are reached, or split the date range and deduplicate by repository + number;
state incomplete coverage if it cannot be exhausted. Use the user's timezone and
explicit inclusive start/end dates in the summary. Include relevant fix/adoption
updates from that window and distinguish verified benefits from untested upgrades
and scenarios that no longer apply. Do not treat absence of a recorded occurrence as evidence that the workflow never encountered the problem.
