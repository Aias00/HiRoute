# Operating-system primitives

This crate owns Windows kernel handles and FFI used by headless hosts. It does
not select an Agent, authorize a command, own business state, or implement a
service lifecycle. Those policies remain with the daemon and their current ports.

`windows::security` is the shared implementation of the Gateway's existing
owner-only ACL creation, verification, and held volume/file-index identity.
New private objects receive their descriptor during creation. `open_identity`
checks the opened object type and rejects reparse points while denying deletion
sharing. `open_owner_only` additionally applies the explicitly requested private
descriptor check; it is not a repair or blanket admission rule for native files.

`windows::pipe` provides owner-only local byte streams. Its namespace binds the
current account SID to the canonical logical locator, so a service and a CLI in
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
