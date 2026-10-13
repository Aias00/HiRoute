# Operating-system primitives

This crate owns Windows kernel handles and FFI used by headless hosts. It does
not select an Agent, authorize a business command, or own business state.
`windows::background` owns the native Task Scheduler and Service Control Manager
adapters, fixed Known Folder resolution, and their closed installation definitions.
The CLI and host runtime select lifecycle policy and maintain installation journals.

`windows::security` is the shared implementation of the Gateway's existing
owner-only ACL creation, verification, and held volume/file-index identity.
New private objects receive their descriptor during creation. `open_identity`
checks the opened object type and rejects reparse points. Existing directory
handles retain identity across renames; private creation and ordinary file reads
deny deletion sharing. `open_owner_only` applies the explicitly requested private
descriptor check; it is not a repair or blanket admission rule for native files.

`windows::pipe` provides owner-only local byte streams. Its namespace binds the
current account SID to the held parent file identity and endpoint leaf, so a service and a CLI in
different logon sessions can connect as the same user. First-instance binding
rejects endpoint occupation; remote connections are rejected. Kernel peer PID
queries support launcher-owned process checks. No filesystem socket is created.

Every pending read, write, or connect owns an event and a stable OVERLAPPED
allocation. A timed-out operation is cancelled and completed before its buffer
is released. Listener polling and connection deadlines retain their callers'
budgets. Protocol owners provide bounded frames and response completion; a byte
stream does not manufacture Unix write-half-close or unbounded flush semantics.

Windows native tests exercise held identity, bidirectional frames, endpoint
exclusivity/rebinding, peer queries, and deadline cancellation followed by another
frame. Windows CI must verify that these cases are actually selected; an empty
test inventory on another platform is not their acceptance evidence.

`windows::job` launches inspected native executables suspended, assigns the
kernel Job and explicit inherited handle list before execution, and stops the
whole Job when the leader exits. Numeric PIDs do not grant termination authority.
`windows::launcher` supplies a fixed system PowerShell helper for managed commands
whose argv must cross a caller's cmd invocation. Credential pipes are exclusive,
private, bounded and explicitly inherited; their raw handle values are locators.

Background lifecycle messages authenticate a held process image, creation time,
and pipe peer identity. Task mode uses the same interactive account at LUA level;
SCM mode authenticates a qualified owner account and uses a restricted business
token beneath an administrator protected service host. `scm_owner` reads the
owner's private prepared journal only while impersonating that authenticated
account, then reverts before machine registration. Sealed publication copies a
held daemon from the administrator's trusted package and requires its digest to
match the owner's prepared image. A user supplied path or self reported digest
cannot select the administrator's executable.

Filesystem traversal uses synchronous `NtCreateFile` with an inspected
`RootDirectory`, one component, `OBJ_DONT_REPARSE` and a handle type check.
Traversal handles share deletion to coexist with DELETE-capable private parent
handles; the relative operation remains bound to the inspected object across
ancestor renames. Private creation attaches the DACL before publication. Files
and created directories deny delete sharing; existing directory identity handles
retain identity but do not promise to prevent a rename. Replay child creation
uses the caller-owned parent directly. Pipe names hash the held parent volume /
file identity and endpoint leaf instead of reopening its pathname.

`durable` publishes complete payloads through unique private stages. Initial key
publication cannot replace an existing key. Windows stages and namespace
transitions use write-through handles; file payloads use `FlushFileBuffers` before
and after held-source, held-parent rename. Windows directory barriers retain the
inspected identity and do not claim POSIX directory fsync. Existing operation and
backup journals keep their writing, committed and final checkpoints. This follows
[Windows file caching](https://learn.microsoft.com/en-us/windows/win32/fileio/file-caching)
and [handle-relative rename](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info).
Process interruption tests do not establish behavior under hardware power loss.

`file_security` captures the actual Windows owner/DACL SDDL and read-only flag
from a held file. Conditional native restoration authenticates these facts along
with content; it creates its stage with the recorded descriptor and verifies the
result before publication. Existing native files retain their accessible ACLs
during reading. A descriptor that the account cannot restore fails before target
replacement. Unix artifact modes and released fingerprints remain unchanged.
