# Switching Models Loses Cache. Does Smart Routing Still Save Money?

Smart routing gives different work to different models. A natural question follows: once the model changes, the cache built up so far is no longer usable. Could that loss eat the money routing saves?

**Short answer: some cache is lost, but nowhere near enough to cancel the savings.**

- **Switches are rare to begin with.** HiRoute keeps the same model through consecutive tool calls within a turn, and only reconsiders at points such as a new user message or context compaction.
- **The loss happens once and gets spread out.** Only the first request after a switch misses the cache; later calls use the cache again.
- **Switching to a cheaper model can even lower the cost of old context.** A cheap model processing it from scratch often costs less than an expensive model reading its cache.
- **The whole-task margin is large.** In the Astra × Qwen experiment, even if every all-Astra input token hit the cache, mixed routing was still about 90% cheaper per task; Astra would need a price cut of about 95% for the two to break even.

HiRoute manages these costs at its switching points, so you do not need to work out the math before using Smart Saving. The sections below explain each point.

## First: what does caching save?

Each time an agent calls a model, it resends the whole conversation: system instructions, tool definitions, earlier messages and tool results. The longer the conversation, the more content is repeated on every call.

KV Cache lets the model remember the beginning it has already processed, called the "prefix" below. When the next request starts the same way, the model reads the cache and charges the cache-read price, typically around a tenth of the ordinary input price.

Caches belong to a model. A model that has never seen this history must process it from scratch; only later requests can read its own cache. So the cache loss from a switch is this: **a prefix that could have been read at the cache price is processed once at full price on the new model.** It is a one-time cost, not one repeated on every later call.

## How HiRoute reduces switching

HiRoute's context-hold mechanism, ContextHold, follows the rhythm of the conversation to decide when a model change is allowed:

| When | What HiRoute does | Effect on caching |
| --- | --- | --- |
| Consecutive tool calls within a turn | Reuse the choice already made for the turn and keep the current model | No switch; the cache keeps growing with the conversation |
| A new user message | Reassess the task; keep or change the model | Keeping it loses nothing; changing it loses the old prefix once |
| After compaction or a history rebuild | Keep the choice if it still applies; otherwise reassess | Part of the history must be rebuilt anyway, so switching adds less |

Most calls in a turn are tool round-trips, and these do not change models. In one unattended coding run of 40 requests, 35 reused an existing choice directly, or 87.5%.

Whether a request actually hits the cache also depends on the provider's caching rules and lifetime, and an unavailable model can still fail over to a backup. See the [model-routing guide](https://hiroute.ai/en/docs/model-routing/) for the rules.

## One switch, spread across later calls

Picture this: your conversation with an agent has reached 100,000 tokens, you send a new message, and routing decides to change models.

- **The first request after the switch:** all 100,000 tokens must be processed again, so this request's hit rate falls from 99% to 0. Seen in isolation, the loss looks large.
- **The tool calls that follow:** the new model now has this conversation cached, matching the no-switch case.

