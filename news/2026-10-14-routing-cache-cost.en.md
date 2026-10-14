# Model Switching Cache Effects and Smart Routing Costs

Smart routing assigns work to different models. A natural concern follows: could the cache lost in a model switch consume the money saved?

HiRoute reduces that overhead in two ways: **keep the model stable through consecutive tool calls within a turn, and reassess at context-rebuild boundaries where less reusable history may be lost.** For switches that do occur, cache rebuilding belongs in the whole-task bill.

We explain the protections first, then use observed call counts and the Astra × Qwen experiment to calculate cache effects and total costs separately.

## How HiRoute protects cache reuse

KV Cache stores context states already computed by a model. Keeping the model and context prefix stable lets later requests read cached history and process new content. A destination without that cache must process the old prefix again, but subsequent calls can reuse the cache it builds.

HiRoute's context-hold mechanism, ContextHold, recognizes session and history continuity and retains valid execution preferences:

| Execution boundary | HiRoute behavior | Effect on caching |
| --- | --- | --- |
| Consecutive tool calls within a turn | When the turn is recognizable, history is continuous, and the decision is valid, reuse it and prefer the eligible current model in the selected group | Avoid selecting a model afresh on every tool round-trip; reuse the growing prefix |
| New user message | Reassess the task, keeping or changing the model | Keeping the model can retain the old cache; switching requires accounting for the full reusable prefix lost |
| Compaction or history rebuild | Keep a reusable decision; otherwise reassess | Some history already needs rebuilding, so additional losses may shrink to the remaining reusable prefix |

