> Historical construction/review snapshot. Later adjudicated labels are in `datasets/`; provisional wording here does not supersede them. Model identities of construction/review systems are undisclosed.

# Independent generalization v1 — author notes

## Delivered material

The current files in this directory are the revised author delivery (v2). `drafts/v1/` preserves the first emitted conversations, labels, ledgers, equivalence maps, and generating source. `drafts/v2/` preserves the delivered revision. `repair-log.jsonl` records the changes; no original input or label was overwritten inside the v1 snapshot.

- `outlines.jsonl`: 24 situations and stage-responsibility outlines, fixed as IG001–IG024 before conversation drafting.
- `inputs.jsonl`: 96 base records (24 workflows × 2 states × 2 languages), followed by 24 alternate-expression records. Only the `conversation` object is evaluation input.
- `author-labels.jsonl`: 120 provisional reference judgments, separate from inputs, each supported by quotations at explicit message locations. There are no continuous gold probabilities or scores.
- `construction-ledger.jsonl`: per-family origins, factual contrasts, concrete dependencies, exact-text reuse, and outer dialogue-structure reuse.
- `equivalence-map.jsonl`: 24 source-to-reworded records containing 284 atomic fact mappings. Each mapping quotes the complete containing message on both sides; these are not minimal clause-level spans.
- `expression-selection.json`: the full 24-ID SHA256 ordering, digest parity, and selected source states.
- `checks/self-check.json`: structural, coverage, and quotation checks. `manifest.json` hashes the delivered files.

## Material accessed

The only pre-existing task-content files read were these three, read completely:

1. `/example/research`
2. `/example/research`
3. `/example/research`

Afterward I read only files I created under `/example/research`, including authoring/check/finalization scripts, the generated JSONL files, the selection record, and revision snapshots. The local Python standard library and shell were used to serialize and check these artifacts. No parent research directory, previous case, benchmark, candidate prompt, model result, implementation source, browser material, or external model endpoint was inspected. No additional agent was started. Progress was sent only to the assigning parent through the provided agent channel. This is a behavioral access record, not a claim of operating-system sandbox isolation.

The engineering situations were composed from general engineering knowledge and the three supplied requirements. No specific real repository or incident is their source. All entities, observations, and outcomes in the conversations are synthetic.

## Construction and coverage

The outlines were saved first, followed immediately by the fixed hash selection; the selection was not conditioned on labels or anticipated difficulty. The selected source states are IG012 b, IG001 b, IG017 a, IG011 a, IG005 a, IG015 b, IG003 b, IG024 b, IG014 b, IG019 b, IG021 a, and IG010 a, in digest order. English and Chinese rewordings preserve the same actors and assessed-stage boundaries, including corrections that happen inside a stage versus corrections reported afterward.

There are 12 human-feedback workflows and 12 autonomous-continuation workflows. Here “human-feedback” means that the assessed stage contains a substantive user observation or correction after an assistant response. “Autonomous-continuation” means that the checkpoint continues from the assistant's record without a human evaluative reply inside that stage. It does not imply that every surrounding engineering action was performed by software; a continuation can report a service owner's later action. Mode was assigned with the outlines and was not used as a judgment rule.

The 24 named situations span API and data behavior; migration, recovery and query work; builds and dependencies; accessibility and localization; concurrency and scheduling; incident and credential operations; rollout and signing; observability and capacity; and documentation. These broad groupings exceed the required six activity kinds without treating bilingual records as new activities.

Eighteen workflows have byte-identical pending request text across states in each language. The ledger records the exact strings. Twenty workflows contain a concrete historical relation to inspect, including persistent dependencies, executed corrections, withdrawn requirements, independently authorized work, and out-of-scope defects. IG004, IG012, IG014 and IG023 contain local residual problems; IG016 and IG020 retain explicit scope uncertainty. IG024 b intentionally retains insufficient outcome evidence rather than guessing hidden correctness.

The conversations did not begin from one universal content template. Outer structures do recur: short autonomous checkpoints often use request/report/continuation, and feedback checkpoints often add a user observation. These shared shapes, plus paired-state text reuse, translation dependence and expression-check dependence, are disclosed per family. The implementation helpers also share schema construction and quotation validation. None of those dependent records should be treated as independent workflows.

No action quota, desired ranking, target accuracy, or model-specific expectation was used. Counts in the self-check are descriptive outputs after authoring. The reference judgments concern only conformity to the supplied policy, not likely executor success or benefit from escalation.

## Revisions and checks

The first emitted draft is retained. Three content/annotation issues were repaired: IG016 a now distinguishes a core client failure from an optional sample outside the assigned deliverable, with grade and applicability uncertainty preserved; IG020 b now states the two same-title work cards explicitly, so its scope alternatives are visible rather than invented by the annotation; and the IG011 a rewording now describes usability of the recovery without broadening the claim to the entire system. The construction ledger also gained explicit role-shape reuse lists. IDs, outlines and hash selection were unchanged.

Mechanical checks verified all 120 IDs and corresponding labels, the 96/24 split, hash-selected rewording sources, 12/12 modes, schema values, complete prior turns, grade/action consistency, all 1,146 evidence quotations, and full message coverage by the 284 fact mappings. The deepest earlier context is one complete turn, below the eight-turn limit. Author review checked the English/Chinese counterparts and the rewordings for actor, scope, chronology, authority and evidence strength while drafting and repairing them. Reworded labels were carried over only after checking those preserved facts, and all their evidence locations were rebound to the reworded text.

## Actual limitations

This is compact synthetic evidence, usually more explicit than messy working conversations. The 24 workflows provide distinct task situations but do not establish representativeness of real engineering traffic. Context depth is shallow, turn patterns recur, and neither translation nor labels have yet had independent review. A mechanical match of label fields cannot establish semantic bilingual equivalence.

The author judgments remain provisional. In particular, the core-versus-local interpretation of a missing decision or package component (IG013 and IG014), and the concrete applicability of a persisting write-path defect to adjacent work (IG021 and IG023), deserve independent scrutiny. Boundary records do not count both actions as correct, and the insufficient record supplies no fabricated alternatives. The current base states comprise 41 clear, 6 boundary and 1 insufficient state; the extra language and expression records are dependent counterparts.

No underlying software was executed and no hidden correctness was checked. Concrete success and failure observations are statements inside the synthetic conversations. There is no evidence here about actual future executor utility, model ranking, deployment safety, or empirical task success.
