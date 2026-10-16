# Local data, retention, and recovery

## Purpose

HiRoute owns local business stores and their supported upgrades. Logical visibility,
physical reclamation, recoverable backups, and native Agent-owned history have
different lifetimes.

## Requirements

### Requirement: Startup preserves damaged or unsupported data

Startup MUST distinguish stale socket, unready service, and unreadable local stores.
Damaged control/runtime/secrets/observation state stops business startup and retains
original directories. It cannot silently reset, overwrite with an empty database,
or claim migration did not happen merely because the service later failed.
Only a genuinely fresh root may initialize; an incomplete existing three-store set,
unknown files indicating prior state, or recovery backup prevents fresh initialization.
The real bootstrap diagnostics directory may already exist without being discarded.

Desktop MUST expose actual local recovery/backup directories and actionable
instructions even when daemon is unhealthy. Opening them changes no data. A verified
backup requires whole-group restoration with its corresponding complete old binary,
not deletion of storage or a single database replacement. Archive/restart recovery
retains actual backup ownership and package identity. A changing filesystem device
number after reboot cannot alone invalidate logically intact supported store identity.

### Requirement: Supported upgrades use one three-store coordinator

0.2.0 is the first supported release baseline; historical 0.1.0/schema22 records are
retained evidence, not supported conversion inputs. A later latest-stable release
does not silently retire supported earlier sources. Current startup accepts only
the declared schema23–29 set with stable format marker `(1, 1)` and advances through
the current schema29 coordinator. Future/unmarked/unsupported formats fail before
business/key/backup mutation. Acceptance of a format alone does not expand the
public release-support promise.

Control, runtime, and secrets migrate as one verified BackupSet with source digests,
store/key identity, and recorded phase. An inconsistent mixed-version live set is
not permission to fix one database independently. Without a prior BackupSet only
a complete consistent supported source can start a new batch. An interrupted batch
finishes its original target and archives before a separate newer-target batch;
its source or target cannot be rewritten to latest. A next-store migration commit
may be resumed only when identity and migration ledger match the recorded phase.
A later store advancing out of order or key/source mismatch fails closed.

Registered schema24 dependency-selection and schema25 Agent-check conversions
preserve original JSON, identities, steps, effects, check revisions/times, and
replay meaning; they do not re-run inference or fabricate validation. Further
supported schema changes retain the same coordinator and frozen-source regressions.
Completed conversion never automatically restores old data. Manual recovery pairs
a complete old backup with a binary verified to read that source. Supported data,
configuration, and public CLI/Gateway semantics are ongoing obligations, not waived
by internal refactoring or development-stage terminology.

### Requirement: Local retention does not imply automatic external upload

Conversation bodies, per-request detail, and route-stage quality samples default
to seven-day local retention and share authorized expiry/deletion boundaries.
Longer-lived aggregate value contains neither body nor competence scores. Retention
itself makes no external upload. A plan explicitly selecting built-in System One
or a custom decision extension may send current input and bounded safe visible
history to its configured endpoint according to [decision policy](../decision-services/spec.md).
Heuristics make no decision-service request. This is independent of the user's
selected business-model calls.

### Requirement: Session cleanup has an explicit bounded scope

Users MUST choose between clearing body and deleting the session record.
Body-only removes questions/answers/tool content while retaining permitted route
facts and aggregate value. Session-record deletion also removes session detail,
while non-content accumulated value remains by default. Confirmation names the
scope; cancellation has no effect. Partial failure reports the actual visibility/
cleanup state, never full success.

Visibility applies immediately across session views, task results, search, old
cursors, and content references. Replay, background projection, cached text, or
native history cannot resurrect hidden data or Continue access. Physical reclaim
may finish later and has independent status. The confirmed identity/range is rechecked;
retry cannot expand into later requests, other sessions, or model/plan/Agent settings.

#### Scenario: Another request arrives during cleanup

- **WHEN** a confirmed cleanup is applied to the previously selected range
- **THEN** new unconfirmed data MUST not be deleted, and old content stays hidden
  even if physical reclamation or late ingestion is still pending.

### Requirement: Accessible local objects are governed by OS operations

On Unix, HiRoute MUST NOT reject existing local files/directories/sockets solely for owner
UID, group/other access bits, or not being 0600/0700. This includes native auth and
Agent configuration, product data/recovery, diagnostics/private samples, CPA-owned
login directories, and local control/protected-input transport. Actual open/read/
write/connect errors remain explicit. Existing objects are not chmod-repaired to
pass admission; newly created objects retain their private defaults.

File kind, link policy, read/format/size bounds, content/inode stability, revisions,
managed child identity, ownership-limited cleanup, and locks still protect operations.
Metadata drift may require re-read, but permission differences alone cannot impose
permanent rejection. Explicit write modes/user-requested permission changes remain
output intent. Gateway/upstream auth, protected capabilities, OAuth state/session/
provider binding, and WebView ACL are independent and remain enforced.

### Requirement: Windows private state and configuration recovery retain native identity

New HiRoute-owned private objects MUST receive their user owner and private ACL at
creation, including child-created secret files and database sidecars. Parent shell
token defaults and unrelated existing user objects remain unchanged. Protected
runtime files and pipe endpoints retain their explicit SID/DACL admission; borrowed
native configuration follows its own access contract without automatic ACL repair.

Identity-sensitive operations MUST hold the actual file/directory objects and reject
reparse or replacement drift before privileged reads, publication or cleanup. A path
string, metadata snapshot or old PID is not authority over a replacement object.
Configuration fingerprints and authenticated recovery records include the relevant
Windows owner, DACL and read-only facts. Restore is conditional on the expected
current object and restores the captured bytes and native permissions together.

Windows-native fingerprint and recovery extensions cannot change supported Unix
fingerprint bytes or retire registered historical readers. Supported store conversion
continues through the existing three-store coordinator with platform-derived source
fixtures; parser acceptance or a development-package upgrade is not proof of a
released Windows-to-Windows upgrade.

## Implementation references

- [Startup format admission](../../../crates/local-storage/src/migrations/startup_format.rs),
  [current migrations](../../../crates/local-storage/src/migrations/mod.rs), and
  [interrupted-batch scenarios](../../../crates/local-storage/src/migrations/upgrade_batch_recovery_tests.rs).
- [Managed content lifetime](../../../crates/observation/src/managed_text/README.md).
- [Supported release policy](../../../docs/release-contracts.md) and
  [compatibility registry](../../../contracts/compatibility-support.v1.json).