Actual hits depend on upstream cache rules and lifetime; an unavailable candidate can still trigger failover. HiRoute uses continuity to reduce unnecessary switches. See the [model-routing guide](https://hiroute.ai/en/docs/model-routing/) for selection rules.

## Spreading the loss from a new-user-message switch

A new user message usually appends to history, leaving the old prefix reusable. Switching to a completely cold model can lose that cache on the first request. Keeping subsequent tool calls stable spreads the loss across the turn.

The calculations use a token-weighted input hit rate. To isolate one switch, assume identical input sequences on both paths, a valid original cache, a fully cold destination on the first call, and normal prefix reuse afterward with no further switches, compaction, or expiration. Define:

- `P`: old prefix tokens that would have hit.
- `U`: the new user message and other uncached input tokens.
- `n`: model calls in the turn, including the first call.
- `g`: uncached tokens added by each tool continuation, held constant in this example.

On the first request, staying with the model gives a hit rate of `P / (P + U)`; a cold switch gives zero. With a 100,000-token prefix and 1,000 new tokens, the request drops from **99.01% to 0%**.

Input on call `i` is `P + U + (i − 1)g`, so:

**Total turn input T = n(P + U) + g × n(n − 1) / 2.**

Without switching, uncached input totals `U + (n − 1)g`. A first-call cold switch adds one loss of `P` tokens. Therefore:

**Turn hit-rate reduction = P / T ≤ 1 / n.**

The bound follows from every input containing `P`, so `T ≥ nP`. Under these conditions, fourteen consecutive calls bound the reduction at approximately **7.14 percentage points**, and twenty at **5 percentage points**.

With `P = 100,000`, `U = 1,000`, and `g = 2,000`, the example becomes:

| Calls in the turn | Total input T | No-switch hit rate | First-call cold-switch hit rate | Reduction |
| --- | ---: | ---: | ---: | ---: |
| 1 | 101,000 | 99.01% | 0.00% | 99.01 percentage points |
| 5 | 525,000 | 98.29% | 79.24% | 19.05 percentage points |
| 14 | 1,596,000 | 98.31% | 92.04% | 6.27 percentage points |
| 20 | 2,400,000 | 98.38% | 94.21% | 4.17 percentage points |

**Continuous execution spreads a large first-request loss across the execution stage.** Frequent switches between short turns with long histories remain costly. If the destination has a partially valid prefix, count only the difference; keeping the original model causes no switching loss. Multiple switches require summing their losses rather than applying the single-switch bound.

### How many calls occurred in real experiments

A turn runs from one user instruction to the end of that native Agent turn. A call is a business-model request: one response can contain several tool invocations, and routing-decision requests are counted separately.

| Observed workload | Model calls |
| --- | --- |
| Drafting, reviewing, revising, and final review | 310 requests across 42 normally completed turns; 3–19 per turn, median 5.5, mean 7.38 |
| Unattended Astra/Qwen HTTPX coding | One initial instruction generated 40 requests: 25 Qwen and 15 Astra |
| The final stable Astra segment of that HTTPX task | 14 consecutive requests, all accepted; the first reassessed and 13 inherited the decision |

Writing counts come from the [per-turn structured records](../experiments/results/decision-routing-20261006/results.json) for Qwen 3.8 Max and GLM 5.3. All 48 registered turns are retained, with 51 requests in the other six interrupted turns reported separately. The [HTTPX experiment](../experiments/cases/unattended-engineering/README.md) included two compactions, 30 accepted requests, and 10 cancellations; its final stable stretch of 14 calls supplies the continuous-execution example.

Across HTTPX, **35/40 requests inherited the decision, or 87.5%**. This shows the protection in operation: most tool round-trips reused an existing choice. The 87.5% measures decision reuse; the earlier 6.27-point gap is a calculation using the observed count of 14 with assumed input sizes. Neither is a measured cache-hit rate. That stable segment began after compaction and illustrates execution length, rather than providing a controlled new-user-message switch.

These samples give a practical sense of scale: writing turns often contain several requests, while long coding work can contain over a dozen consecutive requests. Different workloads offer different room to spread a switching loss.

## Why switching after compaction can lose less

Compaction replaces some history with a summary. Even the original model must process that new content, while unchanged system instructions or tool definitions may still hit. For example, Claude's [compaction and caching documentation](https://platform.claude.com/docs/en/build-with-claude/compaction-threshold#maximizing-cache-hits-with-system-prompts) describes retaining a system-prompt cache after compaction.

When comparing staying versus switching after compaction, count only **the prefix that would still hit but is unavailable on the destination** as the additional loss `R`. Both paths must process the summary and new messages.

Under the same single-switch model, **stage hit-rate reduction = R / T**. At one million total input tokens, losing a 100,000-token prefix reduces the rate by 10 percentage points; losing 10,000 reduces it by one point. This comparison fixes the denominator; real compaction also changes `T`.

The protections address different terms: **continuous execution reduces the number of losses, while a rebuild boundary can reduce the prefix lost each time.** Long retained system instructions and tools still contribute to `R`. The additional impact becomes negligible when `R / T` is sufficiently small.

## What the cache loss costs

The calculations use the experiment's **API-equivalent prices frozen on October 3, 2026**, its **fixed September 30 exchange rate**, and the standard context tier. These are historical analytical references, not current quotes or subscription bills.

| Per million tokens | Astra USD | Qwen 3.8 Flash CNY | Qwen equivalent USD |
| --- | ---: | ---: | ---: |
| Ordinary input | 10 | 0.8 | 0.119322 |
| Cache read | 1 | 0.1 | 0.014915 |
| Cache write | 12.5 | 1.25 | 0.186441 |
| Output | 50 | 2.7 | 0.402712 |

The conversion is `1.1355 / 7.6130` USD per CNY, calculated at full precision. Original records are under `evidence.json → frozen_cost_basis` in the [frozen evidence archive](https://github.com/higress-group/HiRoute/releases/tag/experiment-evidence-2026-10-04).

First isolate caching by comparing a warm and cold execution on the same destination model. Let `p` be the first-processing price and `r` the cache-read price, both in USD per million tokens:

**Additional cache cost K = R × (p − r) / 1,000,000.**

Whether first processing uses the ordinary input or cache-write price depends on the provider. These are alternative billing cases, not two full prices to add together.

| Destination and lost prefix | Premium at ordinary input price | Premium at cache-write price |
| --- | ---: | ---: |
| Astra, 100,000 tokens | $0.900 | $1.150 |
| Astra, 10,000 tokens | $0.090 | $0.115 |
| Qwen, 100,000 tokens | About $0.010441 | About $0.017153 |
| Qwen, 10,000 tokens | About $0.001044 | About $0.001715 |

Astra losing a reusable 10,000-token prefix adds $0.115 under the write-price case. Spread across twenty consecutive calls, that is $0.00575 per call. The total remains $0.115, while losing a full 100,000-token prefix can add $1.15. Preserving long-prefix reuse has real value.

### Switching direction also changes the bill

A cross-model comparison must include the price change. When a new user message arrives, the old-prefix portion changes by:

**Old-prefix cost change = P × (destination first-processing price − original cache-read price) / 1,000,000.**

For `P = 100,000`, with destination processing charged at the cache-write price:

| Direction | Read old prefix on original model | Rebuild on destination | Cost change |
| --- | ---: | ---: | ---: |
| Astra → Qwen | $0.100000 | About $0.018644 | About $0.081356 less |
| Qwen → Astra | About $0.001492 | $1.250000 | About $1.248508 more |

**Even with a cache loss, processing the old prefix on Qwen can cost less; upgrading to Astra pays for stronger capability.** This table includes the cross-model price difference, so the preceding cache premium must not be added again. New input, outputs, routing decisions, and rework still belong in the whole-task bill.

## Put cache overhead back into the whole-task cost

The [Astra × Qwen experiment](https://hiroute.ai/en/news/astra-qwen-smart-routing/) assigned six source-verification components to Qwen and a critical decision to Astra. Two of three paired runs met the original whole-delivery standard in both arms. The mixed delivery that missed the first pair's gate and all results remain in the [experiment record](../experiments/cases/research-cost-quality/README.md). This component-routing comparison establishes a cost margin for the workload.

Whole-task costs include all subject attempts, structural retries, and reported Jev decisions. Unreported cache-write quantities produce cost intervals, so the comparison uses the mixed upper bound against the all-Astra lower bound. Ordinary input, cache reads, and cache writes do not overlap; billed reasoning tokens are already included in output.

| Pair accepted in both arms | Mixed upper bound | All-Astra lower bound | Conservative saving |
| --- | ---: | ---: | ---: |
| Pair 2 | $0.146806 | $1.838882 | 92.01% |
| Pair 3 | $0.177957 | $2.067170 | 91.39% |

### Even if every all-Astra input token hits

Give all-Astra an ideal cache: charge every input token at the cache-read price, keeping observed output usage, while mixed routing stays at its recorded upper bound. This fixed-usage sensitivity calculation checks whether the cache discount changes the conclusion.

Each all-Astra run used 116,383 input tokens, with 34,214 and 34,310 output tokens respectively:

`Pair 2: (116,383 × 1 + 34,214 × 50) / 1,000,000 = $1.827083`

`Pair 3: (116,383 × 1 + 34,310 × 50) / 1,000,000 = $1.831883`

| Pair | Hypothetical all-input-cached Astra | Mixed upper bound | Remaining saving |
| --- | ---: | ---: | ---: |
| Pair 2 | $1.827083 | $0.146806 | 91.96% |
| Pair 3 | $1.831883 | $0.177957 | 90.28% |

The advantage survives for both tasks: input caching does not eliminate output charges, and Qwen's ordinary input price is below Astra's cache-read price at these rates. Calculations retain each model's different observed output volume, with savings calculated at full precision and rounded down to two percentage decimals.

### Room for additional overhead

Against the idealized all-Astra cost `C_A*` and mixed upper bound `C_M`, the break-even condition is:

**New overhead X < C_A* − C_M.**

The margins are approximately **$1.680277** and **$1.653926**. One extra $0.115 Astra prefix-write loss would not offset the savings in this example. `X` includes only cache rebuilding, decisions, or rework added beyond the recorded bill, avoiding double counting.

Reproduce the call counts, costs, and hypothetical comparisons with the [offline calculator](../experiments/cache_cost_analysis.py), following the [reproduction instructions](../experiments/README.md). The records support these specific workloads; a real task should be assessed using its own context, call volume, and delivery quality.

## Keep suitable work on economy and upgrade when needed

HiRoute aims to let an economy model continue handling work suited to it, bringing in a primary model when stronger capability is needed. Complex work can start on primary, and insufficient economy-model performance can trigger an upgrade, avoiding repeated rework just to save on individual calls.

Keep the model stable within a stage to accumulate cache benefits. Reassess at rebuild boundaries to reduce avoidable cache losses. Upgrade when needed to protect the delivery standard. **Smart routing has a total-cost advantage when savings from suitable economy-model work exceed additional cache rebuilding, routing decisions, and rework.**
