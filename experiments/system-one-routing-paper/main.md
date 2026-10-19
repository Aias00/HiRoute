# HiRoute: System One Routing for Long-Running Agents with History-Aware Protection

Community-review manuscript · research snapshot: 18 October 2026. This Markdown version is intended for collaborative review before typesetting and arXiv submission. It retains the frozen labels and principal result tables, reports the MR010_b sensitivity separately, and distinguishes AI construction from AI-assisted human review. Exact study questions, bilingual datasets, curated responses and an offline reproduction package accompany [the reading guide](README.md).

## Abstract

An agent's next task can be routine even when an unresolved execution failure calls for a stronger model. Once that failure is corrected or no longer affects the work, the reason for escalation disappears. We study this temporal routing problem with a System One decision interface and its open-source implementation in HiRoute. One request asks three typed questions: what the current task requires, how the assessed stage performed, and whether a historical problem still applies. HiRoute combines the returned probabilities and continuous score through a fixed rule. A bilingual benchmark evaluates both final group selection and the judgments behind it, using explicit task criteria rather than universal difficulty labels. On 26 engineering workflows, the matched three-question configurations reach 97.12% and 96.63% workflow-averaged action accuracy for Jev 1.13 and 1.14. A prospective mathematical custom-policy study retains the historical protocol and exposes both gains and regressions against fully informed direct selectors. Its main difference intervals include zero; a disputed label-exclusion choice also changes the backend ordering. Case-level analysis explains how judgment errors are blocked, propagated or accidentally canceled by the composition rule. Measured Jev three-question charges are approximately USD 0.084–0.086 per 1,000 requests. Published data, offline replay and a pinned implementation support inspectable execution of a declared routing policy. They leave downstream task success, realized savings and broader generalization for further study. AI construction, AI review and AI-assisted human review are documented separately.

## 1. Introduction

Consider an agent asked to edit a supplied configuration value. The path is known and the change is bounded, so an economy model group would ordinarily be appropriate. But suppose the previous stage was already asked to make this edit and failed to deliver it. A policy may now call for the stronger primary group, even though the remaining edit is still simple. We call this override **historical protection**. If the user later completes the edit and asks for an independent label change, the old failure no longer justifies that override.

HiRoute assigns conversation interpretation to a decision model and policy execution to application code. It separates three judgments:

1. **Current demand:** does the pending work follow a settled path, or does it require unresolved reasoning under the configured task standard?
2. **Historical performance:** did the stage being assessed deliver or reliably advance the work assigned to it?
3. **Present applicability:** does an unresolved problem from that stage still affect the pending work?

In the example, current demand remains routine throughout. Historical performance remains poor for the failed stage. Only applicability changes after the edit is completed. The third judgment therefore lets the router release an obsolete reason for escalation while preserving protection for work that remains affected. It concerns visible evidence and dependency; it does not predict a particular executor's chance of success.

Our focus is the use of System One interfaces for these decisions. We use the term operationally: a service accepts shared state and typed questions and returns structured categories, probabilities or scores. Jev provides such an interface. Its documentation recommends asking bounded questions and composing outputs in software; we treat those statements as an interface design rationale, not proof that the resulting judgments are accurate or their errors independent [1]. We examine observable API behavior and do not infer a proprietary architecture from the outputs.

Task difficulty is policy-dependent. A label such as “simple” is meaningless without a declared scope of work and intended executor roles. The engineering policy studied here distinguishes bounded changes with an established path from unresolved investigation, consequential design choices and intricate interactions. Users may customize this policy. Our evaluation asks whether the router applies that declared standard consistently as a task progresses; it does not define universal difficulty across executor models.

The system contribution is the connection between this decision interface and an executable temporal routing policy. The three questions share one state and one API request; they are not three sequential model calls. Their numerical outputs expose which judgments drive HiRoute's group selection. We compare this format with a direct selector that receives the same facts and full policy but returns the final group itself. An LLM numerical adapter also answers the three questions, allowing us to separate the choice of backend from the choice of output format.

Our contributions are:

