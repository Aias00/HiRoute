# HiRoute: System One Routing for Long-Running Agents with History-Aware Protection

Community-review manuscript · research snapshot: 18 October 2026. This Markdown version is intended for collaborative review before typesetting and arXiv submission. It retains the frozen labels and principal result tables, reports the MR010_b sensitivity separately, and distinguishes AI construction from AI-assisted human review. Exact study questions, bilingual datasets, curated responses and an offline reproduction package accompany [the reading guide](README.md).

## Abstract

Model routing in continuing agent tasks requires both a judgment of current work and a decision about whether previous failures still matter. We study this problem at bilingual synthetic checkpoints using a System One method implemented in open-source HiRoute. A single typed request separates policy-defined current demand, historical performance and present applicability; application code composes the probabilities and continuous score. On 26 engineering workflows constructed by two AI authors under procedural access separation, Jev 1.13 and 1.14 reach 97.12% and 96.63% workflow-macro action accuracy. Direct-category rewriting substantially changes the comparator, preventing attribution of the original gap to decomposition alone. A prospective mathematical custom-policy study preserves the historical protocol: three-question accuracy is 96.88%, 97.66% and 98.44% for the two Jev versions and Qwen, versus 96.88%, 95.31% and 92.19% for contemporaneous explicit-direct selectors; all main-panel difference intervals include zero. A separately disclosed evaluability sensitivity changes Qwen's three-question score to 95.31% without altering frozen labels, showing that backend ranking depends on the scoring population. Component and action correctness differ through conditional gating, harmful propagation and accidental cancellation. AI construction, AI review and AI-assisted human participation are distinguished. The locally auditable evidence and pinned open implementation support scoped policy conformance and inspectable historical protection, rather than universal superiority, improved executor outcomes or realized savings.

## 1. Introduction

Consider an agent that must edit a supplied configuration value. The current request is explicit and bounded. If the previous stage did not deliver that same required edit, a routing policy may require escalation even though the remaining task is simple. Now suppose the user has completed the edit and asks the agent to update an independent label. The historical failure is still a fact, but it no longer provides the same reason to escalate. Treating these situations alike wastes the temporal information that an agent conversation contains.

HiRoute addresses this problem with a division of responsibility. A decision model interprets the conversation; application code implements the routing policy. The current-demand judgment asks what reasoning remains. The historical judgment asks whether the assessed stage delivered or advanced its assigned work. The applicability judgment asks whether a supported execution problem still bears on the work now required. This last judgment concerns evidence and dependency, not a prediction of whether a particular executor will succeed.

Our focus is the use of System One interfaces for these decisions. We use the term operationally: a service accepts shared state and typed questions and returns structured categories, probabilities or scores. Jev provides such an interface. Its documentation recommends asking bounded questions and composing outputs in software; we treat those statements as an interface design rationale, not proof that the resulting judgments are accurate or their errors independent [1]. We examine observable API behavior and do not infer a proprietary architecture from the outputs.

Task difficulty is policy-dependent. A label such as “simple” is meaningless without a declared scope of work and intended executor roles. The engineering policy studied here distinguishes bounded changes with an established path from unresolved investigation, consequential design choices and intricate interactions. Users may customize this policy. Our evaluation asks whether the router applies that declared standard consistently as a task progresses; it does not define universal difficulty across executor models.

The system contribution is the connection between a System One decision interface and an executable temporal routing policy. Native typed outputs expose the evidence used by HiRoute's composition rule in one request, without requiring the model to emit the final compound decision. An LLM numerical adapter provides a comparison using the same semantic targets. This design does not require Jev to win every backend comparison: it makes the decision contract, its component failures and the resulting action separately testable.

We contribute (i) an open-source implementation of current-demand routing corrected by applicable historical failure; (ii) a policy-conditioned evaluation method with both generic action and typed System One tracks, explicitly separating ambiguity and continuous-score boundaries; and (iii) evidence about protection, release, harmful propagation and accidental error cancellation in bilingual engineering and mathematical contexts. The dataset tests a declared policy through temporal contrasts; the construction and audit method, rather than a universal simple/complex label, is intended to transfer to other policies.

For developers of multi-model agents and gateways, the research questions are: can the fixed mechanism retain protection for an unmet requirement and release it after correction or loss of dependency; how do typed judgments compare with a fully informed direct selector; and which judgment errors change the final action? Sections 5.2–5.5 address these questions, while Section 5.6 tests how an evaluability choice affects their interpretation.

## 2. Related work and scope

Preference-based routing methods such as RouteLLM learn to trade execution quality against resource use [3]. Our immediate target is different: faithful execution of an explicit contextual routing policy. The two targets are complementary. A policy can be implemented correctly yet have poor downstream utility; an executor-quality study is needed to make the latter claim.

Context-guided routing for stateful agents studies history access at user-turn boundaries and the effects of changing executor models [5]. TwinRouterBench separates intermediate-prefix tier evaluation from live task resolution and spend [6]. These studies establish temporal routing and execution-based validation as existing directions. Our contribution concerns the scope and temporal interpretation of historical protection under an explicit policy, with separate measurements of judgments and actions. We do not claim to introduce stateful routing or replace live-execution benchmarks. Arch-Router's use of user-defined preferences further motivates making the routing contract explicit [7].

Our contextual contrasts follow the behavioral-testing principle of testing capabilities through controlled changes rather than relying on an aggregate score [4,8]. We extend that principle to routing checkpoints: the pending request may remain fixed while prior delivery, correction, requirement validity or dependency changes. Translations, state variants and paraphrases are correlated observations, so the workflow is the principal statistical unit.

Typed decision interfaces make the intermediate judgments directly observable. A Jev Score returns a distribution over ordered rubric levels and a continuous scalar [2]. An LLM can also be instructed to emit numerical judgments, but those reported numbers and a native decision distribution arise through different mechanisms. Neither is presumed calibrated. A matched backend-by-format experiment is required before attributing differences to decomposition or backend; an obsolete internal protocol is not an adequate principal competitor.

## 3. Method

### 3.1 State and temporal boundaries

At a routing checkpoint the input separates three regions:

