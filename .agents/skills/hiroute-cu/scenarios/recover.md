# RECOVER: normal use after failure

Choose one reproducible save/call scenario with owned test sources/tasks and known
recoverable configuration. Faults affect only owned objects, not system networking
or unrelated processes. Minimum smoke runs the related normal operation, causes a
controlled failure, repairs through the same UI and confirms unrelated pages work.
Reuse the business scenario's prerequisites/evidence. Select fault variants by
save/call/service changes; one recovery does not cover every variant. Codex owns
fault injection, uncertain writes and recovery; no batch business script.

1. Run the normal baseline and record saved value, service state and test identity.
2. Select credential correction, repeated save, page/reopen, test endpoint failure/
   disconnection or controlled service restart. Codex explores unknown UI first.
3. Check pending/error: normal pending does not flash failure, error preserves input
   and offers a valid next step, retry does not duplicate objects/execution.
4. Remove the fault, save/call through the same UI, verify effect and open an
   unrelated page to rule out global deadlock.

Read back unknown submission state before retrying. Inspect drift rather than
bypass checks. Unknown recovery stops related effects with a blocker, while
independent paths continue. Restore owned fault/configuration and preserve failure
evidence. Record each variant separately in the Issue/PR; a successful variant
cannot cover the others not executed.