1. An open-source System One routing mechanism that combines current task demand with applicable historical failure.
2. A policy-conditioned bilingual benchmark and offline evaluator, usable both by generic group selectors and by typed decision interfaces. Ambiguous and score-boundary cases remain visible without forcing a single action label.
3. Experiments and case traces showing when historical protection helps, when it should be released, and how component errors affect the final action.

The construction and audit method is intended to transfer to other policies. Its simple/complex labels remain tied to the declared task standard and executor roles.

For developers of multi-model agents and gateways, the practical questions are whether the mechanism protects affected work, whether it releases unrelated work, and whether the extra judgments improve selection over a fully informed direct answer. Sections 5.1–5.4 report these comparisons. Section 5.5 traces the errors behind the aggregate scores, and Section 5.6 examines a disputed choice about which cases can be scored.

## 2. Related work and scope

Preference-based routing methods such as RouteLLM learn to trade execution quality against resource use [3]. Our immediate target is different: faithful execution of an explicit contextual routing policy. The two targets are complementary. A policy can be implemented correctly yet have poor downstream utility; an executor-quality study is needed to make the latter claim.

Context-guided routing for stateful agents studies history access at user-turn boundaries and the effects of changing executor models [5]. TwinRouterBench separates intermediate-prefix tier evaluation from live task resolution and spend [6]. These studies establish temporal routing and execution-based validation as existing directions. Our contribution concerns the scope and temporal interpretation of historical protection under an explicit policy, with separate measurements of judgments and actions. We do not claim to introduce stateful routing or replace live-execution benchmarks. Arch-Router's use of user-defined preferences further motivates making the routing contract explicit [7].

Our contextual contrasts follow the behavioral-testing principle of testing capabilities through controlled changes rather than relying on an aggregate score [4,8]. We extend that principle to routing checkpoints: the pending request may remain fixed while prior delivery, correction, requirement validity or dependency changes. Translations, state variants and paraphrases are correlated observations, so the workflow is the principal statistical unit.

Typed decision interfaces make the intermediate judgments directly observable. A Jev Score returns a distribution over ordered rubric levels and a continuous scalar [2]. An LLM can also be instructed to emit numerical judgments, but those reported numbers and a native decision distribution arise through different mechanisms. Neither is presumed calibrated. A matched backend-by-format experiment is required before attributing differences to decomposition or backend; an obsolete internal protocol is not an adequate principal competitor.

## 3. Method

### 3.1 State and temporal boundaries

A **routing checkpoint** is the moment before the next request is assigned to a model group. A **stage** is the preceding execution segment whose performance is being assessed. At each checkpoint, the input separates three regions:

| Region | Content | Evidential role |
|---|---|---|
| `prior_context` | Up to eight complete preceding turns | Earlier requirements, settled facts and references |
| `assessment_stage` | The full stage under assessment, subject to explicit product size/partial handling | Requested stage outcome and visible execution |
| `latest_user` | The pending request, kept separate | Current work and any reported feedback or completed correction |

The method organizes original records rather than generating a semantic summary. Raw tool-result bodies are not supplied for scoring. There is no dedicated goal extraction or source-type classifier. An instruction to continue is not inherently negative feedback. A message can report a correction already completed and request a different action that has not been performed; these evidential roles must not be conflated.

The eight-turn background limit is an implementation choice, not an empirically established optimum in the main studies. The independent validation below contains zero or one preceding turn, so it cannot establish long-history robustness or behavior when relevant requirements fall outside that window.

### 3.2 Three independently specified judgments

**Current demand (q0): what must be done now?** Under the published engineering standard, routine work follows explicit requirements and existing patterns. Complex work requires unresolved diagnosis, a consequential choice among alternatives, or intricate architectural, concurrency, state or algorithmic reasoning. Project size and words such as “fix” do not determine the answer. History can explain references such as “that file”; a past failure by itself does not make the remaining work complex.

**Historical performance (q1): what did the assessed stage achieve?** The ordinal rubric has three anchors: zero for a supported core non-delivery or persistent core failure; one for useful progress with a bounded unresolved local issue; and two for delivery or reliable advancement without a supported unresolved execution problem. The reference is the work assigned at the time. Recording an unsuccessful research direction can fulfill an investigation request, whereas promising an artifact does not fulfill a request to deliver it. The router uses the visible record and does not independently verify hidden implementation correctness.