| Region | Content | Evidential role |
|---|---|---|
| `prior_context` | Up to eight complete preceding turns | Earlier requirements, settled facts and references |
| `assessment_stage` | The full stage under assessment, subject to explicit product size/partial handling | Requested stage outcome and visible execution |
| `latest_user` | The pending request, kept separate | Current work and any reported feedback or completed correction |

The method organizes original records rather than generating a semantic summary. Raw tool-result bodies are not supplied for scoring. There is no dedicated goal extraction or source-type classifier. An instruction to continue is not inherently negative feedback. A message can report a correction already completed and request a different action that has not been performed; these evidential roles must not be conflated.

The eight-turn background limit is an implementation choice, not an empirically established optimum in the main studies. The independent validation below contains zero or one preceding turn, so it cannot establish long-history robustness or behavior when relevant requirements fall outside that window.

### 3.2 Three independently specified judgments

**Current demand.** Under the published engineering standard, routine work follows explicit requirements and existing patterns. Complex work requires unresolved diagnosis, a consequential choice among alternatives, or intricate architectural, concurrency, state or algorithmic reasoning. Project size and words such as “fix” do not determine the answer. History may resolve references but should not turn a historical failure into a different complexity criterion.

**Historical performance.** The ordinal rubric has three anchors: zero for a supported core non-delivery or persistent core failure; one for useful progress with a bounded unresolved local issue; and two for delivery or reliable advancement without a supported unresolved execution problem. The target is the work assigned to that stage. An honestly documented unsuccessful research direction may fulfill an investigation task; a promised artifact does not fulfill a request to deliver the artifact. Hidden implementation correctness is not independently adjudicated by the router.

**Present applicability.** A historical execution problem must remain unresolved and have a concrete relationship to the pending work: an affected deliverable, immediate prerequisite, or supported common operation or constraint. Shared project membership alone is insufficient. A completed correction or withdrawn requirement can remove applicability; an explicit repair instruction or a promise to finish does not establish completed execution. All three prompts are frozen. Their development identifier, C, is retained in provenance but is not the method's public name.

### 3.3 Continuous score and deterministic composition

Let $s$ be the simple-current-work probability, $h\in[0,2]$ the returned historical scalar, and $a$ the applicability probability. In the paper configuration, with an eligible, complete historical assessment and valid answers:

$$
\mathrm{primary} \iff s < 0.5\;\lor\;(h<1\;\land\;a\geq0.5).
$$

Otherwise the rule selects the economy group. The simple threshold is configurable: this paper fixes it at 0.5, while the tested product default is 0.8. HiRoute stores the normalized historical score $h/2$; the corresponding strict threshold is 0.5. This is algebraically identical, not a second scoring rule.

The modal ordinal label and continuous score are distinct. For example, probabilities $(0.06,0.93,0.01)$ have modal grade one but a weighted score of 0.95. Thresholding the scalar therefore activates historical protection if applicability is high. We retain this behavior and do not label all grade-one cases as necessarily economy. The supplied scalar is used for routing; scalar/distribution discrepancies are recorded rather than silently corrected.

An absent or invalid applicability answer preserves otherwise eligible historical protection. Invalid, partial or incorrectly attributed historical assessments follow the existing product contracts. These operational paths are evaluated separately from valid-answer semantic accuracy.

### 3.4 When a component can change the action

The composition rule gives an exact diagnostic boundary. Holding the other outputs fixed, changing applicability can change the action **if and only if** $s\geq0.5$ and $h<1$. With a current-demand primary decision, or a score at or above one, the applicability output has no effect on that action. Historical-score changes likewise matter only when current work is classified routine and applicability permits protection.

| Observed outputs | Final decision | What an applicability error can do |
|---|---|---|
| $s<0.5$ | Primary | No change; current demand already determines the action |
| $s\geq0.5$, $h\geq1$ | Economy | No change; historical protection is inactive |
| $s\geq0.5$, $h<1$ | Primary iff $a\geq0.5$ | Can add or remove historical protection |

These are properties of the fixed valid-answer rule, not learned model capabilities. A correct applicability judgment may release an obsolete failure, or block propagation of an erroneously low historical score. Conversely, a false applicability judgment and a low score can produce overprotection together. A correct final action can even arise from two wrong judgments. We therefore measure semantic quality on all uniquely labeled components and inspect action-changing errors separately. A masked error remains a semantic error; decomposition is neither majority voting nor a guarantee of independent errors.

## 4. Policy-conditioned evaluation

### 4.1 Two participation tracks

The action track accepts any system that selects economy or primary from the visible record and full declared policy. The typed track additionally evaluates current demand, historical rubric grade, applicability, probability outputs and error propagation. A direct selector is not penalized for lacking intermediate judgments. Both tracks use the same determinate action labels and retain invalid or missing planned responses as end-to-end failures.

Labels distinguish determinate actions, conditional expectations and insufficient evidence. A case can have a clear applicability answer but a boundary action because the continuous historical score is not determined by a discrete anchor alone. Such cases remain in the versioned dataset and descriptive analyses; they are not forced into the headline denominator. We report determinate coverage alongside accuracy. Review identified one pre-call evaluability decision that does not follow this distinction cleanly: MR010_b was excluded despite a supported historical-protection branch. Section 5.6 preserves the original main panel and reports that alternative interpretation separately.

### 4.2 Development and independent construction

The earlier engineering V3 study contains 48 workflows and 384 bilingual records: 312 determinate, 48 boundary and 24 insufficient. It provides development-era diagnostics and a source for human review. It is not retroactively described as a fresh blind test after repeated protocol development.

For prospective validation, an independent AI author received the public engineering policy and data format, but not the frozen prompt, earlier failure cases or evaluated-model outputs. A separate AI reviewer first assigned anonymous labels, froze them, and then reconciled them with author labels. Access separation was procedural, not operating-system-enforced isolation. These roles are AI construction and AI review, not independent human data collection. The model identities of the AI construction and review systems are not disclosed in this public draft, limiting exact regeneration and model-specific bias analysis.

The initial batch comprised 24 workflows, two states and two languages, with 24 preselected paraphrases. Before any evaluated-model outputs, an explicit coverage audit found too few determinate workflows involving obsolete core failures. A recorded, fixed amendment added two independent workflows and eight records; original conversations and labels were preserved. The final primary panel has 104 records, of which 86 are determinate, 16 boundary and two insufficient. The 24 paraphrases form a separate panel. The original cohort and coverage-directed supplement are reported separately.

