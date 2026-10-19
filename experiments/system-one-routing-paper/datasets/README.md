# Bilingual evaluation datasets

Each row is a JSON object. Join inputs and labels by `id`; keep workflow families,
state pairs and translations together. Read the [data card](../DATA-CARD.md) for
the policy, provenance, intended use and boundaries.

| Dataset | Inputs | Original labels | Scope |
|---|---|---|---|
| Engineering | [128 input records](engineering/inputs.jsonl) | [128 label records](engineering/labels.jsonl) | 104 primary records plus 24 separate paraphrases |
| Mathematics | [64 input records](mathematics/inputs.jsonl) | [64 label records](mathematics/labels.jsonl) | Custom current-demand policy; unchanged historical mechanism |
| Development V3 | [384 input records](development-v3/inputs.jsonl) | [384 label records](development-v3/labels.jsonl) | Development-era diagnostics, not independent validation |
| Direct probes | [Eight probe inputs and expectations](direct-probes/probes.jsonl) | Included in each probe | Interface development only |

The engineering [family metadata](engineering/family-metadata.jsonl) records
construction relationships. Original labels include clear, boundary and
insufficient-evidence cases. The [MR010_b sensitivity](../sensitivities/mr010.json)
is a separate overlay; do not replace the original mathematical labels with it.

Use the [offline evaluator](../REPRODUCE.md) to inspect a complete case, reproduce
the paper results, or score a generic or typed routing mechanism.