**Present applicability (q2): does that problem still matter here?** A historical execution problem must remain unresolved and have a concrete relationship to the pending work: an affected deliverable, immediate prerequisite, or supported common operation or constraint. Shared project membership alone is insufficient. A completed correction or withdrawn requirement can remove applicability; an instruction or promise to repair does not establish that the repair happened.

Thus q1 evaluates the stage's responsibility at execution time, while q2 evaluates the problem's status and relevance now. All three prompts are frozen. Their development identifier, C, is retained in provenance but is not the method's public name. The exact question files are linked from the [reading guide](README.md#exact-study-questions).

### 3.3 Continuous score and deterministic composition

The rule has two reasons to select primary: the current work requires reasoning, or an applicable historical problem warrants protection. Let $s$ be the probability that current work is simple, $h\in[0,2]$ the returned historical performance score, and $a$ the applicability probability. With an eligible, complete historical assessment and valid answers, the paper configuration is:

$$
\mathrm{primary} \iff s < 0.5\;\lor\;(h<1\;\land\;a\geq0.5).
$$

Otherwise the rule selects the economy group. The simple threshold is configurable: this paper fixes it at 0.5, while the tested product default is 0.8. HiRoute stores the normalized historical score $h/2$; the corresponding strict threshold is 0.5. This is algebraically identical, not a second scoring rule.

The historical score is continuous, so a most-likely grade of one does not imply a score of exactly one. For example, probabilities $(0.06,0.93,0.01)$ for grades zero, one and two have modal grade one, but their weighted score is $0\times0.06+1\times0.93+2\times0.01=0.95$. If the supplied scalar is 0.95, the strict $h<1$ condition activates protection when applicability is high. A supplied score of exactly 1 does not. We preserve HiRoute's continuous-score rule, so grade-one cases near this boundary need not have a unique economy target. Routing uses the supplied scalar; the weighted example explains the distinction, and any scalar/distribution discrepancy is recorded rather than silently corrected.

An absent or invalid applicability answer preserves otherwise eligible historical protection. Invalid, partial or incorrectly attributed historical assessments follow the existing product contracts. These operational paths are evaluated separately from valid-answer semantic accuracy.

### 3.4 When a component can change the action

The three judgments do not vote. If current demand already selects primary, historical answers cannot change that action. If current work is routine and the historical score is at least one, protection is inactive. Applicability becomes decisive only for routine work with a historical score below one. More formally, holding the other outputs fixed, changing applicability can change the action **if and only if** $s\geq0.5$ and $h<1$.

| Observed outputs | Final decision | What an applicability error can do |
|---|---|---|
| $s<0.5$ | Primary | No change; current demand already determines the action |
| $s\geq0.5$, $h\geq1$ | Economy | No change; historical protection is inactive |
| $s\geq0.5$, $h<1$ | Primary iff $a\geq0.5$ | Can add or remove historical protection |

This conditional influence explains why a component's accuracy and final-action accuracy can differ. We call an error **masked** when another condition prevents it from changing the action; it remains a semantic error. Conversely, two mistaken judgments can produce the correct group for the wrong reason, which we call **error cancellation**. A correct applicability judgment can also block an erroneously low historical score, while an incorrect one can let that score cause overprotection. We therefore evaluate individual judgments, final actions and error propagation separately. Section 5.5 illustrates each outcome with a complete case trace.

## 4. Policy-conditioned evaluation

The benchmark measures whether a router follows the declared group-selection policy at a checkpoint. **Action accuracy** means choosing the reference model group; it does not mean that an executor completed the underlying task. We evaluate three complementary questions:

- The independent engineering study tests protection and release on newly constructed workflows under a frozen protocol (Section 5.2).
- The matched engineering study compares backends and answer formats on those same inputs, then audits a direct-prompt expression problem (Section 5.1).
- The prospective mathematics study changes the current-demand criteria while retaining the historical mechanism, using a direct prompt specified before the new results (Sections 5.4–5.6).

