# System One routing for long-running agents

[Read the manuscript](main.md)

**HiRoute: System One Routing for Long-Running Agents with History-Aware Protection**

This is the Markdown manuscript for community review before final typesetting and
arXiv submission. The research snapshot is dated 18 October 2026. It studies how a
System One decision interface can support routing that combines current task
demand with historical performance and the present applicability of an execution
problem. HiRoute supplies the open-source implementation of that decision contract.

## Reading guide

- [Method](main.md#3-method): state boundaries, three judgments, continuous scoring
  and the deterministic routing rule.
- [Evaluation](main.md#4-policy-conditioned-evaluation): declared engineering and
  mathematical policies, bilingual construction, AI roles, AI-assisted human review,
  and separation of determinate, boundary and insufficient cases.
- [Results](main.md#5-results-and-mechanism-evidence): direct and decomposed selectors,
  protection and release, and the propagation of component errors into actions.
- [MR010_b sensitivity](main.md#56-a-disputed-evaluability-choice-mr010_b): a disputed
  evaluability choice, reported separately from the original labels and main tables.
- [Decision costs](main.md#observed-decision-costs): API-reported Jev charges, including
  approximately USD 0.084–0.086 per 1,000 three-question requests in the measured workloads.
- [Implementation](main.md#6-hiroute-implementation-and-reproducibility) and
  [limitations](main.md#7-limitations-and-conclusion): what the evidence establishes
  and what remains untested.

The strongest supported claim is conformance to the declared routing policy on the
reported synthetic checkpoints, with observable protection, release and failure
modes. Final action accuracy is not executor task-completion rate or realized cost
savings. The manuscript does not claim a universal backend ranking or a universal
advantage of three questions over a well-specified direct selector.

## Exact study questions

These files are byte-preserving copies of the frozen research question definitions.
The [policy manifest](policy-manifest.json) records their SHA-256 hashes. They are
study artifacts, not new product defaults or a replacement for the production
configuration contract.

| Question file | Use in the manuscript |
| --- | --- |
| [Engineering: three questions](policies/engineering-three-questions.json) | Complete method in the engineering studies; the development identifier C refers to this configuration |
| [Mathematics: three questions](policies/mathematics-three-questions.json) | Custom-policy study; only the two current-demand criteria differ from engineering |
| [Engineering: original direct](policies/engineering-direct-original.json) | Original frozen direct encoding in Section 5.1; retained with its representation confound disclosed |
| [Engineering: explicit direct](policies/engineering-direct-explicit.json) | Policy-equivalent expression sensitivity in Appendix D; not a newly blind dataset |
| [Mathematics: explicit direct](policies/mathematics-direct-explicit.json) | Contemporaneous direct comparison in Section 5.4 |

For eligible, complete history and valid answers, the paper selects primary when
`s < 0.5 OR (h < 1 AND a >= 0.5)`: `s` is the simple-current-work probability,
`h` is the returned historical scalar on the 0–2 scale, and `a` is applicability.
Otherwise it selects economy. The returned continuous scalar is retained; the
modal grade is not substituted for it. Invalid or ineligible history follows the
product contracts described in the manuscript. The paper's 0.5 simple threshold is
separate from the tested product default of 0.8.

## Research material availability

This directory publishes the manuscript and exact question definitions. It does
**not** yet contain the complete datasets, provider adapters, response archive or
portable offline reproduction package. The experiments' existing local arithmetic
checks are reported in the manuscript; they are not an external reproduction of
this public directory. The reproduction commands elsewhere in `experiments/`
apply to their named case studies, not this manuscript.

The complete research package is intended to include:

- Original bilingual inputs and labels, including boundary and insufficient cases;
  the engineering primary, supplement and paraphrase panels remain distinguishable.
- Model responses and validity accounting needed to reproduce the reported results,
  including failed outputs and a separate record of aborted runs.
- Exact provider adapters, execution parameters, offline aggregation, uncertainty
  estimation, component diagnostics and a separate MR010_b sensitivity overlay.
- Anonymous construction and audit provenance, AI-assisted human-review summaries,
  a data card, environment information and version checksums.

The public draft identifies AI construction and review systems by role, without
disclosing their model identities, and describes the hosted decision access service
without naming it. Evaluated Jev/Qwen versions, AI participation, procedural
separation, numerical settings, usage charges and transport differences remain
explicit. These disclosure limits constrain exact regeneration and service-level
reproduction. They do not convert AI review into independent human review.

Original labels and reported main tables are retained. Any proposed relabeling or
analysis change must be explicit and separate from the frozen results. Private
reviewer submissions, invitation links, identity mappings, authentication details
and workstation configuration are not part of the public research materials.

The [pinned HiRoute implementation](https://github.com/higress-group/HiRoute/tree/62186edf22550c24807dd4606a36c31727840598)
is already public. Its controlled tests establish the specific production paths
reported in Section 6, not live executor utility or complete replay of the paper's
saved responses under its 0.5 threshold.

## Contributing to the manuscript

Please comment on the draft PR or propose focused edits to `main.md`. Useful review
includes whether the routing objective is clear, whether each claim matches its
reported population and comparison, and whether the examples and limitations make
the result understandable to agent developers. Keep concerns about annotations,
protocol variants and statistical interpretation explicit rather than silently
changing the existing results.

The frozen question files document the evaluated configurations. A proposed new
protocol belongs in a separately named variant, not an overwrite of those files.
Authors, affiliations, acknowledgments and final publication metadata will be
confirmed with contributors before typesetting and submission. This directory
contains no arXiv identifier or claim of peer-reviewed acceptance.
