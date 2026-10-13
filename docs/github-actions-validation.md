# GitHub Actions validation

GitHub Actions runs on the checked-out commit. Linux checks use `scripts/ci-run.py`;
native Windows acceptance uses `scripts/windows-headless-ci.ps1`.

## Workflows

| File | Execution | Scope |
| --- | --- | --- |
| `gateway-core.yml` | PR, main push, manual, reusable | Affected PR scope or full main Linux backend formatting, size boundary, clippy, tests, E2E contract; full plans use coverage-checked parallel shards; separate frontend build/tests and model-data generator checks |
| `p0-gateway-gates.yml` | Manual, reusable | Linux listener build, neutral loopback, H1/H2 lifecycle, replay/privacy tests, contract checks, benchmark harness smoke |
| `gateway-core-dedicated.yml` | Manual or reusable | Hosted Linux production stability and socket churn |
| `p0-gateway-final-gates.yml` | Manual | Aggregate the three current hosted workflows |
| `build-release.yml` | Manual, exact dispatch SHA | Native macOS ARM64/Intel DMGs and Linux/Windows x86_64/ARM64 archives; uploads candidate assets and integrity evidence for the existing release publisher |
| `windows-headless.yml` | PR, main push, manual, reusable | Native MSVC primitives, full shared business libraries, foreground control/Gateway, real Task/SCM installation and fixed upstream Agent clients |

Normal Rust checks select the organization `Default` runner group with the
`cncf-ubuntu-16-64-x86` label. The organization must permit this public repository
to use that group. Dedicated stability continues to use `Default Larger Runners`
with `ubuntu-latest-16-cores`. Frontend, selection and result-only jobs use the
standard GitHub-hosted Ubuntu runner. Jobs receive read-only repository permissions,
checkout does not persist credentials, and pull-request jobs receive no repository or
deployment secrets.

An affected PR plan remains one backend shard and executes the exact commands selected by
`test-plan.py`. A full plan expands into eight bounded jobs: formatting/clippy, unit and
binary tests, three ordinary integration-test groups, Gateway E2E, Product smoke, and the
remaining doctest/contract/transport gates. Each Cargo test shard keeps the full workspace
package and all-features context. `ci-shards.py check` compares every non-Desktop Cargo
integration target with the explicit shard inventory; a missing, duplicate or same-name
cross-shard target fails selection instead of being silently omitted. The required
`backend` status is a small aggregate job, so the public branch-protection contract remains
stable while individual shard jobs can run concurrently.

Changes to `gateway-core.yml`, `ci-run.py`, or `ci-shards.py` force a full plan on the pull
request because those files define the hosted backend execution contract. Changes only to
their tests or documentation keep the ordinary tooling-only selection.

The website release catalog (`apps/website/data/releases.json`) also selects the
`hiroute-host-runtime` library tests and package Clippy in backend CI. The native
upgrade reader consumes this data, so website validation alone cannot cover a new
release. Other website-only edits retain their dedicated workflow.

The Windows jobs use the standard hosted `windows-2025` image. They do not require
an organization owned Windows runner. Native inventories must contain every
required case; zero, ignored or missing required cases fail. The shared library
scope executes the complete ordinary native test inventory. Explicitly registered
installation-input gates and child fixtures have their own execution entries;
unregistered ignored cases fail the ordinary inventory. Fixed Agent gates
explicitly select their opt-in real client tests. Their npm versions and all
transitive package integrities come from `tools/windows-agent-fixtures/package-lock.json`.
Qoder's fixed official archive has an independent SHA-256 check. No provider
credentials are supplied by pull request jobs; a login prerequisite remains a
failed gate until authorized account validation exists.

Background jobs build the unchanged pinned CPA with its two native bootstrap
security cases and use the production package generator twice. The administrator
receives a separate verified complete package. Actual ordinary-account Task and
SCM cases exercise authenticated account selection, protected host publication,
session boundaries, stop/recovery policy and two-phase finalization. The separate
candidate upgrade gate needs explicitly bound baseline package inputs; a new
Windows platform has no released Windows baseline to invent.

For a separately selected Windows 11 workstation run, `-CaptureArtifacts` records
the exact Cargo `compiler-artifact` executable for every required test recipe,
the product binaries, locked source inputs and complete package bytes. The
`windows-native-replay.ps1` consumer rejects a partial scope, verifies every hash
and source SHA, and runs those executables without rebuilding Rust. It restages
the whole verified package with fresh administrator ACLs before Task/SCM tests.
Static PE import auditing rejects Debug CRT dependencies; actual process launch
still owns DLL resolution and runtime acceptance. The `windows-11-arm` hosted
consumer explicitly records x64 emulation on ARM64, which does not establish
native ARM64 product support. Consumer results retain the producer revision and
have their own runtime scenario evidence.

The toolchain comes from `rust-toolchain.toml`. Cargo sources are cached, not
`target/`; Linux shards do not exchange compiled binaries. Windows replay retains
the original producer build evidence and assesses the consumer separately.
The shared Rust setup exports `CARGO_NET_GIT_FETCH_WITH_CLI=true` for
subsequent job steps, so pinned Git dependencies use the installed Git client
instead of Cargo's embedded libgit2 transport. This avoids the libgit2 SSL failure
observed when fetching Pingora on a cold hosted runner; TLS verification and locked
dependency revisions remain enforced. Fetch failures still fail the job, and the
setup does not retry test commands.
Build concurrency defaults to the runner CPU count; incremental compilation is
disabled. Every job keeps its own checkout-local Cargo output. The hosted job is
disposable, so there is no persistent worktree cleanup service.

## Evidence and failures

Each invocation checks the full expected SHA against HEAD and rejects tracked
source changes. On pull requests this is GitHub's checked-out test commit, not
an inferred branch tip. The wrapper waits synchronously and returns failure
on process failure, timeout, revision mismatch, or a required Rust test invocation
with no passing tests. It preserves the child process exit separately from its
own decision. `scenario_state: not_assessed` is intentional: process success does
not establish product scenario acceptance.

`artifacts/ci/<name>/` contains the complete command log and JSON result, including
revision, CPU count, platform, timestamps, and Cargo/Rust versions for Cargo
commands. Failure prints the log tail; Actions uploads available artifacts even
when a check fails. Each shard uses a distinct artifact name; the aggregate job does not
replace those per-command records. Timeout and cancellation stop the command process group
before removing its private short TMPDIR. A forcibly terminated VM cannot be
expected to finish uploading artifacts.

The adapter can be tested without Rust compilation or remote access:

```sh
python3 scripts/test-ci-run.py
python3 scripts/test-ci-shards.py
python3 scripts/ci-shards.py check
```

## Coverage boundaries

The fixed-machine benchmark and semantic result-checking scripts remain available for
maintainer validation; hosted benchmark smoke is not fixed-machine performance evidence.

The hosted Rust checks exclude `hiroute-desktop`. Frontend build/tests do not
prove native Tauri interaction. macOS Desktop remains on the existing local
validation path. The separate [release candidate workflow](release-builds.md)
builds native macOS, Linux and Windows packages on both architectures. Building
an archive records package integrity and an awaiting-runtime state; it does not
replace that architecture's native business acceptance or Desktop validation.
Real-provider credentials and separately provisioned production E2E fixtures are
not supplied by this CI setup. Existing failing checks remain failures, not
expected-red success or waived gates.

Tooling-only pull requests run their selected Python checks in the selection job and retain
CI reports. Contributors can run `python3 scripts/test-plan.py --base origin/main` to inspect
the affected command set before opening a pull request.
