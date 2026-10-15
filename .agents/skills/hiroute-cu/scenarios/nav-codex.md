# NAV-02: open Codex Agent details

Start on Home with Claude Code last selected in Agent details to exercise both
steps. An already selected Codex endpoint can meet the goal but is not a two-step
rerun. Minimum smoke uses [nav-codex.mjs](../scripts/nav-codex.mjs), entries
`scenario` and `select`, with the current app and budget under [runtime](../references/runtime.md).
This proves navigation only. Extend changed structure with unknown pages,
ambiguity and endpoint checks; Jev `review` is optional for fresh screening, whose
evidence remains separate from navigation.

Prefer the stable batch path. For unfamiliar navigation, give Jev the whole bounded
goal, opening only main navigation and Agent entries marked read-only. Do not
preselect dynamic indices or permit enable/save/recover. Verify the Agent heading
and Codex subheading independently with complete current AX. Local `select`
requires unique semantic matches, no model/API calls; before/after checks verify
Home → Agent and Claude → Codex. Unexpected dialogs/sheets stop.

Missing candidates, risk boundaries or invalid actions return to Codex, never
substitute Enable for navigation. Restore Claude details/Home from fresh AX if
required, without changing routes. The batch fragment is reusable but must pass
on this package. Record fresh revision, counts, trace and limits in the Issue/PR;
o historical navigation result establishes current business acceptance.
