---
name: hiroute-collaboration
description: Delegate suitable executable work through HiRoute by default.
---

# HiRoute collaboration

Delegate suitable executable work by default. Before delegating, run `hiroute worker plans` and select a published plan whose stated purpose matches the work; if none matches, say so instead of inventing one.

Start executable work with `hiroute worker exec --plan <ID> --cwd <PATH> ...`. Permission policy defaults to `approve-all`; use a restricted policy only when the user asks for one and the selected harness supports it. Supply exactly one input source and keep the returned submission key, task ID, and run ID. Use the response's next actions to read progress, check status, wait, fetch results, continue, or recover an uncertain submission. Never replace an uncertain submission key automatically.

Do not delegate coordination, plan discovery, status checks, waits, result summaries, or another delegation. A Worker must not recursively delegate. Never request or expose grants, capabilities, root identities, digests, revisions, credentials, or native session material.
