# Scenario index

Select and order scenarios here; steps live in their own files. Maturity describes
reusable knowledge, never a passing result for the current candidate. Load only
selected scenarios and missing prerequisites, then follow their script links.
Each scenario separates minimum smoke from extensions selected by the change.

| ID / scenario | Read when | Prerequisite state (reuse when verified) | Maturity |
| --- | --- | --- | --- |
| [NAV-01 · Agent overview](scenarios/nav-agent.md) | Navigation preflight or small CU/Jev trial | Running HiRoute | Repeatable navigation only |
| [NAV-02 · Codex details](scenarios/nav-codex.md) | Multi-step navigation or Codex entry | Running HiRoute | Batch read-only navigation |
| [INSTALL · Lifecycle](scenarios/install.md) | New package, upgrade, first launch or restart | Identifiable candidate package | Needs exploration |
| [MODEL · Sources](scenarios/model.md) | New access, credential repair or protocols | App and authorized source | Needs exploration |
| [PLAN · Routing](scenarios/plan.md) | Create, publish or edit routing policy | Required MODEL sources | Needs exploration |
| [DECISION · Branches](scenarios/decision-routing.md) | Decision services, saving/custom branches or scores | MODEL, exact candidate, authorized decision provider | Needs exploration |
| [CODEX · Connection](scenarios/codex.md) | Desktop/CLI configuration and enable cycles | PLAN and native client | Needs exploration |
| [CLAUDE · Connection](scenarios/claude.md) | Normal claude entry, presets and authentication | PLAN and Claude Code | Needs exploration |
| [PI · Native integration](scenarios/pi.md) | Static sources, extra models, SDK workers or recovery | MODEL, PLAN, official Pi/Node, dedicated context | Explored; batch extraction pending |
| [DSH · Native ACP](scenarios/dsh.md) | Static sources, both protocols, native worker or recovery | MODEL, PLAN, official DSH and supported Node | Needs exploration |
| [MULTITURN · Real execution](scenarios/multiturn.md) | Context, tools, switching or recovery | CODEX/CLAUDE and real source | Needs exploration |
| [DELEGATE · Worker tasks](scenarios/delegate.md) | Skill/executor configuration or task lifecycle | PLAN and Worker; main agent may keep original connection | Needs exploration |
| [OBSERVE · History/usage](scenarios/observe.md) | Sessions, tasks, models or cost | Actual MULTITURN or DELEGATE facts | Needs exploration |
| [RECOVER · Failures](scenarios/recover.md) | Save, call or service recovery | Reproducible prerequisite of selected scenario | Needs exploration |

For one problem, select its scenario and missing state. A core acceptance chain
usually follows INSTALL → MODEL → PLAN → client connection → MULTITURN/DELEGATE
→ OBSERVE, with RECOVER where needed. Navigation proves none of those business paths.
Add a distinct scenario for a new authorized user goal. Keep variants with the
same goal and recovery boundary together; the index is not a closed feature list.