[![After one switch, the turn's hit rate recovers with more calls: 99 points lower after one call, only 4.2 points lower after twenty](assets/cache-amortization-en.svg)](assets/cache-amortization-en.svg)

The more calls in a turn, the thinner the loss is spread: **with n calls in a turn, one switch lowers the turn's hit rate by at most 1/n.** In the experiment records, drafting and reviewing turns averaged about 7 calls, and long coding work ran a dozen or more in a row.

The case to watch is a long conversation with only one or two calls per turn and frequent model changes, where the loss has nothing to spread across. HiRoute reconsiders only at new messages and compaction precisely to avoid that kind of back-and-forth.

## Switching after compaction loses less

When context grows too long, the agent compresses early history into a summary. After that, even the original model must process the summary and what follows. What can still hit the cache is usually only the unchanged part, such as system instructions and tool definitions (see Claude's [compaction and caching documentation](https://platform.claude.com/docs/en/build-with-claude/compaction-threshold#maximizing-cache-hits-with-system-prompts)).

Switching at that point therefore loses only this remaining prefix, not the whole history. **Continuous execution reduces how often you switch, and switching at compaction reduces how much each switch loses.**

## In dollars: what one loss costs

The figures below use the frozen prices from the Astra × Qwen experiment, in USD per million tokens. Qwen's CNY prices are converted at a fixed exchange rate:

| Per million tokens | Astra | Qwen 3.8 Flash |
| --- | ---: | ---: |
| Ordinary input | 10 | About 0.12 |
| Cache read | 1 | About 0.015 |
| Cache write | 12.5 | About 0.19 |
| Output | 50 | About 0.40 |

Estimated on the expensive side, one cache loss adds:

- **Astra loses 100,000 tokens:** about **$1.15**.
- **Astra loses 10,000 tokens** (for example, only system instructions and tools left after compaction): about **$0.115**.
- **Qwen loses 100,000 tokens:** under **2 cents**.

The cost is paid once. A long cached prefix is genuinely worth money, which is why HiRoute works to keep execution continuous.

### Switching to the cheaper model improves the bill

A real switch also involves the price difference between models. For the same 100,000-token old prefix:

| Direction | No switch: original model reads cache | Switch: new model processes again | Result |
| --- | ---: | ---: | --- |
| Astra → Qwen | $0.10 | About $0.019 | **About $0.08 saved** |
| Qwen → Astra | About $0.0015 | $1.25 | About $1.25 more |

**Smart routing saves money mainly by moving work to the cheaper model, and in that direction the cache loss is close to negligible; moving up to Astra pays for stronger capability.**

## Back to the whole task: savings far exceed cache overhead

What ultimately matters is the bill for the whole task. In the [Astra × Qwen experiment](https://hiroute.ai/en/news/astra-qwen-smart-routing/), Qwen handled six source-verification components and Astra handled the critical decision, compared against using only Astra. In two of three paired runs, both approaches met the acceptance standard; the remaining pair's results are kept in full in the [experiment record](../experiments/cases/research-cost-quality/README.md).

| Run where both met the standard | Mixed routing | All Astra | Saving |
| --- | ---: | ---: | ---: |
| Pair 2 | About $0.147 | About $1.84 | 92% |
| Pair 3 | About $0.178 | About $2.07 | 91% |

Now give all-Astra the most favorable case: every input hits the cache, while Qwen in mixed routing never hits it at all. Mixed routing is still about 90% cheaper per task, with about $1.65 of headroom left. However good the cache, it only lowers input charges, not output.

[![Per-task cost compared with one cache loss: all Astra costs about $1.83–$2.07, mixed routing about $0.178, leaving about $1.65 of headroom; losing 10,000 tokens of Astra cache adds only $0.115](assets/cache-headroom-en.svg)](assets/cache-headroom-en.svg)

Losing 10,000 tokens of Astra cache once uses about 7% of that headroom; the full headroom equals roughly another 140,000 tokens of Astra cache rebuilding.

## What if the primary model gets cheaper?

Primary models often get cheaper. Suppose every Astra price falls by the same proportion and everything else stays the same:

[![All-Astra cost as a multiple of mixed routing as Astra's price falls: about 10.3× today, about 3.4× at 20%, and break-even at about 5.1%](assets/cache-price-break-even-en.svg)](assets/cache-price-break-even-en.svg)

Halve Astra's price and all-Astra still costs about 6.8 times as much as mixed routing; cut it by 80% and it is still about 3.4 times. **Only a price cut of about 95% brings the two to break-even** — and that is with the calculation most favorable to all-Astra.

## Bottom line: keep the economy model working, upgrade when needed

HiRoute aims to let an economy model keep handling the work it suits, and bring in a primary model when stronger capability is needed. Complex work can start on the primary model, and weak economy-model performance can trigger an upgrade, avoiding repeated rework just to save on individual calls.

- **Keep the model stable within a stage** to accumulate cache benefits.
- **Reassess at rebuild points such as compaction** to avoid unnecessary cache losses.
- **Upgrade when needed** to protect the delivery standard.

Cache losses are one-time, bounded costs, and HiRoute controls when switches happen. **Moving suitable work to a clearly cheaper model saves far more than cache rebuilding, routing decisions, and rework cost.** In the experiment above, the gap is about an order of magnitude.

## Calculation basis

- **Prices:** the experiment's API-equivalent prices frozen on October 3, 2026, at the standard context tier. CNY is converted to USD at the fixed September 30 rate of `1.1355 / 7.6130`, calculated at full precision. These are historical analytical references, not current quotes or subscription bills. Original records are under `evidence.json → frozen_cost_basis` in the [frozen evidence archive](https://github.com/higress-group/HiRoute/releases/tag/experiment-evidence-2026-10-04).
- **Hit-rate example:** a token-weighted input hit rate, calculated rather than measured. The new message is about 1,000 tokens and each later tool call adds about 2,000. It assumes identical input on both paths, a valid original cache, no cache at all on the new model's first request, and no further switches, compaction, or expiration. If the new model already holds part of the prefix, count only the difference; if a turn switches several times, add up each loss. The 87.5% figure measures decision reuse, not a cache-hit rate.
- **Call counts:** a turn runs from one user instruction to the end of that native Agent turn; a call is one business-model request. One response can contain several tool invocations, and routing-decision requests are counted separately. Writing counts come from the [per-turn structured records](../experiments/results/decision-routing-20261006/results.json) for Qwen 3.8 Max and GLM 5.3: 310 requests across 42 normally completed turns, 3–19 per turn, median 5.5, mean 7.38, with 51 requests in six interrupted turns reported separately. HTTPX counts come from the [coding experiment](../experiments/cases/unattended-engineering/README.md): one instruction generated 40 requests (25 Qwen, 15 Astra), with two compactions, 30 accepted requests, and 10 cancellations; the final 14 consecutive Astra requests began with a reassessment after compaction.
- **Cache loss:** extra cost = lost prefix length × (first-processing price − cache-read price). Whether first processing is billed at the ordinary input or cache-write price depends on the provider; the article uses the more expensive cache-write price. The switch-direction table already includes the price difference between models, so the cache-loss amounts must not be added again. New input, output, routing decisions, and rework still belong in the whole-task bill.
- **Whole-task costs:** these include all subject attempts, structural retries, and reported Jev decision costs. Some cache-write quantities were not reported and are treated as intervals, so the comparison uses the mixed upper bound against the all-Astra lower bound. Ordinary input, cache reads, and cache writes do not overlap; billed reasoning tokens are already included in output. The all-input-cached Astra cost keeps the observed output usage. The mixed upper bound bills all Qwen input at the cache-write price; Qwen reported zero cache reads in the experiment. Savings are calculated at full precision and rounded down.
- **Price-cut analysis:** a hypothetical price-sensitivity calculation, not a new experiment. Every Astra price falls by the same proportion; Qwen prices, Jev decision costs, both approaches' token usage, and delivery outcomes stay as recorded. The break-even and figure use pair 3; pair 2 gives nearly the same result (about 5.15% of today).
- **Figures:** generated by `experiments/render_figures.py` from the same frozen evidence.
- **Reproduction and scope:** recompute call counts, costs, and hypothetical comparisons with the [offline calculator](../experiments/cache_cost_analysis.py), following the [reproduction instructions](../experiments/README.md). The records support these specific workloads; actual savings depend on a task's context length, call volume, and delivery quality.
