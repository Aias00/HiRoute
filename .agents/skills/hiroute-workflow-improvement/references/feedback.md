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

Priority is separate from category. Use `priority/high` for the recurrence promotion
below; do not invent another improvement-specific priority label. Reuse its existing
definition. If missing, provision it only when a qualifying promotion needs it, with
description `High-priority issue` and suggested color `d73a4a`.

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
- **Recurrence:** confirmed count or lower bound, distinct task contexts and links to
  the original evidence, or `unknown` when independence cannot be established. Use
  the counting rules below rather than comment totals or a previous summary's number.
- **Suggested improvement:** smallest useful correction and any known workaround.
- **Acceptance and resumption:** observable outcome, minimal sanitized scenario,
  necessary prerequisites and stable evidence references for a future session.
  Record known missing context; do not require a complete environment archive.
- **Related:** matching prior discussions and linked reports for independent causes.

For new evidence on an existing Issue, append the observation date, source revision,
new reproduction/impact and what differs. Do not overwrite its original evidence.
A distinct occurrence is useful new evidence even when its symptom is unchanged.

When a correction becomes available, append its public fix/release link and revision.
When HiRoute adopts it, append the adoption PR/commit, installed CLI or generated
workflow revision and, when feasible, the comparable scenario and observed result.
Otherwise record why benefit remains unverified or the scenario no longer applies. Keep
these distinct from occurrence counts. A useful review separates reported findings,
upstream fixes available, adopted fixes, verified benefit and remaining friction;
Issue open/closed state alone cannot represent that progression.

## Count independent occurrences and promote

Count evidence for the same unresolved cause across tasks, without resetting at a
new session or date. The original report counts as **1** when it records observed
friction. Each later independent encounter counts once, including one by the same
person or agent in a different task. A new agent, author, session or comment alone
does not prove independence: require a distinct task/run context, observation date
and sanitized scenario/impact with applicable source revisions.

Do not count retries, repeated searches, handoffs, copied reports, acknowledgements,
design discussion, or fix/adoption validation of the original scenario as additional
occurrences. Legacy comments can count if their evidence establishes a distinct
encounter; no new template is required retroactively. Deduplicate evidence referenced
from multiple comments or Issues. Summaries and reported totals point to evidence;
they are not additional occurrences. If history is incomplete, report a confirmed
lower bound; promote only when at least 3 distinct occurrences are supported.

Before posting, reread the Issue and relevant comments so a retry or concurrent
report of the same event does not create another occurrence comment. A new comment
records the current context, links the earlier counted evidence and states the
count before and after this observation. For example:

```markdown
### Independent recurrence
- Observed on: YYYY-MM-DD (timezone).
- Source and context: originating task/PR, applicable revisions, and the distinct task sequence.
- Observation and impact: sanitized expected/actual behavior and concrete cost.
- Recurrence: 2 confirmed before this observation; at least 3 including this occurrence.
- Earlier evidence: original Issue body (1); linked independent recurrence comment (2).
- Priority: recurrence threshold reached; promote to `priority/high`.
```

When recording a new occurrence on an **open, unresolved** feedback Issue and the
confirmed count reaches or exceeds **3**, apply `priority/high` automatically after
the comment is persisted. The original report plus one independent recurrence
comment plus the current independent encounter qualifies; an Issue with two ordinary
discussion comments does not. A newly filed report with 3 independently evidenced
encounters also qualifies. Verify the recorded evidence, current state and labels
before promotion; reread after writing to confirm the result.

If already high or higher, preserve that priority. On promotion replace only
`priority/low` and `priority/medium` when present, preserving category and unrelated
labels. Do not override an explicit maintainer decision against escalation or a
closed/wontfix decision; record the new evidence and explain why automatic promotion
was skipped. A newly filed regression after a fix starts its own count and links the
old Issue; do not inherit its resolved occurrences. Lower counts do not trigger an
automatic downgrade, and frequency alone does not assign critical priority or a
delivery deadline. Reconcile uncertain writes by reading the Issue; retry only a
missing comment or label operation, without counting the same event again.

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

For a qualifying promotion, ensure `priority/high` exists, then add it. Include
`--remove-label 'priority/low'` and/or `--remove-label 'priority/medium'` in the same
edit command only for lower labels actually present:

```sh
# Create only when missing; preserve any existing definition:
gh label create 'priority/high' --repo OWNER/REPO --color d73a4a --description 'High-priority issue'
gh issue edit NUMBER --repo OWNER/REPO --add-label 'priority/high'
gh issue view NUMBER --repo OWNER/REPO --json url,state,labels
```

If the recurrence comment succeeds but promotion fails, keep its URL and confirmed
count, report the pending priority update, and repair only labeling. Do not repost
the occurrence or claim promotion succeeded from the intended-action line alone.

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
label edit or closure and is not evidence of recurrence. Use the same independence
rules above; distinguish occurrences in the interval from the cumulative confirmed
count used for promotion. Page through results when limits are reached, or split
the date range and deduplicate by repository + number;
state incomplete coverage if it cannot be exhausted. Use the user's timezone and
explicit inclusive start/end dates in the summary. Include relevant fix/adoption
updates from that window and distinguish verified benefits from untested upgrades
and scenarios that no longer apply. Do not treat absence of a recorded occurrence as evidence that the workflow never encountered the problem.
