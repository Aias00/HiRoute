---
name: hiroute-workflow-improvement
description: Capture concrete issue-spec CLI or workflow friction encountered in HiRoute, route actionable feedback upstream or to the project, and label and deduplicate reports for later analysis. Also use for requested feedback reviews; not ordinary product bugs or automatic workflow rewrites.
---

# Improve issue-spec through evidence

Turn observed CLI/workflow friction into small, searchable suggestion Issues.
Continue the original task wherever possible. Feedback is not a new planning,
review or delivery gate, and creating an Issue does not authorize its implementation.

HiRoute's repository policy authorizes filing and labeling evidence-backed feedback
from its work. Honor a user's narrower scope, such as read-only or no publication.
Do not infer this standing authorization when copying this skill to another project.
Do not start agents, recurring jobs, workflow upgrades or automatic rule rewrites.

## Establish the cause and destination

Read [instruction ownership](../../README.md), the current project's
`issue-spec/config.yaml` rules and shared guidance. Inspect the relevant installed
command's exact `--help`, or the instruction and its owning source. Record the CLI
revision from `issue-spec version --json`, the generated workflow source revision
from `../issue-spec-workflow/release.json`, and the project commit when applicable.
Different installed and generated revisions alone are not a defect.

| Finding | Destination | Category label |
| --- | --- | --- |
| CLI behavior, command UX, unsupported capability, incorrect output or error | `higress-group/issue-spec` | `improvement/cli` |
| Shared workflow or generated instruction gap whose correction remains useful after removing HiRoute names, paths and policy | `higress-group/issue-spec` | `improvement/workflow-generic` |
| HiRoute policy, project supplement, local skill, validation or repository-specific convention | `higress-group/HiRoute` | `improvement/workflow-project` |

Use the smallest source that actually owns the correction. A stale local recipe or
an agent ignoring correct instructions is not automatically a CLI defect. Correct
routine command use first. File only when a concrete usability or instruction gap
remains; transient network/authentication errors alone do not establish one.

For workflow findings, explain whether the fix survives removal of project policy.
If ownership remains uncertain, gather a bounded additional read; keep a local draft
and state the uncertainty instead of guessing the destination. Split genuinely
independent CLI and workflow causes into linked reports, not copies of one symptom.
Never edit generated `issue-spec-*` files to implement upstream suggestions.

## Deduplicate and write

Use the [report and analysis guide](references/feedback.md) for the body fields,
label definitions, provider commands and time-window queries.

1. Verify the destination repository and active backend with read-only CLI/auth
   inspection; use explicit `--repo` on each command. HiRoute and the upstream above
   are GitHub repositories. Do not change profiles or credentials to bypass failure.
   If an explicitly selected backend differs, resolve it before writing.
2. Search both open and closed discussions in the destination with one to three
   specific command, symptom or instruction terms using `issue-spec search issues`.
   Read plausible matches with `issue-spec read issue --comments`. Do not rely only
   on titles, labels, the first page of a broad query, or a search snippet. Follow
   relevant remaining pages or narrow the query; disclose bounded search coverage.
3. Reuse an existing Issue for the same cause and expected correction. Add only new
   evidence or a distinct recurrence through `issue-spec comment create`; do not
   repeat an unchanged report. Respect closed/wontfix decisions. A demonstrated
   regression after a fix may warrant a new Issue linked to the old one; do not
   reopen it automatically. Split unrelated causes even if their symptoms match.
4. Prepare a self-contained English body with sanitized reproduction, expected and
   observed behavior, impact, owning layer, suggested correction and acceptance.
   Distinguish measured facts from hypotheses. A design gap can use an observed
   task sequence rather than a failing command. Do not manufacture a bug to fill
   a category or attach private logs, credentials or local absolute paths.
5. On GitHub, provision missing labels first, then create the ordinary suggestion
   with `gh issue create --body-file ... --label ...` so creation includes its
   classification. This is HiRoute's explicit feedback-publication policy. Use
   issue-spec for selected workflow artifacts and ordinary recurrence comments;
   non-GitHub Issue creation follows its configured issue-spec backend. Use installed
   command help rather than invented flags. No Proposal, typed QUESTION/TASK or
   PROCESS is needed just to collect feedback.
6. Apply `improvement/feedback` and exactly one category label from the table to
   every report, including a reused matching Issue. Preserve unrelated labels.
   Provision missing labels without overwriting existing definitions. For a reused
   Issue with conflicting category labels, resolve ownership before changing them;
   do not silently remove others' classification.
7. Read back the Issue URL, body/comment and labels. If a create returns an uncertain
   result, search/read before retrying. If labeling fails after creation, retain the
   URL and report the missing labels; retry only that step after resolving the
   failure. Never create a replacement Issue just to retry labels, switch providers
   after a denial, or claim the labeled feedback is complete when it is partial.

Report the Issue URL, category, create/reuse result and any incomplete labeling.
If nothing actionable was found, say so without filing placeholder Issues.

## Review an interval

When asked, collect the umbrella label from both repositories for the requested
interval, including closed Issues. Use creation time for new reports and dated
occurrence comments for recurrences; updated time is only a discovery aid. Group
by underlying cause and layer, link representative evidence, distinguish unique
Issues from occurrences, and prioritize by observed impact and recurrence.
Propose common fixes upstream and project-specific fixes locally. Summarizing
feedback does not authorize implementation, closure, relabeling history or a schedule.
