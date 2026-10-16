# Platform compatibility

Use for a change to OS-facing behavior, including shared changes made on macOS/Linux
that may break Windows. Do not start another implementation, account acceptance or
platform matrix merely because a file has a Windows branch.

## Preserve the right boundary

Start from [platform primitives](../../../../crates/platform/README.md) and the
affected caller in the [code map](../../../../docs/code-map/architecture.md).

- Prefer `hiroute_platform::private_fs` for new private directories/files, protected
  handle reads and owned temporary cleanup, and `durable` for journaled publication.
  Add genuinely shared OS behavior there with Unix/Windows leaves. Keep business
  lifecycle, payload limits, authorization and error mapping with their caller.
- Distinguish newly owned material, existing storage and borrowed native configuration.
  A common API must not chmod/re-ACL borrowed paths, accept a substituted object or
  erase a permission fact from an authenticated restore record. Preserve existing Unix
  directory modes and each caller's single-link policy; do not infer private ownership
  from a pathname, mode or a successful open alone.
- Check intermediate failure states. A complete file payload is not the same guarantee
  as no-clobber creation, durable rename or recoverable installation. Do not introduce
  a hard-link requirement or duplicate directory fsync into Unix callers as a side effect
  of Windows support. Remove only owned temporary objects on ordinary failure.
- Keep meaningful native differences explicit: Windows ACL/owner identity, reparse
  traversal and delete-sharing rules are not POSIX modes or unlink semantics; shell
  argv, child process cleanup, console attachment and user background tasks also need
  their native owner. Avoid business functions with parallel copies of the same policy.
- A Windows launch workaround must not silently alter Unix shell/login/PATH behavior.
  Test-only diagnostic facts stay test-only; production diagnostics use the existing
  channel and must not print credential content or native user configuration.

## Select evidence by the affected behavior

Use [validation](../../hiroute-validation/SKILL.md) and its
[Windows iteration route](../../../../docs/validation-routing.md#windows-iteration-and-release-validation).
Use the configured Linux MSVC cross toolchain for fast compilation/linking, then run
affected native cases on the configured Windows development machine. Keep SDK paths,
machine addresses and account material in personal configuration or ignored evidence.
Cross-compilation alone does not establish native execution or user-account behavior.

| Changed boundary | Evidence to select |
| --- | --- |
| Shared private filesystem primitive | Unix and Windows creation/read/cleanup cases, plus affected storage/Worker callers; reject links, substituted entries and permission adoption without deleting foreign files |
| Agent launch/environment | Affected argv/environment and process ownership cases; real native client execution when its externally observed launch contract changed |
| Background lifecycle or installation | Affected installed CLI/service route and recovery scenarios under the claimed user type; an administrator-only run cannot prove ordinary-user behavior |
| Shared persistence/recovery | Existing compatible old-to-new/reopen/interruption cases; Windows evidence does not replace affected macOS/Linux consumers |

Record actual OS/build, processor, binary target, execution mode, candidate SHA and
artifact hashes. Windows Server execution does not prove Windows 11; x64 emulation
on ARM does not prove an ARM-native package. Do not expand the user's agreed scope
to RDP, logoff continuity or next-login behavior without a relevant requirement.

The hosted Windows matrix is a release-candidate gate with a manual diagnostic entry.
Keep the full matrix release/manual-only; the path-filtered PR compile check
does not run runtime acceptance. Do not mark real failures nonblocking just to reduce
iteration time. Failed/cancelled producers cannot supply complete replay artifacts;
distinguish that dependency failure from a native test failure. Preserve failures and
their exact revision until the relevant cause is repaired and checked.
