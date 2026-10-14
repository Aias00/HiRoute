# NAV-01: open the Agent overview

Start with running HiRoute on Home and visible main navigation. This is navigation
only, with no configuration or model calls. Minimum smoke selects the unique
Agent navigation from fresh AX and verifies the Agent heading and My Agents / Task
records entries. An entry's presence does not prove its page opened. A known single
step can use direct CU; reuse the [NAV-02 script](../scripts/nav-codex.mjs) for a
multi-step segment. Jev is useful only when the read-only entry needs rediscovery.

Extend for changed navigation/overview with missing/loading/error states and return.
On timeout, missing entry or unchanged Home, preserve evidence and return to Codex;
do not reinstall to conceal the failure. Restore Home from fresh AX when needed.
The path is repeatable navigation knowledge. Rerun the current package and record
its revision/result in the Issue/PR; it proves no save, model or worker behavior.