A **workflow** is one underlying scenario. A **record** is a particular state and language version of that scenario. A **response** is one model call on a record. Translations and repeated calls therefore add observations, not independent workflows. The older V3 dataset supports development diagnostics and sampled human review; it is kept separate from independent validation.

Case IDs use IG for independent engineering workflows and MR for mathematics. The suffixes `a`/`b` distinguish paired states, and `en`/`zh` distinguish languages. A case trace names the relevant record; the reproduction tool exposes its full context and all stored responses (Section 6).

### 4.1 Two participation tracks

The action track accepts any system that selects economy or primary from the visible record and full declared policy. The typed track additionally evaluates current demand, historical rubric grade, applicability, probability outputs and error propagation. A direct selector is not penalized for lacking intermediate judgments. Both tracks use the same determinate action labels and retain invalid or missing planned responses as end-to-end failures.

We distinguish three label statuses. **Determinate** records have enough evidence for a unique reference action. **Boundary** records support only a conditional expectation, often because a grade-one judgment leaves the continuous score near the routing threshold. **Insufficient** records lack evidence needed to decide. The latter two remain in the dataset and descriptive results but receive no forced action label or headline accuracy credit. We report the fraction with determinate actions alongside accuracy. Section 5.6 examines one disputed exclusion, MR010_b, without replacing the original labels or main results.

### 4.2 Development and independent construction

The earlier engineering V3 study contains 48 workflows and 384 bilingual records: 312 determinate, 48 boundary and 24 insufficient. It provides development-era diagnostics and a source for human review. It is not retroactively described as a fresh blind test after repeated protocol development.

For prospective validation, an independent AI author received the public engineering policy and data format, but not the frozen prompt, earlier failure cases or evaluated-model outputs. A separate AI reviewer first assigned anonymous labels, froze them, and then reconciled them with author labels. Access separation was procedural, not operating-system-enforced isolation. These roles are AI construction and AI review, not independent human data collection. The model identities of the AI construction and review systems are not disclosed in this public draft, limiting exact regeneration and model-specific bias analysis.

The initial batch comprised 24 workflows, each with two states and two languages: 96 records, plus 24 preselected paraphrases. Before any evaluated-model outputs, a coverage audit found too few determinate workflows involving obsolete core failures. A recorded amendment added two independently authored workflows and eight records; original conversations and labels were preserved. The final primary panel has 104 records: 86 determinate, 16 boundary and two insufficient. The 24 paraphrases form a separate panel. Results distinguish the original cohort from this coverage-directed supplement.

The engineering construction therefore has two sequential AI authoring sources: one for the original 24 workflows and paraphrases, and a second for the two-workflow supplement. The same public-policy/anonymous-first-label procedure covers both cohorts. The matched engineering comparisons reuse this dataset; they add no authoring sources. Twenty-six workflows are the resampling units, not 26 independent human sources or 26 independent model architectures. Construction-source diversity remains limited.

### 4.3 Human review and label reliability

Two people independently reviewed the same preselected 96 V3 records, using their own independent AI assistants for understanding and engineering knowledge, and personally reviewing their judgments. Neither saw original answers or model outputs in the review interface. The assistants differed from the case author; their exact models and prompts were not recorded. Responses were batch-entered, so submission intervals are not reading-time measurements.

