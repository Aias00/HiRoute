# Curated saved model responses

These gzip-compressed JSONL archives retain complete semantic outputs, LLM
completion text, status, attempts, usage charges and timing. They are inputs to
the [offline replay](../REPRODUCE.md), not cached benchmark scores. Metadata
removals and source/public hash differences are documented in the [data card](../DATA-CARD.md).

| Archive | Scope |
|---|---|
| [Engineering matched](engineering-matched.jsonl.gz) | 1,536 planned/results; fully informed original direct versus three questions |
| [Independent engineering](engineering-independent.jsonl.gz) | 1,792 results; one invalid response retained |
| [Mathematics matched](mathematics-matched.jsonl.gz) | 768 results; one invalid Qwen completion retained |
| [Direct expression](direct-expression.jsonl.gz) | 1,024 original/explicit native controls on seen engineering inputs |
| [Direct probes](direct-probes.jsonl.gz) | 32 developer-probe responses, not a blind test |
| [Development V3](development-v3.jsonl.gz) | 6,912 historical responses; includes both direct-LLM threshold conditions |
| [Abandoned engineering](aborted-engineering.jsonl.gz) | 598 starts, 594 results, four unknown outcomes; separate from completed runs |
| [Completed format development](format-development-completed.jsonl.gz) | 20 result-only interface checks, not headline semantic evaluation |
| [Stopped format development](format-development-stopped.jsonl.gz) | One rejected result retained; planned but unobserved entries are not invented starts |

Exact request bodies and planned IDs are indexed by the
[execution guide](../execution/README.md). Preserved unknown outcomes and failures
are not silently retried or filled with results from another campaign.
