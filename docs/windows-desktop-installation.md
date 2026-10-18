# Windows Desktop installation and updates

The [English installation guide](../apps/website/content/guides/install-windows.en.md)
and [Chinese installation guide](../apps/website/content/guides/install-windows.zh.md)
own the user steps, SHA-256 verification and SmartScreen prompts. The NSIS package
supports native Windows x64 and installs for the current ordinary user without
Python, WSL or administrator privileges.

The shell's program directory is fixed at `%LOCALAPPDATA%\Programs\HiRoute`.
Standalone service state remains under `%LOCALAPPDATA%\HiRoute\cli`; neither root
can contain the other. The native adapter checks KnownFolders, architecture,
existing manager ownership and reparse-free paths before product writes. An
SCM-managed installation is refused unchanged because SCM installation is not
offered in this release.

The installer verifies the complete embedded standalone package and uses its
native Rust transaction, the single Windows installation producer. The core
requires the package target to match the native host; the NSIS package and public
Desktop target alone are x64-only. Only the current
`hiroute.windows-install-journal/v1` schema is read. An unreleased Task installation
is adopted when its layout and journal match; incompatible material requires
repair/reinstallation. A prepared journal blocks business startup until recovery reconciles
files, the marker and actual Task registration. No new business mutations are
replayed during installation recovery.

The separate shell inventory records only its installed files and digests. Its
private preparation journal retains matching previous bytes until installation,
login settings and actual Control/Gateway readiness succeed. Changed/foreign
entries are preserved and require repair. Committed cleanup does not replay
installation or roll back a successfully committed package.
After NSIS overlays a replacement, obsolete owned frontend asset paths are removed
only when both their current bytes and retained backups match the old inventory.
This happens before the new payload's strict closed-file-set verification. A retry
recovers any pending shell transaction first. If the standalone core still matches
the previous installation, shell rollback restores its captured settings and files
without reinstalling or draining that unchanged core.

Recovery and uninstall run a verified shell copy outside the program directory.
A failed first installation can therefore roll back before `shell-package.json`
exists, without trying to delete a running shell or CLI. When no Agent storage or
storage-upgrade backup exists, rollback does not need to start the daemon.

After both transactions commit, the Rust core prunes obsolete versioned payloads
under `%USERPROFILE%\.local\lib\hiroute`. It retains the previous package until
shell rollback is no longer possible. Uninstall uses the bundled CLI outside the
version root and removes the verified current payload too. Cleanup resumes from
the current manifest after interruption and preserves changed, foreign or linked
entries. The shell records cleanup results in
`%LOCALAPPDATA%\HiRoute\shell\package-cleanup.json` for diagnosis.
After uninstall commits, package cleanup errors are reported without undoing the
uninstall or skipping an explicitly requested removal of standalone data.

The installer accepts `/S` for unattended installation with the default login-start
and tray choices and no prompts. Failures return a nonzero process exit code.

Login-start and tray choices default on and can be deselected independently.
Login start updates the user's Task and the fixed HKCU Run command together,
compensating a failed Run update. Disabling the tray makes window close exit the
shell; a background login launch then opens a visible window. Quit never stops the
service. A second launch focuses the existing window.

## Local console unavailable

The service binds its console at startup. Disabling the console with
`hiroute console disable` or `hiroute console init --mode off` therefore prevents
the shell from opening it after the next service restart. An occupied explicit
console port has the same effect, while Control and Gateway remain independent.
The bundled page identifies this local-console failure and shows the log path.
Recheck does not change the saved configuration or create a temporary listener.

To restore the local interface with automatic port selection, run in PowerShell:

```powershell
hiroute console init --mode loopback
hiroute service restart
```

Restart can interrupt active calls and tasks. After it completes, select Recheck
in the shell. Keeping the console disabled remains an explicit owner choice.

## Update ordering and recovery

The host selects a newer stable x64 NSIS entry from `releases-v3.json`. The page
cannot supply a download URL, install directory or executable. Downloading and
size/SHA-256 verification finish in private staging while the service runs. Native
lifecycle preparation pauses new admissions and reports active calls/tasks. The
user can cancel; cancellation resumes admission and leaves work untouched.

A missing or retired Windows catalog (HTTP 404/410) is shown as an unavailable
update check. The current installation and service remain usable; retry later.
The shell does not call that a failed installation, claim the current version is
latest, or use the macOS V2 feed as a Windows fallback.

After drain, a private native finisher acknowledges readiness and requires a
generation-bound explicit commit. It waits for the shell to exit, rechecks the
staged installer and runs NSIS passively with its update
flag. The installer repeats drain before stopping the owned daemon. A manual
installer uses that same hook and a bounded wait. An older daemon without drain
support requires a separate explicit interactive confirmation; passive updates
refuse it.
Passive updates skip the login/tray options page and reuse the saved choices;
silent `/S` installations continue to use the documented defaults.

Pre-stop failures resume admission. Replacement failure attempts the old package
and Task through the same Rust journal core; it never rewrites a marker to pretend
recovery succeeded. The next shell start reads the private outcome and can recover
an interrupted transaction after the installer is no longer active. An unresolved
file or service conflict stays on the bundled failure page with a diagnostics path.

## Uninstall boundaries

Agent restoration uses production Preview/Apply and waits for the resulting
Operation before service or file removal. A blocked restore reports the Agent IDs
and leaves the installation available for a user decision. The Task, exact owned
shell login command and recorded files are removed; a foreign Run command or file
is preserved. User PATH removal applies only when the installer added that fixed
entry. Data removal is an explicit unchecked option and does not turn the headless
state root into the NSIS program directory.

Restoration includes each independently managed model, collaboration Skill and
management Skill. Codex's model profile and original root are distinct contexts:
profile status displays both root-owned Skill facets, so uninstall restores those
through the root and any model profile separately. Explicit native profile targets
from discovery remain bound to status, Preview and Apply. Incomplete or conflicting
managed state blocks uninstall before any restore is applied.

If the service cannot start or Agent state cannot be enumerated, interactive
recovery/uninstall offers a separate confirmation with **No** selected by default.
It explains that configured Agents may need manual restoration, affected Agents
cannot be listed, and stopping the owned unresponsive Task may interrupt work.
Continuing always retains all business data, including restoration material.
Silent/passive mode refuses this exception; a known restoration conflict cannot
be bypassed by it. Retry the installer to recover an interrupted installation.

## Evidence and release claims

The native acceptance inventory is
[`windows-desktop-cases.json`](../scripts/windows-desktop-cases.json). Windows 11
x64 and Server selections require real standard-user execution, the candidate SHA,
installer digest, OS/build and architecture. A Server result is not Windows 11
evidence. Browser fixtures, Cargo checks and package construction do not establish
installation, native authorization, upgrade recovery or a live model call.

The [release builder](release-builds.md) keeps a Desktop installer separate from
the headless archive. Release publication requires both native selections and
refuses an identity sidecar that is incomplete or bound to another artifact.
