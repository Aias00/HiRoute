# Validation execution locations

The caller's `~/.config/hiroute/validation.json` selects backend, Desktop and
frontend execution independently. Inspect it with `python3 scripts/validation.py show`.
An SSH backend uses the host and HOME-relative workbench configured in
`~/.config/hiroute/remote-rust.json`; configuration belongs to the caller.

Each Git origin needs its own workbench repository and checkouts. Select a
different `workbench` for another origin rather than resetting an existing
repository. The SSH driver also accepts `--workbench` before its operation.
All such workbenches retain the account-wide capacity locks under the original
default root. Changing a checkout namespace does not create more compiler slots.
Each checkout keeps its own default `target/`; compiler objects may use sccache.

The submitted candidate and validation driver have separate revision identities.
The driver comes from committed bytes at the caller's HEAD; `--sha` selects the
candidate from the advertised branch. Both revisions must satisfy the driver's
Git admission checks. This permits a tooling-only driver repair to validate an
earlier unchanged product candidate while recording both actual revisions.
Evidence still belongs to the tested candidate, not every later commit.

```sh
python3 scripts/validation.py backend run --ref refs/heads/codex/my-change \
  --sha FULL_CANDIDATE_SHA --plan my-change --phase final -- \
  cargo test --locked -p hiroute-cli --lib
```

Read failed reports before retrying. A checkout or runner failure means Cargo
was not executed. Related-run IDs must exist in the selected workbench and have
executed reports; changing workbenches does not import old evidence. Preserve
the original failure and revision in the feature's diagnostic record.

Native Windows and hosted CI have their [own execution entry](github-actions-validation.md).
Linux backend success does not establish Windows runtime, account or logged-owner
Task acceptance. Native Desktop and real product scenarios likewise require
their applicable execution environment.
