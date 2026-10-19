> Historical construction/review snapshot. Later adjudicated labels are in `datasets/`; provisional wording here does not supersede them. Model identities of construction/review systems are undisclosed.

# Independent mathematical-research author notes

This is the original `independent_math_author_v1` cohort: 16 families, two states per family, and English/Chinese versions of each state, for 64 input records and 64 separate answer records.

## Access and sequence

The only supplied research materials read were `public/BRIEF.md`, `public/POLICY.md`, and `public/DATA-FORMAT.md` in this validation packet. No parent draft, earlier cases, private prompts, model returns, or prior experiment outcomes were inspected. No network service, model API, or additional agent was used. Subsequent reads and writes were confined to this author's output directory.

All 32 original state conversations and their two language versions were drafted before answer annotation. The first complete input and ledger were sealed in `first-drafts/`; its seal records that no labels file yet existed. The complete draft-generation source is preserved there and at the output root. The first labels were then independently authored and sealed in `first-annotations/`, with their generation source and the input hash. There were no conversation revisions after the first draft seal and no label revisions after the first annotation seal.

During final self-review, the construction ledger's scaffold field was expanded to disclose recurring motifs within this newly authored set. This was a metadata-only clarification, recorded in `REVISION-LOG.jsonl`; the initial ledger remains in `first-drafts/`. Common motifs include a requested artifact versus a promise, one unfinished entry in an otherwise useful list, and an open follow-up versus a fixed preparation task. These were not imported from previous benchmark cases. The mathematics, actors, dependencies, and scopes were separately drafted for the situations described in the ledger.

## Interpretation of this set

The normative answers follow the public mathematical-research policy, not a universal complexity scale. No probability, confidence target, or exact historical scalar is supplied. Semantic grades are never equated with those quantities. Boundary records preserve both possible actions as conditional policy branches; they do not mark either action automatically correct.

The observed modes are `human_feedback` for 24 records and the required legacy value `autonomous_continuation` for 40 records. The latter means that no evaluative human feedback is visible in the record. It is not evidence that an autonomous system actually triggered the continuation. Positive human feedback counts as feedback, and assistant self-reports alone do not count as human feedback.

All records use zero prior complete turns. Each assessed exchange itself contains the local instructions, relevant facts, visible output, and any feedback needed for that checkpoint. This was a natural self-contained drafting choice, not an attempt to pad histories to a target length. Some assessed exchanges have more than two messages because an actual correction or local feedback occurs within that same stage.

Thirteen families retain the entire pending request verbatim across states, separately in English and Chinese: MR001–MR005, MR007, MR009–MR010, and MR012–MR016. MR006, MR008, and MR011 require different current reports to express the timing of a completed correction, a real versus absent dependency, or an authorized withdrawal. MR006, MR012, and MR016 retain the same determinate action in both states.

Seven families contain clear new-reasoning work: MR003, MR004, MR005, MR009, MR012, MR014, and MR016. Eight contain a bounded local-problem or insufficient-material variant: MR002, MR005, MR007, MR010, MR011, MR013, MR015, and MR016. These are descriptive counts taken after authoring, not action or success quotas.

The first annotations contain 50 clear records, 12 boundary records, and 2 insufficient records. Of the clear records, 28 have the economy action and 22 have the primary action. Current-demand labels are 44 routine, 18 reasoning, and 2 underdetermined. These counts did not guide rewriting, relabeling, or selection.

## Deliberate distinctions and limitations

- MR006 distinguishes an actual correction completed within the assessed exchange from a copy editor's later completed correction reported in the pending request. Both can support a clear current action while retaining different earlier-stage outcomes.
- MR007 and MR015 distinguish a newly added step from an omission under the original assignment. MR015_a deliberately retains the supported ambiguity between an absent final estimate and useful core progress with one remaining endpoint step; no independent mathematical judgment decides its weight.
- MR008 distinguishes an independent appendix task from a task that still needs the same missing table. A shared manuscript alone is not treated as a dependency.
- MR009 treats a completed bounded screen with no violating examples as useful delivered work. No truth claim about the general graph conjecture is used to grade that screen.
- MR010_b lacks both the marked source and the remaining note. Its current demand is insufficiently evidenced. The visible requirement to display an inventory was nevertheless unfulfilled, so its earlier-stage grade remains 0 rather than becoming unknown merely because other evidence is absent. The insufficient action panel does not erase supported stage or applicability targets.
- MR014 evaluates a plan against a request for a plan. The author's outstanding publication-year reply is an external dependency, not an execution failure. The replacement-lemma question is intentionally described at the collaboration-scope level: the explicitly unproved condition establishes new reasoning demand, without asking the annotator to recover a theorem statement or prove applicability. A later human review may judge whether the abstraction is natural enough for use.
- MR016_b's omitted N=1 expansion is not made applicable solely because the next task is also about series. The general finite identity is already supplied and is the stated starting point for the new convergence question. The infinite product's justified scope, including the relevant limit/summation interpretation, remains part of the research task; no specific theorem answer is used as hidden truth.

The bilingual versions were directly authored and reviewed for actor, chronology, authority, scope, formulas, and evidence. Annotation targets match across languages, while each version's evidence quotes come from its own conversation. This is author self-review, not a separate human domain/naturalness review or an independent translation review.

## Files and validation

- `inputs.jsonl`: input-only conversations; only the `conversation` object is intended for evaluated decision mechanisms.
- `labels.jsonl`: separate first independent targets, conditional answers, concise rationales, and verbatim evidence.
- `family-ledger.jsonl`: construction outlines, original inspiration, scope differences, pending-text identity, feedback observations, and internal motif disclosure.
- `draft-generation-v1.py`, `annotation-generation-v1.py`: preserved creation sources. They use exclusive writes for first inputs and labels.
- `first-drafts/`, `first-annotations/`: sealed original materials. No later model evaluation influenced them.
- `finalize-and-validate-v1.py`, `VALIDATION-REPORT.json`: author-side transport and preservation checks. The finalizer is an auditable one-time script, not a rerunnable mutator of first materials.
- `REVISION-LOG.jsonl`: the sole metadata-only ledger clarification.
- `manifest.json`: SHA-256 and byte size of every material under this output directory, with UTC creation time; the manifest itself is excluded to avoid a circular hash. The first-drafts directory also has its own earlier manifest.

Evidence arrays contain complete verbatim messages with exact corresponding message locations. Retaining the whole relevant message preserves qualifications and chronology instead of extracting only convenient fragments. Validation checks all quote/location pairs, ID coverage, bilingual annotation equality, legal panel/action structure, pending-text identity, and byte-for-byte preservation of the first conversations and first answers. It does not independently solve mathematics, test executor capability, or replace the separate anonymous-input and human reviews.
