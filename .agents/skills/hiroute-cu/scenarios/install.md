# INSTALL: package and application lifecycle

Verify a frozen package revision/hash, architecture, current version and data
configuration; read `docs/macos-installation.md`. New-user empty state and upgrade
of an existing HOME are distinct variants. Never delete real data to manufacture
empty state. Reuse a verified installation instead of reinstalling for every case.

Minimum smoke identifies candidate/live processes, opens normally, reads existing
data and service state, then quits/reopens successfully. Extend installation,
upgrade/lifecycle changes with empty state, data preservation and window/process
exit semantics. No batch business script exists; Codex owns system actions.

1. Identify full packaged revision and actual processes, and prepare recovery for
   owned installation changes; filenames/timestamps do not prove latest source.
2. Install/upgrade through normal Finder/system entry. Jev may explore explicitly
   read-only entries, never install or approve system changes.
3. Open the app and verify actual app/daemon candidate, readable sources/plans/
   sessions and absence of an old process taking over the new UI.
4. Close/reopen the window, then explicitly quit/relaunch. Verify background
   behavior and persistence separately; closing a window is not task cancellation.

Preserve launch logs/screenshots/revision, not repeated reinstall/clear-data attempts.
Restore only owned changes. Missing package/CUA or isolated new-user environment
is explicit missing coverage. Run-specific results belong in the Issue/PR; this
scenario itself is a test design awaiting current real execution.
