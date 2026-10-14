# Jev as a read-only exploration tool

Load for dynamic selection on unfamiliar read-only paths. Jev explores entries,
pages/details and visible state, and screens bounded suspicions. Codex owns defect
confirmation, writes and acceptance. See [origin and license](jev-origin.md).

Supply `scenario.review` with only current necessary expectations and numbered
visible facts. The loop adds previous projection/actual action; initially neither
exists. Never send full AX, raw history, credentials or unrelated private data.
Raw evidence stays complete locally. One call selects the next action and answers
`normal / loading / suspected_issue / unknown` with fact IDs. State expected
rejections explicitly; an error label alone does not prove a defect. Assessments
and action answers are independent, assessment passes first. Endpoint screening
needs no next candidate.

Invalid/missing/low-confidence/loading/suspected/unknown results stop for Codex,
not an automatic click retry. Codex may wait boundedly in the original budget,
verify the suspicion or continue an independent segment. Code handles exact
timeouts, repeated actions and numeric comparisons. No suspicion is not business
success; before/after/verify checks remain required. Text-only input cannot verify
pure visual issues such as occlusion. Codex particularly checks Chinese and layered
reasoning, retaining unknown when evidence is weak. The 0.7 confidence threshold
is local policy, not calibrated accuracy. Background references:
[API](https://docs.typesafe.ai/api), [state](https://docs.typesafe.ai/concepts/state),
[capability boundaries](https://docs.typesafe.ai/model-jaggedness/jev-1.13).

## Refine bounded attempts

State goal, current page, allowed region and completion criteria. Use direct CU
for known deterministic one/two-step paths. One failed unfamiliar attempt warrants
trace review and a targeted wording/context/subgoal/candidate adjustment from the
current state, rather than immediately walking the whole path manually. A useful
starting budget is up to three targeted attempts/about three minutes, tuned to the
scenario. Visible progress can continue within the preset budget; do not repeat
an unchanged prompt indefinitely. All attempts share budget and preserve traces,
prompts and results. Never lower criteria, broaden rights or change the user goal
to obtain green. Repeated substantive failure, no new basis or budget exhaustion
warrants Codex takeover; unavailable keys/tools or sensitive boundaries need no
artificial attempt quota. The runtime has no automatic prompt/retry platform.

## Host-owned read-only boundary

Only navigate, open details, expand panels without business effects and scroll.
No form entry, business toggles, save/delete/submit, worker task or real model call.
Mark every candidate `readOnly: true` only after observing its page/region/semantics;
unknown controls must not be marked merely to pass. Button wording, role or model
risk score cannot alone prove read-only behavior. Auto-save/mark-read possibilities
need Codex judgment. Native clicks cannot guarantee no effects; positive scope and
before/after checks remain necessary. Checks permit reasonable bounded exploration,
not unrestricted true. Codex handles writes under normal CU authorization. Prefer
isolated objects and known prerequisites, without generated rollback.

## Input and network

Use OpenRouter [Decisions API](https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-questions-and-answers-request)
with `typesafe/jev-1.13`, not chat completions. Credentials come from explicit
`apiKey`, `OPENROUTER_API_KEY`, or the ignored skill-local `.env` containing
`OPENROUTER_API_KEY=...` / `OPENROUTER_API_KEY_FILE=/absolute/key/file`. They are
only authentication, never scenario content. Only current goal, needed context
and candidates leave the host. Network errors/timeouts stop without hidden retries;
Codex verifies state before retrying and does not infer noise from later success.
A local proxy may be injected via `fetchImpl`; do not source a whole shell config
or change system networking. Record total time, Codex round trips, Jev calls,
attempts/takeovers, including failures/dry-run. Navigation results do not establish
E2E speedup.
