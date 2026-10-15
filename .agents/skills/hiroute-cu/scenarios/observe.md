# OBSERVE: sessions, tasks and usage

Use existing current test markers/time range/request/task references; no fact
means unmet prerequisite. Contracts: session-history and usage-and-savings specs.
Minimum smoke reuses actual model/task execution, finds/opens its UI record and
checks final result, actual model and available usage. Empty pages do not pass;
make an authorized call or report missing facts. Extend filters/pagination with
scope/return, metrics with aggregation/cache weighting/failure usage, sessions
with tool rounds/links. No batch business script; numeric checks are deterministic.

1. Search/page/open/return with stable location; final answer and tool exchanges are visible.
2. Match real requests/tasks, separating user rounds, within-request handoff and
   across-round switching; do not invent unknown links.
3. Compare Home/session/filter values over the same range: input/output/cache/cost,
   no duplicates or lost known failed-attempt usage, token-weighted cache, unknown ≠ zero.
4. Verify performance/score sample and evidence entries when present; unscored ≠
   zero and subscription-equivalent value is not actual payment.

Do not send private bodies to Jev, only safe navigation/synthetic records. Missing
observation and failed execution are distinct; never delete records to hide metric
issues. Default read-only, restore filters/page. Record same-range current facts
and results in the Issue/PR; HTTP 200/empty views do not establish correctness.