The engineering construction therefore has two sequential AI authoring sources: one for the original 24 workflows and paraphrases, and a second for the two-workflow supplement. The same public-policy/anonymous-first-label procedure covers both cohorts. The matched engineering comparisons reuse this dataset; they add no authoring sources. Twenty-six workflows are the resampling units, not 26 independent human sources or 26 independent model architectures. Construction-source diversity remains limited.

### 4.3 Human review and label reliability

Two people independently reviewed the same preselected 96 V3 records, using their own independent AI assistants for understanding and engineering knowledge, and personally reviewing their judgments. Neither saw original answers or model outputs in the review interface. The assistants differed from the case author; their exact models and prompts were not recorded. Responses were batch-entered, so submission intervals are not reading-time measurements.

The reviewers agreed on action/status in 92 of 96 records (Cohen's kappa 0.932). Both endorsed all 78 originally determinate actions. Differences concerned ambiguity and evidential sufficiency, and remain available without changing the frozen original labels. This is valid independent AI-assisted human review of the sampled V3 material. It is not a human review of the new independent cohort, and optional scenario-naturalness ratings were not collected.

### 4.4 Analysis and fair competitors

We average action correctness within each workflow, then across workflows. Paired bootstrap intervals resample whole workflows, retaining translations, states and repeats together. They describe the constructed sample; a small number of generation sources limits broader source-level inference. We also report applicable-failure protection, obsolete-failure release, healthy-history behavior, same-request contrasts and language-specific results.

The principal competitor is a direct decision supplied with the same visible facts, exact assessment definitions and complete routing objective. A current-demand-only rule and an ungated historical rule are mechanism ablations. Since the declared objective already requires historical relevance, removing relevance predictably creates opportunities for errors; this is not by itself evidence that a learned applicability judgment is reliable.

A fresh matched campaign crosses Jev 1.13, Jev 1.14 and Qwen 3.8 Flash with direct and decomposed formats: 128 unchanged inputs, two interleaved repeats and 1,536 new calls. Both Qwen formats use non-thinking mode, temperature zero, JSON output and a 768-token completion cap. Interface development uses separate records, with no test-guided prompt revision. This campaign is prospective for its conditions, but the dataset has already been evaluated and is not newly blind. Earlier Qwen configurations remain preliminary results rather than the principal backend comparison. The direct encoding includes the complete assessment definitions and composition policy; it is fully informed, but is not presumed to be the best possible direct prompt for each backend.

### 4.5 Mathematical custom-policy construction and matched execution

One independent AI author prepared 16 mathematical-research workflows, two states and two languages, using the public policy and data format rather than prior evaluated-model outputs. It authored both languages, sealed conversations before annotations, and recorded self-review; bilingual versions are not independent authoring sources. Only the two current-demand criteria change; q0's general instructions, the exact historical rubric and applicability question, continuous scoring and composition remain fixed. The criteria distinguish work with a supplied, settled mathematical path from an unresolved proof obligation, theorem application or counterexample search. The router does not independently establish mathematical truth. All 64 conversations retain their reviewed wording and have empty prior-context arrays.

A separate AI reviewer first labeled anonymous records, froze its answers, then reconciled them with author labels. Two independent human participants subsequently reviewed all 64 records each with their own AI assistance and without author/model answers in the interface. They reported no mathematical research experience. Action/status agreement is 60/64 (kappa 0.902), and both endorse all 50 original determinate actions. Naturalness-category agreement is 37/64; all ratings are natural or plausible, which is not representative-workflow validation. The earlier plan's domain-expert prerequisite was explicitly amended before model calls to proceed with this disclosed non-expert review; it was not retrospectively claimed to be satisfied. The study separates the AI author, anonymous AI labeling/adjudication, and later AI-assisted human judgments; none is counted as an additional mathematical expert. The provenance record retains role and source identifiers without inventing unrecorded author or human-assistant model versions.

Pre-call adjudication retains 50 determinate, 12 boundary and two insufficient actions (78.125% determinate coverage). Only MR005_a's bilingual single grade changes from 1 to an allowed {0,1} set: useful proof structure coexists with non-delivery of the only missing bound. Current reasoning still fixes primary. MR015_a remains an action boundary, MR010_b insufficient, and MR016_b retains component-level reference ambiguity despite both humans favoring not-established. Originals and all disagreements remain available. Unique component labels cover 62 current-demand, 60 grade and 62 applicability records; their denominators differ from the action panel.

Before any mathematical-model response, independent method review and a 33-file freeze bind inputs, labels, exact prompts, requests and analysis. We use the single explicit-condition/local-scope direct rewrite from Appendix D, replacing only its embedded demand criteria and domain phrase. Native and Qwen conditions receive the same state/question semantics. The six cells run in randomized interleaved order with two repeats: 768 calls. All receive HTTP 200, with no retries or unknown outcomes. One Qwen direct response copies question definitions and reaches the 768-token cap; it remains a format failure in the planned denominator. No prompt, threshold, case or label is changed after outputs.

## 5. Results and mechanism evidence

### 5.1 Engineering comparison and representation audit

The matched engineering campaign compares the same full objective in direct and typed formats. The original direct encoding contains all assessment definitions and the composition rule, but subsequent audit exposes a representation confound. We retain this table as a comparison of frozen configurations; it does not estimate a pure decomposition effect.

| Backend | Original direct action | Three typed judgments | Difference (percentage points) | Paired 95% interval |
|---|---:|---:|---:|---|
| Jev 1.13 | 67.31% | 97.12% | +29.81 | [+16.35, +44.23] |
| Jev 1.14 | 67.31% | 96.63% | +29.33 | [+15.87, +43.27] |
| Qwen 3.8 Flash | 91.35% | 91.83% | +0.48 | [−3.85, +5.77] |

All 1,536 planned responses are valid, with no retries or unknown outcomes. Main accuracy is workflow-macro on 86 determinate records across 26 workflows, each repeated twice; 16 boundary and two insufficient records remain unforced. The original 24-workflow cohort gives three/direct accuracies of 96.88%/68.75%, 96.35%/68.75% and 92.71%/92.19%. Qwen's small net gain combines 11 corrected actions with ten regressions; decomposition is not uniformly beneficial across backends.

Both typed Jev configurations protect all 52 related-core-failure responses and release all 28 obsolete-core-failure responses. The original direct configurations release all 28 but miss all 52 protections. In IG001, a CSV download still contains an excluded account; adding a column and regenerating the same download remains affected. The first English Jev 1.13 typed response gives $s=0.89,h=0,a=0.99$, correctly invoking protection; original direct selects economy. Qwen selects primary in both formats.

Independent review identifies two co-occurring expression issues: direct categories refer to a policy in the instructions rather than stating the action conditions, and embedded local assessment instructions may be read as governing the entire direct decision. Original direct development checks covered healthy continuations, not failure protection. On all 172 determinate responses per native version, original direct actions equal the corresponding typed current-demand-only actions. This is observable underuse of the historical override; separate-question outputs cannot be treated as the direct model's hidden reasoning.

A single policy-equivalent rewrite makes action conditions explicit and scopes the embedded component instructions locally. A frozen supplement on the already-seen inputs raises Jev 1.13 direct accuracy to 92.31%, but Jev 1.14 only to 68.27%; original controls reproduce all prior determinate actions. The rewrite changes two expression features together, and neither three-question nor Qwen cells are rerun in that supplement. Appendix D preserves the probes, full results and audit. These findings establish representation sensitivity, not an intrinsic inability to make compound decisions or an optimal direct prompt.

Post-hoc alignment with the earlier three-question run localizes the residual difference. Among 28 responses requiring current reasoning, every native method/version gets 24 correct. For 52 routine responses requiring related-core protection, explicit direct gets 42/52 and 2/52, versus 52/52 for three questions. For 92 routine responses requiring economy, explicit direct gets 92/92 for both, versus 90/92 and 89/92. The tradeoff is improved failure protection with some overprotection; this cross-run alignment is descriptive. Section 5.4 uses the same frozen explicit-direct rewrite in a contemporaneous mathematical comparison.

### 5.2 Independent validation and structural ablations

| Frozen method | Independent primary panel | Original cohort only |
|---|---:|---:|
| Jev 1.13, complete three-judgment method | 97.12% | 96.88% |
| Jev 1.14, complete three-judgment method | 96.15% | 95.83% |
| Jev 1.13, ungated two-judgment ablation | 73.08% | 75.00% |
| Jev 1.14, ungated two-judgment ablation | 73.08% | 75.00% |

These are workflow-macro accuracies on determinate actions, not task-completion rates. Determinate primary coverage is 82.69%. The native three-minus-two differences are 24.04 percentage points for Jev 1.13 (paired 95% interval [11.54,38.46]) and 23.08 for Jev 1.14 ([11.54,36.54]). The original cohort alone retains positive differences of 21.88 and 20.83 points. Native requests differ, so these contrasts include possible changes in the individual answers as well as the composition rule.

To isolate numerical gating on observed answers, we construct an ungated counterfactual from each complete three-judgment response. Restoring the applicability gate corrects 38 and 36 actions, respectively, in this earlier independent validation, without an observed added regression in its determinate primary panel. The fresh matched campaign has analogous counts of 38 and 37; these are separate observations. Twenty-eight corrections in each version concern obsolete core failures; the remainder concern irrelevant local problems or uncertain severity under withdrawn requirements. This diagnostic does not prove that every history-aware router needs our decomposition.

Both versions retain protection in all 52 responses from 13 workflows with relevant core failures, and release protection in all 28 responses from seven workflows with obsolete core failures. The counts include bilingual variants and repeats. Zero observed misses is not a population guarantee. In 48 preselected original/paraphrase response pairs neither version changes its action; all 36 determinate pairs are correct on both expressions. This is local expression robustness, not arbitrary custom-policy invariance.

### 5.3 Two mechanisms must remain distinct

The independent panel primarily demonstrates the restriction of genuine historical problems to their current scope. In IG017, an earlier signed archive used the wrong team. The user later reports re-signing it correctly and passing the portal's profile check. Low historical scores remain appropriate for the failed original stage, while low applicability permits the pending upload-preset work to use economy.

Earlier V3 diagnostics show a second phenomenon: the applicability judgment sometimes blocks propagation of an erroneously low score on a stage that was actually reliable. Holding full responses fixed corrects 74/82 actions, including 22/30 of this second type. Those cases do not establish that the historical scorer was correct. The new independent panel contains no such corrections, so the two studies' mechanism contributions must not be pooled into one claimed replication.

### 5.4 Prospective mathematical custom-policy results

| Backend | Explicit direct | Three questions | Difference (points) | Paired workflow 95% interval | Corrected / regressed calls |
|---|---:|---:|---:|---|---:|
| Jev 1.13 | 96.88% | 96.88% | 0.00 | [−4.69, +4.69] | 2 / 2 |
| Jev 1.14 | 95.31% | 97.66% | +2.34 | [−2.34, +9.38] | 4 / 1 |
| Qwen 3.8 Flash | 92.19% | 98.44% | +6.25 | [−1.56, +15.62] | 10 / 2 |

These are equal-workflow means on the original 50 clear records across 16 workflows, with 100 planned clear calls per cell. All difference intervals include zero. Qwen has the highest three-question point estimate under this frozen evaluability choice; the alternative MR010_b interpretation in Section 5.6 changes that ordering. The study establishes neither a stable backend ranking nor universal decomposition advantage. The pronounced direct 1.14 weakness in engineering does not recur in aggregate here, cautioning against attributing a dataset-specific pattern to an intrinsic version limitation. Different domain datasets are not a non-inferiority design.

The four routine records requiring related-core-failure protection cover only two workflows (eight calls per cell). Direct protection succeeds 6/8, 4/8 and 4/8 times for Jev 1.13, 1.14 and Qwen; three questions succeed 8/8 in each. In MR001_b, a fully supplied telescoping calculation was requested as an actual display, but the previous stage wrote only its heading. The next formatting task remains routine and depends on that missing display. English Jev 1.14 direct chooses economy twice, while the decomposed responses give s=1, h=0.06 and a=0.94, correctly invoking protection. This supports the declared history-aware objective on that case, not a prediction that escalation will improve mathematical execution.

The comparison has regressions as well as gains. In healthy stages, Jev 1.13 overprotects both Chinese MR009_b repeats and 1.14 one English MR004_a response. Qwen fails to release an externally completed correction in both Chinese MR006_b repeats. Conversely, Qwen releases the unrelated notation-key work in MR008_a, correcting three semantic errors and one invalid direct output. Section 5.5 traces these changes through the component outputs.

Across 24 boundary calls, native three-question methods select primary 23/22 times, versus 1/0 for direct; Qwen three selects primary once. These unforced actions reveal different boundary behavior and receive no accuracy credit. Independent raw-response recomputation reproduces all main and component results, intervals and invalid-call handling. API-reported Jev usage charges total USD 0.051580368, while Qwen consumes 565,243 Token Plan tokens. The fixed historical mechanism is usable under this custom criterion; the study does not establish non-inferior customization across domains, whose task distributions differ.

### 5.5 Component quality, action quality and error propagation

The mathematical results expose why the three evaluation layers cannot substitute for each other. All percentages below are workflow-macro. Each component uses only records with a unique label for that component; the final action uses the determinate panel. The grade column measures the modal rubric class, while routing still uses the supplied continuous score.

| Backend | Current demand, 62 records | Historical modal grade, 60 | Applicability, 62 | Final action, 50 |
|---|---:|---:|---:|---:|
| Jev 1.13 | 98.44% | 85.16% | 92.97% | 96.88% |
| Jev 1.14 | 98.44% | 89.06% | 92.19% | 97.66% |
| Qwen 3.8 Flash | 100.00% | 84.38% | 77.34% | 98.44% |

First, the populations differ. A post-hoc common-cohort check retains 48 records with both determinate actions and unique applicability, covering all 16 workflows and 96 calls per backend. Applicability/action macro accuracy is 93.75%/96.88% for Jev 1.13, 93.75%/97.66% for 1.14 and 91.41%/98.44% for Qwen. The large Qwen applicability deficit shrinks, but its lower applicability accuracy still coexists with the highest action point estimate on exactly the same records. This sensitivity check supplements the full component panel rather than replacing it.

Second, component errors have unequal effects under Section 3.4's rule. The following post-hoc partition accounts for every uniquely labeled applicability error. Counts are responses, including repeats; the categories are mutually exclusive, with current-demand masking checked before score masking.

| Applicability errors | Jev 1.13 | Jev 1.14 | Qwen |
|---|---:|---:|---:|
| Boundary or insufficient action label | 1 | 2 | 18 |
| Determinate action already selected by current reasoning | 4 | 4 | 9 |
| Determinate action insensitive because $h\geq1$ | 2 | 3 | 0 |
| Incorrect applicability changes the determinate action | 2 | 1 | 2 |
| **All applicability errors** | **9** | **10** | **29** |

Qwen's 18 non-determinate cases comprise 14 boundary and four insufficient responses. They are not counted as correct actions. Its nine current-masked errors occur where the demand judgment correctly selects primary. Only two applicability errors change an action in the original determinate panel. The four insufficient responses are all MR010_b; their exclusion is disputed, and Section 5.6 shows its effect rather than treating those errors as inherently inconsequential. Qwen also makes no current-demand errors in the uniquely labeled demand panel and assigns no score below one on the 44 healthy-routine responses, whereas each Jev version does so twice. These error locations and the evaluability convention jointly explain the original action ranking; they do not establish better historical interpretation or a ranking stable under other case mixtures.

Four traces distinguish mechanisms that an aggregate score conceals:

- **A boundary is not a wrong action (MR002_b_en).** Four of five requested ledger rows were delivered; the remaining row must be included in the pending table. Gold grade is one, applicability is positive, and the action is a continuous-score boundary. Qwen returns $s=0.95,h=1$ twice, with $a=0.05$ then $0.90$. One applicability label is wrong, yet neither response activates the strict $h<1$ condition. Both select economy; neither receives determinate-action credit.
- **A correct action can mask a wrong applicability judgment (MR014_b_zh).** A requested plan was delivered; the pending lemma note now requires resolving unproved replacement conditions. Gold is current reasoning, grade two, no applicable past failure, hence primary. Qwen gives $s=0.10,h=1.75,a=0.90$: applicability is wrong, but current demand correctly selects primary. Jev instead assigns $s=0.67$–$0.74$, incorrectly selecting economy. Jev 1.13 additionally gives $h=0.85/0.80$; deleting its correct negative applicability gate would accidentally recover primary through that erroneous low score. That is error cancellation, not justified historical protection.
- **A wrong applicability judgment can cause overprotection (MR006_b_zh).** A copy editor has completed both symbol replacements and supplied the corrected sentence. The pending task only adds a heading and quote format. Qwen correctly retains the original failure score, $h=0.20$, but $s=0.95,a=0.90$ wrongly escalates both repeats. Jev keeps the historical failure score and correctly releases its applicability. Here the third judgment is consequential and fails for Qwen.
- **Two errors can reinforce each other (MR009_b_zh).** The assigned finite comparison was delivered, with generalization explicitly excluded. The pending task just formats it. Jev 1.13 reports $s=1,h=0.97/0.91,a=0.67/0.73$: an erroneously low score and false applicability jointly produce primary. In MR004_a_en, Jev 1.14's similarly low scores produce one wrong action at $a=0.52$ and one correct action at $a=0.47$. The gate can block score error, but cannot guarantee that the two judgments avoid a shared misreading of stage responsibility.

Finally, same-response mechanical ablations show what the stored outputs contribute to policy execution. No extra API calls or prompt changes are involved:

| Backend | Current demand only | History without applicability | Full HiRoute rule | Gate restored: corrected / regressed calls |
|---|---:|---:|---:|---:|
| Jev 1.13 | 92.19% | 85.94% | 96.88% | 12 / 2 |
| Jev 1.14 | 92.19% | 84.38% | 97.66% | 13 / 0 |
| Qwen 3.8 Flash | 93.75% | 92.19% | 98.44% | 7 / 0 |

The denominator is the original 50 determinate records and two repeats, macro-averaged over 16 workflows. Applicability restoration reduces overprotection, including Qwen's unrelated MR008_a notation-key task: $h=0.20,a=0.05$ releases protection, while the paired request for the still-missing original table remains protected. Jev 1.13's two action regressions remove the accidental MR014_b cancellation described above. Thus semantic correction and action improvement are not identical. These ablations measure arithmetic contributions on observed judgments under a policy that explicitly values relevance; they are not separately queried competitors or causal evidence of improved downstream execution.

The System One contribution is therefore an inspectable decision interface coupled to an explicit application rule. The component track diagnoses whether the right facts were recognized; the action track measures whether the declared policy was followed; the propagation audit explains which mistakes mattered. A higher score in any one layer cannot stand in for the other two.

### 5.6 A disputed evaluability choice: MR010_b

MR010_b asks for a lemma inventory from a separately supplied marked page, explicitly requiring the inventory to be displayed in the conversation. The assistant replies only that it is prepared. The pending request asks to apply the remaining margin note to that same inventory and return the updated version. The separate page, inventory contents and note are unavailable to the evaluator.

Before model calls, annotation confirmed grade zero for non-delivery of the explicitly requested display and positive applicability because the same deliverable remains required. It nevertheless marked current demand unknown and the final action insufficient, adopting a conservative requirement to know the pending work's contents before awarding an action label. The pre-call adjudication already recorded that a protection-based primary answer was defensible. Both language records were retained and sent to every condition; their action exclusion was not selected after seeing results.

Independent review identifies a genuine asymmetry in that convention. Under the declared policy, an established historical-protection branch can be sufficient for primary even when current demand is unknown: discovering additional reasoning would provide another primary reason, not cancel the existing one. MR016_b already applies the converse logic by retaining primary from current reasoning while applicability is unknown. The record does not establish an additional product-history eligibility failure that would justify treating MR010_b differently. We therefore treat the original exclusion as a disputed evaluation choice, not a necessary consequence of the routing rule. We do not add a runtime requirement that every current-task detail must be known before historical protection can operate.

The table below is a reviewer-proposed, post-hoc sensitivity: retain every original label and main result, but in a separate calculation count only MR010_b_en and MR010_b_zh as primary under the established core-non-delivery/applicability interpretation. It retains actual model returns, continuous scalars, thresholds, all other labels and the equal-workflow aggregation. There are 52 action-scored records, 104 calls per cell and the same 16 workflows. No discrete grade is substituted for a returned score, and no new model calls are made.

| Backend | Original direct | Original three | Sensitivity direct | Sensitivity three |
|---|---:|---:|---:|---:|
| Jev 1.13 | 96.88% | 96.88% | 93.75% | 96.88% |
| Jev 1.14 | 95.31% | 97.66% | 92.19% | 97.66% |
| Qwen 3.8 Flash | 92.19% | 98.44% | 89.06% | 95.31% |

Both Jev three-question configurations select primary on all four added responses; all direct configurations and Qwen three select economy. Qwen's four added responses give $s=0.95$, $h=1.75/1.80$ and $a=0.05$: they both overestimate historical delivery and reject applicability. These become four additional action errors under the candidate interpretation, but changing applicability alone would not repair them because the high historical score still blocks protection. The applicability-error partition would therefore be 14 boundary, nine current-demand masked, four score-masked and two action-changing errors, still totaling 29. Masking means lack of marginal action influence, not that the resulting action is correct.

The sensitivity explains why the original highest point estimate is not robust to the evaluability convention. It does not justify selecting the alternative as the official score because it favors Jev. The main table and its intervals remain tied to the original frozen panel; the sensitivity does not inherit those intervals or establish a new significant ranking. It reveals a labeling-consistency issue that future prospective evaluability rules should resolve symmetrically for either sufficient policy branch.

### 5.7 Remaining failure boundaries

In IG008_b, two release owners have not resolved whether to remove an old command or retain an alias for automation. The assessed stage correctly records that disagreement. The pending instruction asks to finish a compatibility paragraph. Both Jev versions treat this as routine writing even though it requires resolving an unapproved tradeoff. Historical protection should not invent a past failure to repair a current-demand error. Only four independent-panel workflows have reasoning-demand labels; their workflow recall is 75% in both versions, a weakness obscured by high aggregate demand accuracy.

In IG002, a deployment plan omits a bounded rollback section. The pending task only creates a migration file from an agreed template, keeps the old read path and does not execute deployment. Historical scores below one sometimes combine with an incorrect applicability judgment to escalate this step. The modal grade is one, but changing to modal-grade routing would violate the frozen rule. This case illustrates overextending a project-level concern to the responsibility of the immediate step.

These two workflows account for six/eight remaining primary-panel response errors in the original validation, and six/seven in the fresh matched campaign. IG008 also fails in both Qwen formats, exposing a shared boundary rather than a Jev-specific defect. Different questions can misread the same evidence, so semantic separation does not guarantee statistically independent errors. In particular, applicability cannot rescue a genuine failure scored above the protection threshold, nor can it override a current-demand judgment that already selects primary.

## 6. HiRoute implementation and reproducibility

HiRoute exposes configurable task criteria, rubric descriptions and thresholds while owning context assembly, assessment attribution, response validation, group selection and observation. The open-source implementation is a contribution because it makes these contracts executable and available for integration. It is not additional evidence of semantic accuracy merely because it contains the research prompt.

The local research artifact preserves exact prompt hashes, input and label versions, anonymous first labels, adjudication, raw provider responses, request identities and costs. Independent offline arithmetic recomputation verified all 1,791 valid actions in the prospective campaign; its one invalid response remained a failure. Both the old V3 freeze and the 205-file independent freeze remained unchanged. The fresh matched campaign independently reproduces all 1,536 actions from raw answers and verifies its 42-file freeze. API-reported Jev usage charges total USD 0.096355392, while Qwen consumes 1,006,558 Token Plan tokens. Appendix C normalizes the observed decision charges per request; these are service-specific resource observations, not architecture speed or downstream savings estimates. An aborted first campaign and dependency-restoration record are disclosed in Appendix B, with no selective splicing of successful cells.

At [public revision 62186edf](https://github.com/higress-group/HiRoute/tree/62186edf22550c24807dd4606a36c31727840598), 27 actual classification-module checks and one real-listener test passed. The listener test covers both smart-saving and categorical routing across four successive decisions, verifies actual provider calls and records protection applied, suppressed, and applied again. These checks use controlled services and the product default simple threshold of 0.8; the paper uses 0.5. They establish covered production behavior, not replay of all saved paper responses under the paper configuration. That stronger correspondence check remains open. The mathematical study tests custom policy interpretation without asking the router to independently prove mathematical claims. Qwen in these experiments is a research numerical-output adapter; its results do not certify a shipped Qwen decision-provider integration.

The product source and the [versioned research package](README.md#research-material-availability) accompany this manuscript. The package contains original bilingual inputs and labels, a separate MR010_b sensitivity overlay, curated model responses including invalid and abandoned outcomes, exact request plans, frozen parsing and scoring adapters, offline analysis, and anonymized construction and review evidence. Its [data card](DATA-CARD.md) documents dependence and disclosure limits. [Offline reproduction](REPRODUCE.md) verifies public hashes and recomputes the principal machine-result tables without private files, credentials or network calls. Private human submissions and undisclosed construction/review model identities remain outside the package, so arithmetic reproducibility does not imply full regeneration of the annotation process.


## 7. Limitations and conclusion

The evidence concerns policy-conforming decisions at synthetic checkpoints in continuing tasks. It does not measure actual executor success, optimal cost, natural traffic prevalence, long unattended trajectories or robustness near the eight-turn truncation boundary. Labels encode an explicit historical-protection objective, so superiority over an ablation that omits that objective does not establish optimal routing utility. Bilingual records and repeats do not increase the number of independent workflows. AI-assisted human judgments support the reviewed labels; mathematical reviewers were not domain experts, and the two mathematical protection workflows provide narrow coverage. Two Jev versions are not two independently designed System One architectures.

The open-source HiRoute mechanism separates what the current task requires, what happened during execution and whether that execution problem still matters. Typed System One outputs make those judgments observable, while application code makes their composition deterministic. Independent engineering data and a custom mathematical policy provide evidence of useful historical protection and release under this declared objective. They also show that input representation, current-demand errors and jointly mistaken historical judgments limit the result.

The strongest supported conclusion is scoped policy conformance with auditable failure modes. Neither a component leaderboard nor a final-action leaderboard is sufficient: masked errors can remain latent, and accidental cancellation can improve an action score for the wrong reason. The original engineering direct gap is expression-sensitive; all original mathematical direct-versus-three difference intervals include zero, and the MR010_b sensitivity changes the backend ordering. Boundary-action differences also leave the operational consequences of continuous-score behavior unresolved. The evidence supports the implementation and evaluation method without establishing universal decomposition or System One superiority. Broader independent policy samples, saved-response production replay at the paper configuration and live execution studies are needed for stronger transfer, correspondence and utility claims.

## Appendix A. Legacy migration evidence

Legacy comparisons remain available for regression and migration analysis, not as the principal efficacy baseline. In the independent primary panel legacy Jev 1.13/1.14 score 78.85%/75.00%; complete methods score 97.12%/96.15%. The complete method improves 30/36 individual response actions but regresses on two per version, both in IG002. Multiple protocol elements changed, so these differences do not isolate a single mechanism. All old results and negative cases remain in the artifact.

## Appendix B. Execution deviations and retained accounting

The first Qwen development request was rejected because JSON-object mode required the literal word JSON in the prompt. Adding that output-format marker yielded 20/20 valid development checks. The first main campaign nevertheless used request bodies cached before the marker change. It was stopped and preserved, including four unknown outcomes. No semantic outputs informed a prompt change. A replacement campaign regenerated all bodies, added exact body-to-adapter consistency checks, froze the amendment and reran every cell. Successful cells from the aborted campaign were not spliced into the reported results. Development and aborted-run resource records are separate from the completed main-run costs.

The first analysis invocation then failed before prediction processing because two imported modules had not been copied. Restoring byte-identical modules from the prior, already-frozen analysis source resolved the import error. No frozen scoring code, requests, labels or prompts changed. Independent raw-response arithmetic agrees with the completed report, and independent whole-workflow bootstrap recomputation reproduces the three main intervals. These checks validate numerical reduction; they do not independently validate every schema interpretation or certify the semantic labels. These procedural failures are disclosed for reproducibility; they are not counted as semantic errors of a model whose valid response was never obtained.

## Appendix C. Service costs, latency and numerical consistency

### Observed decision costs

The following table aggregates the existing API-reported USD usage charges for every planned Jev request in the two completed matched campaigns. All calls in these cells have a usage charge. It includes boundary and insufficient cases as well as the determinate panel, and excludes separate development and aborted runs. No new API calls or price assumptions are introduced. The normalized column divides each cell's total charge by its observed request count and multiplies by 1,000; it is not a separately executed thousand-request experiment.

| Campaign | Model | Format | Requests | Total usage charge (USD) | USD per 1,000 requests |
|---|---|---|---:|---:|---:|
| Engineering | Jev 1.13 | Original direct | 256 | 0.026680080 | 0.104219 |
| Engineering | Jev 1.13 | Three questions | 256 | 0.021497616 | 0.083975 |
| Engineering | Jev 1.14 | Original direct | 256 | 0.026680080 | 0.104219 |
| Engineering | Jev 1.14 | Three questions | 256 | 0.021497616 | 0.083975 |
| Mathematics | Jev 1.13 | Explicit direct | 128 | 0.014730996 | 0.115086 |
| Mathematics | Jev 1.13 | Three questions | 128 | 0.011059188 | 0.086400 |
| Mathematics | Jev 1.14 | Explicit direct | 128 | 0.014730996 | 0.115086 |
| Mathematics | Jev 1.14 | Three questions | 128 | 0.011059188 | 0.086400 |

Three-question routing costs approximately **USD 0.084–0.086 per 1,000 checkpoints** in these observed workloads. Each checkpoint evaluates all three judgments in one API request. This small measured charge supports using the method as a low-cost decision step at successive agent turns. The three-question cells also incur lower charges than the corresponding direct encodings in both campaigns; that comparison is specific to these request encodings and service charges, rather than a general pricing guarantee for decomposition.

These amounts cover decision calls, not executor work, dataset construction or review. Qwen uses a subscription Token Plan and is reported in tokens; we do not derive a comparable per-request USD cost from that allowance or claim a measured dollar saving relative to Qwen. The evaluated backend identities and versions remain disclosed. The hosted Jev access service is not named in this public draft, which limits exact reproduction of billing and transport conditions.

### Client-observed latency and scalar consistency

| Backend | Format | Median client latency | p95 client latency |
|---|---|---:|---:|
| Jev 1.13 | Direct | 948 ms | 1,030 ms |
| Jev 1.13 | Decomposed | 911 ms | 1,040 ms |
| Jev 1.14 | Direct | 935 ms | 1,040 ms |
| Jev 1.14 | Decomposed | 899 ms | 1,077 ms |
| Qwen 3.8 Flash | Direct | 740 ms | 1,654 ms |
| Qwen 3.8 Flash | Decomposed | 1,328 ms | 1,769 ms |

This post-run descriptive table includes all 128 inputs and two repeats per cell. p95 uses the nearest-rank definition. Jev was accessed through a hosted decision API over a proxy; Qwen used its Token Plan endpoint, so absolute differences include transport, scheduling and service effects. The sample does not demonstrate that System One is universally faster: Qwen direct has the lowest median in this campaign. Three native questions do not require serial round trips.

Native Jev historical scalars differ from the expectations reconstructed from displayed probabilities by at most 0.01. Qwen's generated scalar differs by more than 0.05 in seven of 256 decomposed responses, with a maximum difference of 0.7. A post-run sensitivity check replacing only the scalar with that expectation would change three Qwen actions, all from correct primary protection to incorrect economy in the determinate main panel. The reported main results retain supplied scalars; no arithmetic repair, modal-grade substitution or threshold calibration is applied. This illustrates why generated numerical reports and native decision outputs require separate consistency checks.

## Appendix D. Audited native-direct expression sensitivity

Independent post-run review identified a concrete threat in the native direct adapter: referential action categories and verbatim local component instructions may not convey the intended scope reliably. We froze exactly one policy-equivalent alternative before new developer-probe responses. It puts the primary union and economy complement in the category definitions and explicitly limits embedded q0/q1/q2 restrictions to their respective assessments. Full assessment definitions, continuous scoring and thresholds are unchanged. A separate reviewer checked semantic equivalence before calls.

Eight new bilingual developer probes cover primary-by-current-demand, primary-by-applicable-failure, release after completed correction, and a local-score boundary. In 32 calls, each native version changes from four to six correct determinate responses out of six; two boundary responses per encoding receive no accuracy label. Improvements concern the still-undelivered required build-tag line. Jev 1.14's new protection probabilities are only 0.53 and 0.52. These small diagnostic probes demonstrate expression sensitivity, not general accuracy or robust margins.

This result triggered a separately frozen supplement on all 128 previously seen engineering inputs: original versus the unchanged explicit encoding, both native versions, two interleaved repeats. The original main experiment is retained in full. This supplement estimates representation sensitivity on the seen sample. Since no decomposed or Qwen condition is rerun in this supplement, comparison with those earlier cells is descriptive and may include time-of-execution effects. The two representational changes are not separated, and no further prompt search is performed.

The supplement completes all 1,024 calls with valid responses, no retries and no unknown outcomes. Workflow-macro determinate-action accuracy changes from 67.31% to 92.31% for Jev 1.13 (+25.00 points; descriptive paired workflow interval [+13.46,+36.54]) and from 67.31% to 68.27% for Jev 1.14 (+0.96; [0,+2.40]). Both original controls reproduce every earlier determinate-main action, although probabilities vary. The explicit variant improves 42 and two determinate calls, respectively, without an observed regression. Relevant-core-failure protection becomes 42/52 and 2/52; obsolete-failure release remains 28/28 and healthy-history correctness 52/52 for both. Boundary cases are unforced. On the original 24 workflows alone, explicit accuracy is 92.71%/69.27%; the two-workflow supplement is 87.50%/56.25%.

For IG001_a English, Jev 1.13's primary probability changes from 0.38/0.38 to 0.83/0.80 across the two repeats, correctly protecting the still-faulty download. Jev 1.14 changes from 0.05/0.04 to 0.39/0.55, protecting only once. Both explicit variants still fail the Chinese dependent-index continuation IG026_a and the unresolved compatibility decision IG008_b. Both correctly release IG002_b, where earlier decomposed outputs sometimes overprotect. These mixed outcomes remain part of the result.

Passing the small developer probes did not establish semantic adequacy on the broader contextual panel. The supplement strengthens the specific expression-sensitivity diagnosis, especially for 1.13, without identifying an internal cause or proving that other direct formulations must fail. No further prompt selection follows. API-reported Jev usage charges for this supplement total USD 0.111021120, separate from USD 0.003288432 for the developer diagnostic. Independent raw-choice arithmetic reproduces the main averages; full semantic reannotation was not performed.

## References

1. TypeSafe. *Introduction*. https://docs.typesafe.ai/introduction (accessed 18 October 2026).
2. TypeSafe. *Score*. https://docs.typesafe.ai/primitives/score (accessed 18 October 2026).
3. Ong et al. *RouteLLM: Learning to Route LLMs with Preference Data*. https://arxiv.org/abs/2406.18665.
4. Ribeiro et al. *Beyond Accuracy: Behavioral Testing of NLP Models with CheckList*. ACL 2020. https://aclanthology.org/2020.acl-main.442/.
5. Parashar et al. *Whose turn is it? Context-guided model routing for stateful agents*. https://arxiv.org/abs/2610.19292.
6. Yang et al. *TwinRouterBench: Fast Static and Live Dynamic Evaluation for Realistic Agentic LLM Routing*. https://arxiv.org/abs/2605.18859.
7. Tran et al. *Arch-Router: Aligning LLM Routing with Human Preferences*. https://arxiv.org/abs/2506.16655.
8. Gardner et al. *Evaluating Models’ Local Decision Boundaries via Contrast Sets*. https://aclanthology.org/2020.findings-emnlp.117/.
