# PLAN: configure and publish routing plans

Use available models, call name, native reasoning configuration and selected policy.
Read `issue-spec/specs/routing-plans/spec.md` plus ordered/smart-saving/free-first
specs as applicable. Minimum smoke saves/publishes a test plan, reopens to verify
effective revision/name, then makes a real request matching the plan's source/model.
Reuse verified sources. Extend policy changes with simple/complex and free/paid
branches, publication changes with draft isolation and active publication, shared
semantics with both clients. No batch business script yet.

1. Codex edits ordered/smart-saving/free-first policy. Jev only explores read-only
   pages, never fills, switches policy or saves. Extract stable writes after validation.
2. Save/cancel/reopen/publish/read back source, order, reasoning and call name;
   draft saves cannot alter the active version.
3. For smart saving, verify real simple/complex branches/models and no needless
   tool-continuation reclassification. Check selected external decision protocol,
   timeout and failure behavior.
4. Free-first checks free-only and explicit paid fallback. Never rename a paid
   model to conceal missing real free-source evidence.
5. Use the same plan from Codex/Claude, distinguishing ordinary new requests from
   already accepted worker tasks under their publication/version contract.

Missing source/service is an unmet prerequisite; fixtures do not prove a real
external decision service. Do not publish by editing databases. Restore only owned
configuration, preserving later user edits. Record each policy's actual coverage,
versions, calls and costs in the Issue/PR.