The reviewers agreed on action/status in 92 of 96 records (Cohen's kappa 0.932). Both endorsed all 78 originally determinate actions. Differences concerned ambiguity and evidential sufficiency, and remain available without changing the frozen original labels. This is valid independent AI-assisted human review of the sampled V3 material. It is not a human review of the new independent cohort, and optional scenario-naturalness ratings were not collected.

### 4.4 Analysis and fair competitors

Our main accuracy measure first averages action correctness within each workflow, then gives every workflow equal weight. We call this **workflow-macro accuracy**. Paired bootstrap intervals resample whole workflows, keeping translations, states and repeats together. They describe uncertainty within the constructed sample; the small number of generation sources limits broader inference. We also report protection for relevant failures, release after a failure becomes obsolete, behavior after healthy stages, paired-context contrasts and language-specific results.

We use two kinds of comparison. A **direct selector** receives the same visible facts, exact assessment definitions and full routing objective, then selects the group itself. It tests whether decomposition helps beyond giving the model the complete policy. **Mechanism ablations** remove parts of HiRoute's rule: current-demand-only ignores history; ungated history uses a low historical score without checking relevance. These ablations show what the removed conditions contribute. Because relevance is part of the target policy, outperforming an ungated rule alone cannot establish that the model's applicability judgment is reliable.

A fresh matched campaign crosses Jev 1.13, Jev 1.14 and Qwen 3.8 Flash with direct and decomposed formats: 128 unchanged inputs, two interleaved repeats and 1,536 new calls. Both Qwen formats use non-thinking mode, temperature zero, JSON output and a 768-token completion cap. Interface development uses separate records, with no test-guided prompt revision. This campaign is prospective for its conditions, but the dataset has already been evaluated and is not newly blind. Earlier Qwen configurations remain preliminary results rather than the principal backend comparison. The direct encoding includes the complete assessment definitions and composition policy; it is fully informed, but is not presumed to be the best possible direct prompt for each backend.

### 4.5 Mathematical custom-policy construction and matched execution

The mathematical study asks whether the same historical mechanism can operate under a different declared task standard. Its criteria distinguish a supplied, settled mathematical path from an unresolved proof obligation, theorem application or counterexample search. Only the two current-demand criteria change; q0's general instructions, the exact historical rubric and applicability question, continuous scoring and composition remain fixed. The router interprets those criteria from the record; it does not independently establish mathematical truth.

One independent AI author prepared 16 workflows, each with two states and two languages, using the public policy and format rather than prior evaluated-model outputs. It authored both languages, sealed conversations before annotations, and recorded self-review. All 64 conversations retain their reviewed wording and have empty prior-context arrays. Bilingual versions do not count as independent authoring sources.

A separate AI reviewer first labeled anonymous records, froze its answers, then reconciled them with author labels. Two independent human participants subsequently reviewed all 64 records each with their own AI assistance and without author/model answers in the interface. They reported no mathematical research experience. Action/status agreement is 60/64 (kappa 0.902), and both endorse all 50 original determinate actions. Naturalness-category agreement is 37/64; all ratings are natural or plausible, which is not representative-workflow validation. The earlier plan's domain-expert prerequisite was explicitly amended before model calls to proceed with this disclosed non-expert review; it was not retrospectively claimed to be satisfied. The study separates the AI author, anonymous AI labeling/adjudication, and later AI-assisted human judgments; none is counted as an additional mathematical expert. The provenance record retains role and source identifiers without inventing unrecorded author or human-assistant model versions.

Pre-call adjudication retains 50 determinate, 12 boundary and two insufficient actions (78.125% determinate coverage). Only MR005_a's bilingual single grade changes from 1 to an allowed {0,1} set: useful proof structure coexists with non-delivery of the only missing bound. Current reasoning still fixes primary. MR015_a remains an action boundary, MR010_b insufficient, and MR016_b retains component-level reference ambiguity despite both humans favoring not-established. Originals and all disagreements remain available. Unique component labels cover 62 current-demand, 60 grade and 62 applicability records; their denominators differ from the action panel.

Before any mathematical-model response, independent method review and a 33-file freeze bind inputs, labels, exact prompts, requests and analysis. We use the single explicit-condition/local-scope direct rewrite from Appendix D, replacing only its embedded demand criteria and domain phrase. Native and Qwen conditions receive the same state/question semantics. The six cells run in randomized interleaved order with two repeats: 768 calls. All receive HTTP 200, with no retries or unknown outcomes. One Qwen direct response copies question definitions and reaches the 768-token cap; it remains a format failure in the planned denominator. No prompt, threshold, case or label is changed after outputs.

## 5. Results and mechanism evidence

### 5.1 Engineering comparison and representation audit

The matched engineering campaign compares the same full objective in direct and typed formats. The large Jev gaps below need an important qualification: although the original direct prompt contains the full policy, later audit finds that its wording can suppress the historical override. We retain the frozen comparison, then examine this expression problem before interpreting the gain.

| Backend | Original direct action | Three typed judgments | Difference (percentage points) | Paired 95% interval |
|---|---:|---:|---:|---|
| Jev 1.13 | 67.31% | 97.12% | +29.81 | [+16.35, +44.23] |
| Jev 1.14 | 67.31% | 96.63% | +29.33 | [+15.87, +43.27] |
| Qwen 3.8 Flash | 91.35% | 91.83% | +0.48 | [−3.85, +5.77] |

All 1,536 planned responses are valid, with no retries or unknown outcomes. Main accuracy is workflow-macro on 86 determinate records across 26 workflows, each repeated twice; 16 boundary and two insufficient records remain unforced. The original 24-workflow cohort gives three/direct accuracies of 96.88%/68.75%, 96.35%/68.75% and 92.71%/92.19%. Qwen's small net gain combines 11 corrected actions with ten regressions; decomposition is not uniformly beneficial across backends.

Both typed Jev configurations protect all 52 related-core-failure responses and release all 28 obsolete-core-failure responses. The original direct configurations release all 28 but miss all 52 protections. In IG001, a CSV download still contains an excluded account; adding a column and regenerating the same download remains affected. The first English Jev 1.13 typed response gives $s=0.89,h=0,a=0.99$, correctly invoking protection; original direct selects economy. Qwen selects primary in both formats.

The expression problem has two parts. The direct categories refer back to a policy instead of stating the selection conditions, and instructions intended only for current-demand assessment may appear to govern the whole decision. A model can therefore treat “history does not change task complexity” as a reason to ignore history when selecting the group. Original direct development checks covered healthy continuations, not failure protection. On all 172 determinate responses per native version, original direct actions equal the corresponding typed current-demand-only actions. This demonstrates underuse of the historical override, although the separate typed answers do not reveal the direct model's internal reasoning.

A single policy-equivalent rewrite makes action conditions explicit and scopes the embedded component instructions locally. A frozen supplement on the already-seen inputs raises Jev 1.13 direct accuracy to 92.31%, but Jev 1.14 only to 68.27%; original controls reproduce all prior determinate actions. The rewrite changes two expression features together, and neither three-question nor Qwen cells are rerun in that supplement. Appendix D preserves the probes, full results and audit. These findings establish representation sensitivity, not an intrinsic inability to make compound decisions or an optimal direct prompt.

Comparing those saved explicit-direct outputs with the earlier three-question run shows where they differ. Among 28 responses requiring current reasoning, every native method/version gets 24 correct. For 52 routine responses needing protection from a related core failure, explicit direct gets 42/52 and 2/52, versus 52/52 for three questions. For 92 routine responses that should use economy, explicit direct gets 92/92 for both, versus 90/92 and 89/92 for three questions. Thus the remaining tradeoff is stronger failure protection with some overprotection. These are descriptive comparisons across runs. The mathematics study in Section 5.4 compares the same frozen explicit-direct rewrite with three questions in one contemporaneous campaign.

### 5.2 Independent validation and structural ablations

Does applicability add useful protection control beyond simply escalating after a low historical score? The earlier independent campaign compares the complete method with a separately queried two-question ablation that omits applicability.

| Frozen method | Independent primary panel | Original cohort only |
|---|---:|---:|
| Jev 1.13, complete three-judgment method | 97.12% | 96.88% |
| Jev 1.14, complete three-judgment method | 96.15% | 95.83% |
| Jev 1.13, ungated two-judgment ablation | 73.08% | 75.00% |
| Jev 1.14, ungated two-judgment ablation | 73.08% | 75.00% |

These are workflow-macro accuracies on determinate actions, not task-completion rates. Determinate primary coverage is 82.69%. The native three-minus-two differences are 24.04 percentage points for Jev 1.13 (paired 95% interval [11.54,38.46]) and 23.08 for Jev 1.14 ([11.54,36.54]). The original cohort alone retains positive differences of 21.88 and 20.83 points. Native requests differ, so these contrasts include possible changes in the individual answers as well as the composition rule.

Separate requests can change the model's answers as well as the rule being tested. We therefore also hold each complete three-question response fixed and recompute its action with and without the applicability condition. Restoring that condition corrects 38 and 36 actions in the earlier independent campaign, with no added regression on its determinate primary panel. The fresh matched campaign gives 38 and 37 corrections; these are separate observations. Twenty-eight corrections per version concern obsolete core failures. The rest concern irrelevant local problems or uncertain severity under withdrawn requirements. This isolates the gate's arithmetic contribution on observed answers; it does not show that every history-aware router needs this decomposition.

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

Qwen's applicability accuracy is 77.34%, yet its final-action accuracy is 98.44%. There are two reasons this can happen: the percentages score different sets of records, and the rule allows an action to be correct despite a component error. We examine each reason before interpreting the ranking. All percentages below are workflow-macro. The grade column evaluates the most likely rubric grade; routing still uses the supplied continuous score.

| Backend | Current demand, 62 records | Historical modal grade, 60 | Applicability, 62 | Final action, 50 |
|---|---:|---:|---:|---:|
| Jev 1.13 | 98.44% | 85.16% | 92.97% | 96.88% |
| Jev 1.14 | 98.44% | 89.06% | 92.19% | 97.66% |
| Qwen 3.8 Flash | 100.00% | 84.38% | 77.34% | 98.44% |

First, an applicability answer can be uniquely labeled even when the action is a score boundary or lacks enough evidence. To compare like with like, a post-hoc check retains 48 records with both a determinate action and unique applicability: all 16 workflows and 96 calls per backend. Applicability/action accuracy becomes 93.75%/96.88% for Jev 1.13, 93.75%/97.66% for 1.14 and 91.41%/98.44% for Qwen. The Qwen gap shrinks, but lower applicability accuracy still coexists with the highest action point estimate on exactly the same records. This check supplements the original panels; it does not replace them.

Second, applicability is decisive only when current work is classified routine and the historical score is below one (Section 3.4). The table traces every uniquely labeled applicability error: outside the determinate action panel, blocked by current demand, blocked by the historical score, or consequential for the action. Counts are responses including repeats. Each error enters only one row, checking current demand before the score.

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

The practical issue is whether these two records belong in the action denominator. The table reports a separate, reviewer-proposed sensitivity that scores MR010_b_en and MR010_b_zh as primary because non-delivery and applicability are established. Every original label and main result remains unchanged. This calculation uses the same saved responses, continuous scalars, thresholds, other labels and workflow aggregation: 52 scored records, 104 calls per cell and the same 16 workflows. It makes no new calls and does not substitute a discrete grade for the returned score.

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

HiRoute provides the executable path from the decision interface to model selection. Users configure task criteria, rubric descriptions and thresholds; the application assembles context, attributes the assessed stage, validates the response, selects the group and records the decision. Publishing this path lets other developers inspect and integrate the mechanism alongside its research questions and results.

The public research package has three starting points: the [datasets](datasets/README.md) contain bilingual inputs and original labels; the [data card](DATA-CARD.md) explains construction, dependence and disclosure limits; and the [reproduction guide](REPRODUCE.md) gives the offline commands. Replay parses the saved responses, reapplies the continuous-score rule and recomputes the principal machine-result tables without private files, credentials or network calls. It also supports scoring another router and inspecting a complete case with its stored model outputs.

The package retains exact request plans, invalid and abandoned outcomes, frozen parsing and scoring adapters, anonymous construction/review evidence, and a separate MR010_b sensitivity overlay. All original labels remain available. Independent arithmetic replay verifies all 1,791 valid actions in the earlier prospective engineering campaign and all 1,536 in the fresh matched campaign; the former campaign's invalid response remains a failure. Freeze checks and execution deviations are described in Appendix B. Appendix C reports service-specific decision charges and latency; the matched engineering run records USD 0.096355392 of Jev charges and 1,006,558 Qwen Token Plan tokens. These observations concern routing overhead, not realized executor savings.

Product-path evidence is distinct from offline arithmetic. At [public revision 62186edf](https://github.com/higress-group/HiRoute/tree/62186edf22550c24807dd4606a36c31727840598), 27 classification-module checks and one real-listener test passed. The listener test covers both smart-saving and categorical routing across four successive decisions, verifies provider calls, and observes protection applied, suppressed and applied again. It uses controlled services and the product's default simple threshold of 0.8, whereas the paper uses 0.5. Replaying every saved paper response through the production path at the paper configuration remains open. Qwen's research numerical adapter likewise does not certify a shipped Qwen decision-provider integration.

Private human submissions and undisclosed construction/review model identities remain outside the [versioned package](README.md#research-material-availability). The public files reproduce machine-result arithmetic and expose the available labeling evidence; they do not fully regenerate the annotation process.

## 7. Limitations and conclusion

The study measures policy-conforming decisions at synthetic checkpoints in continuing tasks. It leaves actual executor success, optimal cost, natural traffic prevalence, long unattended trajectories and robustness at the eight-turn truncation boundary unmeasured. Labels encode a historical-protection objective, so improvement over an ablation that omits that objective does not establish optimal routing utility. Bilingual records and repeats do not increase the number of independent workflows. AI-assisted human judgments support the reviewed labels, but mathematical reviewers were not domain experts and only two mathematical workflows test related-core protection. Two Jev versions also represent one model family rather than independent System One architectures.

The open-source HiRoute mechanism separates what the current task requires, what happened during execution and whether that execution problem still matters. Typed System One outputs make those judgments observable, while application code makes their composition deterministic. Independent engineering data and a custom mathematical policy provide evidence of useful historical protection and release under this declared objective. They also show that input representation, current-demand errors and jointly mistaken historical judgments limit the result.

The strongest supported result is that the open mechanism can apply a declared policy with inspectable protection, release and failure modes. A final-action score alone hides why a router succeeds: another condition may mask a wrong judgment, or two errors may cancel. The experiments also set limits on the comparison. Engineering direct results depend on prompt expression; all original mathematical direct-versus-three difference intervals include zero; and the MR010_b sensitivity changes the backend ordering. Continuous-score boundary cases still need operational outcome evaluation. Broader independent policy samples, production replay at the paper configuration and live execution studies would test transfer, implementation correspondence and downstream utility, respectively.

## Appendix A. Legacy migration evidence

Legacy comparisons remain available for regression and migration analysis, not as the principal efficacy baseline. In the independent primary panel legacy Jev 1.13/1.14 score 78.85%/75.00%; complete methods score 97.12%/96.15%. The complete method improves 30/36 individual response actions but regresses on two per version, both in IG002. Multiple protocol elements changed, so these differences do not isolate a single mechanism. All old results and negative cases remain in the artifact.

## Appendix B. Execution deviations and retained accounting

The first Qwen development request was rejected because JSON-object mode required the literal word JSON in the prompt. Adding that output-format marker yielded 20/20 valid development checks. The first main campaign nevertheless used request bodies cached before the marker change. It was stopped and preserved, including four unknown outcomes. No semantic outputs informed a prompt change. A replacement campaign regenerated all bodies, added exact body-to-adapter consistency checks, froze the amendment and reran every cell. Successful cells from the aborted campaign were not spliced into the reported results. Development and aborted-run resource records are separate from the completed main-run costs.

The first analysis invocation then failed before prediction processing because two imported modules had not been copied. Restoring byte-identical modules from the prior, already-frozen analysis source resolved the import error. No frozen scoring code, requests, labels or prompts changed. Independent raw-response arithmetic agrees with the completed report, and independent whole-workflow bootstrap recomputation reproduces the three main intervals. These checks validate numerical reduction; they do not independently validate every schema interpretation or certify the semantic labels. These procedural failures are disclosed for reproducibility; they are not counted as semantic errors of a model whose valid response was never obtained.

Source-integrity checks leave both the older V3 freeze and the 205-file independent freeze unchanged. The fresh matched engineering campaign also verifies its 42-file freeze. Exact prompt hashes, input and label versions, anonymous first labels, adjudication, raw semantic responses, request identities and costs remain traceable through the published package and its disclosure record.

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
