# Local Worker platform

## Purpose

The platform launches selected executables, owns child/stdio handles and explicit
private materials, observes process state, and performs bounded owned cleanup.
It does not own task admission, permission policy, ACP semantics, or business recovery.

## Requirements

### Requirement: Launch consumes the explicitly selected native form

The launcher MUST consume exact executable/args/env/materials prepared by the profile.
Adapter ACP uses selected adapter/CLI/runtime; native ACP uses the selected CLI
itself; Pi uses selected official CLI/SDK plus Node. The platform cannot guess a
missing adapter/runtime, switch PATH candidates, install/download, parse Harness
configuration, or build another launch API from Worker data.

Installation discovery is metadata-only. It cannot hash/pin/fingerprint user CLI/
adapter/Node program contents, monitor changes, or run simulated New/Load/Cancel,
including cached/background/first-use replacements. Explicit Pi SDK interface and
borrowed-Claude launch capability checks below are narrow exceptions, not general
preflight version whitelists. The product's own package/build/test artifact identity
validation remains a separate contract. Installation selection permits a try, not
successful ACP, login, native route, resume, or task authority.

### Requirement: Run materials have explicit paths and ownership

Each launch creates its own private root and only declared relative files/directories,
within entry/content limits. Parent directories are explicit. Traversal, conflicting
paths, or session-root nesting cannot broaden ownership. Ordinary files are user
read/write; an explicitly declared secret-free native bridge may also be user
executable. File content/name/env cannot imply executable policy.

Borrowed HOME/native context/session roots are not launch materials and cannot be
created/modified/deleted merely because an env variable names them. Concurrent
launches remain independent. Secret data never enters argv/log/error. Cleanup keeps
object identity/open handle and rejects replacement/reparse/symlink ownership drift
before deleting its own root; a stale pathname/PID/inode is not deletion authority.
These private materials are not same-UID sandbox isolation.

### Requirement: Spawn returns owned process and stdio without a helper protocol

Daemon directly owns the child and newly created resources. ReadyWorker means spawn
succeeded and stdin/stdout/materials were delivered, not initialized ACP or execution
success. No helper, nonce echo, or platform-ready handshake is required; nonce is
local correlation, not resource authority. Undelivered/error paths clean owned
resources and report actual failure. ACP initialization belongs to the executor.

### Requirement: Observation and stop report the actual proven scope

Observation is Running, Exited with optional code, or Unknown. Stop separately reports
root/process-group/Job scope and residual state. Only currently held/proven launch
objects may be signalled; historical or Worker-provided PID is not kill authority.
Root exit alone is insufficient proof ordinary adapter/native tools stopped, and
none of the supported scopes promises arbitrary malicious detached-process cleanup.

Unix children start in a new process group. Stop sends TERM once, retains a short
bounded native-cleanup window, then KILL fallback within remaining budget. Keep the
unreaped leader identity while signaling even if adapter exits first; after reap,
old group IDs support absence checks only, not destructive signals. Budget expiry
retains owned handles/state for a later bounded stop. A root-only platform needs
separate native evidence/mechanism for ordinary child-chain cancellation; current
Windows coverage is recorded from actual runs and their environment. Real cancellation proves tool side effects
stop while neighboring work continues, not just a signal or root exit.

### Requirement: Bounded cleanup does not replay business work

While daemon lives, platform uses caller's finite stop/cleanup budget and returns
active/unknown/cleanup-failed honestly. Disconnecting an observer does not establish
stop. Daemon-crash reconciliation is best effort and does not restore a process handle
from an old PID. Platform cannot own task database/cancel intent, auto-redo work,
resume native sessions, or acknowledge manual residual handling. Normal supported
cancel requires real success evidence; unknown residual is not a universal substitute.
Capabilities describe basic selected-installation start/stop, not tool/network/file
sandbox, universal tree kill, or crash-without-residue. Ordinary permitted coding/
shell input cannot be rewritten or rejected solely for those non-guarantees.

### Requirement: History cleanup follows the actual storage owner

Task/native maintenance, not launch-material cleanup, owns retained task metadata,
Pi/DSH task history, and registered older private history. Delete only with proven
ownership, no active user, and all related retention conditions satisfied. Unknown
old roots are retained and failure is retryable through existing jobs. Borrowed
Codex/Claude/Qoder history belongs to the native client and cannot be registered as
HiRoute-owned configuration roots or recursively deleted. Archive/body hiding does
not imply native physical erasure. Seven-day body expiry starts from each original
content event, not terminal time, while task/run/key/tombstone/aggregate metadata
has its separate lifetime. The platform adds no generic GC or whole-instance disk cap.

### Requirement: Borrowed Claude checks one native isolation prerequisite

Before a real Claude Worker using borrowed settings relies on host-managed provider
protection, profile MUST check the selected CLI's bounded `--version` in disposable
non-secret roots with the selected command path. The known supported Claude 2.x
lower bound is 2.1.231. Older/unknown-major/unparseable/failed/timed-out output fails
that launch before model work. Stop the owned check process before removing temporary
roots. This check does not run during discovery/save or ordinary isolated probes,
read program contents, cache identity, simulate a task, or select another CLI.

Borrowed JavaScript adapters require an explicit selected Node to execute the private
environment-protection bootstrap before adapter import. Protected route/auth/context/
CLI/loader/proxy settings cannot be replaced/deleted, including protected absence.
The protection is process-local and not propagated as a preload to arbitrary tools.
A missing Node cannot launch unprotected even if a shebang adapter was selectable.

### Requirement: Pi checks same-package interfaces without model or resource activation

Pi binds the selected official npm CLI to its manifest-declared executable and SDK
export from that same package. Version labels, including acceptance reference 1.0.2,
are diagnostic, not a production allowlist. Current bridge requires Node >=22.19.
Passive discovery never imports the SDK. Explicit dependency selection, selected-
installation availability, and actual startup check Worker resource/provider/new-
session interfaces; Continue additionally requires native open. Model/collaboration
configuration uses only its own required interface subset.

Checks use selected Node offline with bounded output/time: version 10 seconds/64
bytes, SDK 15 seconds/128 bytes. They create no native session/prompt, load no user
auth store/helper/extension, refresh no model catalog, and download nothing. Failure
is not ready; availability maps to artifact_unavailable, while real action errors
retain the closed stage/reason with safe retry classification.

Execution borrows effective HOME/PI_CODING_AGENT_DIR/cwd and existing user/project/
installed-package Skills through offline resource loading. It uses an empty in-memory
credential store and binds only frozen provider/model/run token/budget. User models,
auth, settings, and resources remain unchanged. Current Pi Worker supports approve-all;
extensions/MCP/other policies require separate support rather than inference.

Task-owned `native-pi.jsonl` is strict v3. Before native open, Continue checks header,
context, exact session, message, and compaction shapes. Missing/damaged/older/unknown
history or missing required message content fails without SDK migration, repair,
skipping bad lines, or replacement session. Native compaction/Continue retains the
frozen route/budgets and actual native summary. SDK release-label change alone may
remain compatible; a changed writer format independently fails this history contract.

## Implementation references

- [Launcher ownership](../../../crates/daemon/src/delegation/local_worker/README.md) and
  [OS platform scenarios](../../../crates/daemon/tests/local_worker_platform.rs).
- [Native profile boundary](../../../crates/daemon/src/delegation/profile/README.md).
- [Pi capability checker](../../../crates/integrations/src/agents/pi_runtime.rs) and
  [SDK bridge process scenarios](../../../crates/daemon/src/delegation/profile/pi_worker_bridge.test.mjs).
