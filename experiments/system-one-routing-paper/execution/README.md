# Execution plans and exact request bodies

[studies.json](studies.json) indexes all nine campaigns, their planned counts,
response records, dataset association, expected model identities and archive paths.
[backends.json](backends.json) retains the evaluated Jev/Qwen versions and relevant
transport conditions without naming the hosted decision access service.

Each campaign directory contains `study.json`, `jobs.jsonl.gz` and
`requests.jsonl.gz`. Plans preserve the original request order, identifiers,
repeats and exact semantic request bodies. Some native templates are shared by
both model versions; their jobs and backend table supply the model identity.
Other archives store complete per-job bodies. Neither format exposes labels to
the model. Gzip reduces repeated prompt text; no custom archive format is needed.

| Campaign metadata | Exact requests | Job plan |
|---|---|---|
| [Engineering matched](engineering-matched/study.json) | [Requests](engineering-matched/requests.jsonl.gz) | [Jobs](engineering-matched/jobs.jsonl.gz) |
| [Independent engineering](engineering-independent/study.json) | [Requests](engineering-independent/requests.jsonl.gz) | [Jobs](engineering-independent/jobs.jsonl.gz) |
| [Mathematics matched](mathematics-matched/study.json) | [Requests](mathematics-matched/requests.jsonl.gz) | [Jobs](mathematics-matched/jobs.jsonl.gz) |
| [Direct expression](direct-expression/study.json) | [Requests](direct-expression/requests.jsonl.gz) | [Jobs](direct-expression/jobs.jsonl.gz) |
| [Direct probes](direct-probes/study.json) | [Requests](direct-probes/requests.jsonl.gz) | [Jobs](direct-probes/jobs.jsonl.gz) |
| [Development V3](development-v3/study.json) | [Requests](development-v3/requests.jsonl.gz) | [Jobs](development-v3/jobs.jsonl.gz) |
| [Abandoned engineering](aborted-engineering/study.json) | [Requests](aborted-engineering/requests.jsonl.gz) | [Jobs](aborted-engineering/jobs.jsonl.gz) |
| [Completed format development](format-development-completed/study.json) | [Requests](format-development-completed/requests.jsonl.gz) | [Jobs](format-development-completed/jobs.jsonl.gz) |
| [Stopped format development](format-development-stopped/study.json) | [Requests](format-development-stopped/requests.jsonl.gz) | [Jobs](format-development-stopped/jobs.jsonl.gz) |

The two format-development ledgers recorded results without separate start events.
Their metadata keeps unobserved plan entries distinct from observed starts.
Abandoned, diagnostic and completed campaigns are never pooled into one accuracy.
The [reproduction guide](../REPRODUCE.md) describes offline parsing and evaluation;
[response archives](../responses/README.md) retain the recorded outputs.
