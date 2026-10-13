# Windows headless HiRoute

Windows standalone uses the same `hiroute.exe`, role-all `hirouted.exe`, Application,
Gateway, business stores and observations as the other headless hosts. It does not
require Desktop or WSL. This page describes the implementation candidate; a release
claim requires the native acceptance gates below to pass at the published revision.

## Install a verified candidate

Use native Windows Python 3 and a distributor-provided archive and companion manifest
from one exact source revision. The installer checks the archive size and SHA-256,
closed file inventory, every file digest and the architecture of all three executable
images before changing an installation. Do not infer an unpublished download URL.

```powershell
python scripts/install-standalone-windows.py install `
  --manifest C:\packages\PACKAGE.tar.gz.json --archive C:\packages\PACKAGE.tar.gz
```

The default manager is Task Scheduler. The ordinary user installs and starts it without
administrator privileges. `hiroute.exe` and `hirouted.exe` have stable hardlink entry
points under the actual profile's `.local\bin`; versioned binaries live under
`.local\lib\hiroute`. Add the stable directory to the user PATH, or invoke the full path.
User PATH changes are not required for daemon-managed executable discovery.

The native Profile and LocalAppData KnownFolders select the installation and runtime.
Environment variables named HOME or XDG do not redirect an installed Windows service.
An installation maintenance journal blocks daemon admission until file publication and
the actual manager registration agree. Retained prepared journals require repair or
completion; a successful file copy is not a completed installation.

```powershell
hiroute service start --output json
hiroute service status --output json
hiroute system status --output json
hiroute gateway show --output json
hiroute service autostart enable --output json
hiroute service stop --output json
```

Starting, stopping, restarting, logs and doctor use the same CLI family. Business
readiness requires Local Control to answer and the actual Gateway listener to be ready.
Task registration or a successful process-manager call alone cannot establish readiness.
Foreground operation remains available through `hiroute service run`.

## Background manager policy

| Manager | Identity and permissions | Lifetime |
| --- | --- | --- |
| Task Scheduler, default | Current user's SID, InteractiveToken, least privilege | Survives terminal exit; demand start and login autostart are separate. Requires that user's interactive session; logout ends this mode |
| Windows service, optional | Authenticated owner account; administrator registers a sealed service host; business child has the same SID, medium integrity, disabled administrative group and reduced privileges | Runs in service session zero, including without an interactive login; manual and automatic SCM startup are separate |

For a standard user who needs a service, prepare the installation as the owner:

```powershell
python scripts/install-standalone-windows.py install `
  --manifest C:\packages\PACKAGE.tar.gz.json --archive C:\packages\PACKAGE.tar.gz `
  --manager scm --external-manager
```

The result is `pending_admin`. In an elevated administrator session, use a CLI obtained
and verified independently by that administrator:

```powershell
python scripts/install-standalone-windows.py admin-scm `
  --cli C:\admin-verified-package\bin\hiroute.exe `
  --account COMPUTER\owner --operation install
```

The installer prompts for that owner's password; automation may use `--password-stdin`.
Use the qualified local or domain account and its Windows password. A Windows Hello
PIN is not a LogonUser password. The default Task mode needs no account password.
Passwords do not belong in command arguments, environment variables, task XML or files.
Never elevate the CLI from a standard user's writable installation directory. Keep the
administrator's verified `hirouted.exe` beside that CLI: the sealed host must come from
that trusted package and match the candidate installed by the owner.
The helper authenticates through LogonUser, derives the SID from the returned token,
loads the profile, reads the owner's private marker and held image while impersonating
the owner, and leaves impersonation before changing machine service state.

Return to the ordinary owner's session to finalize, using the prepared versioned CLI:

```powershell
python scripts/install-standalone-windows.py finalize `
  --cli C:\Users\owner\.local\lib\hiroute\VERSION\hiroute.exe
```

