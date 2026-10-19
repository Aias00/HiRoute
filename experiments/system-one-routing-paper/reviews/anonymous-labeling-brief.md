> Historical construction/review snapshot. Later adjudicated labels are in `datasets/`; provisional wording here does not supersede them. Model identities of construction/review systems are undisclosed.

# Independent review of engineering routing cases

Read only the supplied public policy, data format, anonymized input packet, and this brief. Do not read candidate prompts, old datasets, previous reviewer decisions, experiment plans containing results, evaluated model outputs, or product code. Do not spawn agents or call model APIs. Report proactively; do not wait to be polled.

## Phase A: independent labels

Annotate every anonymized input using only its visible context and the public policy. You receive no author labels or family mapping. Preserve boundary and insufficient-evidence distinctions. Give exact evidence spans, scope and chronology for every judgment. Do not infer code correctness or future executor capability. Do not infer one case's answer from a similar-looking case or aim for an expected class balance.

Save all initial labels and a list of doubtful inputs, including unnatural conversation, ambiguity, missing evidence and visible answer leakage. Record your accessed files. Freeze the initial labels with a content hash and report completion before requesting any author material. These first judgments must never be overwritten.

## Phase B: reconciliation after the initial labels are frozen

Only now receive author labels, original IDs, workflow/translation/equivalence maps and construction notes. Compare:

1. Are labels supported by evidence and the public criteria? Distinguish contradiction of a unique answer from narrowing an allowed set.
2. Do Chinese and English preserve actors, authority, scope, chronology and evidential strength?
3. Are alternate expressions genuinely equivalent, including the absence of evidence? Do they leave all uncertainties unchanged?
4. Are state contrasts natural, with the claimed factual difference actually present? Do not force an action change or invariance merely because it was intended.
5. Are apparently independent workflows sharing a scaffold? Disclose shared construction groups, without claiming that a different feature name removes dependence.

For each disagreement preserve both initial answers, the evidence examined, and the final decision or unresolved set. Input repairs must be justified by the policy or readability, not by anticipated model errors. Save before/after text and re-annotate changed records explicitly. Do not force consensus or discard difficult records to increase coverage. No evaluated model response is available at either phase.

Return an auditable ledger, reviewed inputs/labels, bilingual and equivalence findings, and a candid assessment of whether the data support a prospective generalization test. This is AI review; do not label it human validation.
