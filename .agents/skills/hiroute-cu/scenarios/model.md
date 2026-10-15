# MODEL: source, credential and model connection

Use authorized source/protocol, credential references and test models; modify only
owned objects. Contract: `issue-spec/specs/model-connections/spec.md`. Minimum smoke
saves through UI, reopens/readbacks and completes a real call from the tested client.
Existing sources can be reused, but a save test must actually save. Required tools/
streaming are not optional. Extend credential/save changes with failure repair and
cancel; protocol/metadata changes with affected clients, protocols, tools/streaming.
No batch business script exists yet; stable form segments may be extracted later.

1. Observe the actual form. Jev explores proven read-only entries; entering a page
   that creates a draft is a Codex write. Codex owns form/secret entry and saves.
2. Select source, account, protocol and model; save/reopen consistently. Cancel
   leaves no empty object.
3. On a test source, fail with an invalid value, correct/save and verify that one
   error cannot permanently disable save or other pages.
4. Complete real text and required tool/stream calls, checking actual model/source,
   upstream path and authentication. Test protocols separately; missing metadata
   alone does not mean unsupported. Keep raw local traces private.

Separate upstream failures from UI save failures. Direct comparisons diagnose but
do not replace the UI rerun. Do not log credentials. Restore only test changes,
keeping original user sources and host credential rules. Locate actual controls
before writing steps. Record current executed scope/result in the Issue/PR.