Finalization verifies the actual SCM account, binary path, sealed host and descriptor
before committing the private installation definition. Upgrades use the same prepared
transaction. Uninstall prepares as the owner, uses administrator `admin-scm --operation
uninstall`, then finalizes as the owner only after SCM removal is confirmed. A pending
service deletion retains the journal instead of reporting completion.

The SCM host runs from an administrator-protected ProgramFiles directory. It executes
the verified owner image only in its restricted business child; it never interprets a
user-selected executable path while privileged. Service-account profile loading is an
[SCM responsibility](https://learn.microsoft.com/en-us/windows/win32/services/service-user-accounts);
the separate authenticated helper explicitly follows
[LoadUserProfile requirements](https://learn.microsoft.com/en-us/windows/win32/shell/local-user-profiles).

## Native boundaries and feature coverage

Application behavior, routing, model/plan management, subscription login, observations,
configuration transactions and Worker workflows retain their existing owners. Windows
adapters supply the OS mechanisms those owners require:

- Private named pipes carry Control, protected input, lifecycle and grants. Account SID,
  held runtime-directory identity and kernel peer identity bind the endpoint. Remote
  clients and occupied endpoints are rejected; bounded records replace Unix half-close.
- Job Objects contain daemon, CPA, native Agent and tool process trees. Membership and
  explicit inherited handles are assigned during creation. A leader exit does not mean
  surviving descendants are stopped; cancellation waits for the Job to become empty.
- Business processes set new-object owner/default ACL to their actual user SID before
  third-party code runs. This covers Go auth files and SQLite sidecars without changing
  the parent shell token. CPA-generated inherited private files have a narrow reader;
  protected HiRoute files and borrowed native-client credentials keep separate rules.
- Reparse-free, handle-relative file operations bind reads, creation, rename, diagnostics
  and cleanup to held objects. Private new objects receive their ACL at creation.
  Existing accessible native configurations retain their ACLs during discovery.
- Windows configuration fingerprints include actual owner/DACL/read-only facts. Windows
  V5 artifact markers authenticate these facts in backup AAD and restore them conditionally.
  Unix V4 fingerprints and registered V2/V3 recovery adapters retain their released bytes.
- Executable discovery resolves native PE binaries and declared Node entry points. Shell
  metacharacters in user paths/arguments cannot be turned into a managed shell command.
  Codex, Claude Code, Qoder, Pi and DSH retain separate native adapters and capability probes.

Protected CLI carriers use pointer-width inherited handles on Windows:
`--capability-handle`, setup `--spec-handle`, and protected-input `--secret-handle`.
These accept private, non-stdio pipes with explicit inheritance and bounded bytes/EOF;
they do not accept credential text, a pathname or an arbitrary numeric PID as authority.
Unix descriptor flags keep their existing behavior. JSON request files/stdin remain
ordinary bounded request sources and cannot grant authority.

## CI and ongoing maintenance

Native CI binds each report to the full product SHA, validation SHA, runner image,
Windows version, CPU architecture, toolchain and selected cases. Missing or zero-selected
required cases fail. Build checks, fixture tests and production business acceptance are
separate evidence. Windows Server results establish those server runs; Windows 11 and
each promised executable architecture require their own recorded native runs.

Required gates cover pipe identity/deadlines, junction and hardlink refusal, native ACL
restore/authentication, complete file publication, process-tree cancellation, literal
argv, diagnostics rotation, actual installed foreground Control/Gateway/input/restart,
Task lifecycle, standard-owner SCM session-zero lifecycle, installation upgrade/recovery,
and the five native Agent adapters. Subscription/provider tests use controlled endpoints;
actual account login/refresh and upstream native clients require their own authorized
acceptance evidence and pinned version records.

Future features use the shared business owner and extend the affected platform adapter
and native gate together. CI catches deterministic OS and contract regressions without
a personal Windows computer. Fresh runner profiles do not represent every enterprise
policy, antivirus product or upgrade history; any field failure needs a retained,
reproducible fixture before it becomes an ongoing regression gate.
