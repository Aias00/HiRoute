# Stable schema 23 Worker journal fixture

`schema23-worker-operations.json` is frozen output from the original schema 23
producer at `cdbb2cec` (product code identical to `c509481d`). The one-shot exporter
is commit `5d8306cc`, based directly on that producer. It was run with:

```text
cargo test --locked -p hiroute-local-storage --lib export_schema23_worker_operations -- --ignored --nocapture
```

Managed run: `3724c06e070e4d00bd51bab284c8ab49`, green. Exact exporter:
`5d8306cc2f9ce5e95d79657e4d406cfd9e1ca062`. Fixture SHA-256:
`1886b84980aab4ddb7b3cd9ed21da0a654b13474e448f02883a233dc312d043f`.

The exporter opened the production `LocalStorageSet`, admitted real Worker
selection Operations and saved their effects and step journals through the old
storage API. It exported the exact SQL row values for Codex and Claude at staged,
activated and succeeded checkpoints, with a prior successful selection. It did
not export keys, authentication material or user data. Timestamps are original
synthetic-run values and are deliberately not normalized.

The current test reconstructs the published v20 Worker tables and schema 23
version/marker, inserts these frozen rows verbatim, then enters production
startup. Fresh keys and store identities are generated per test. This isolates
the actual historical contract from the current serializer while keeping the
fixture independent of machines and credentials.

Read `worker_dependencies_v24_tests.rs` for the user-visible invariants: old
Operations remain replayable, activation does not increment a revision twice,
compensation respects the current owner, interrupted upgrades require the exact
backup set, and completed backups survive the next upgrade batch. The ordinary
current-format selection test also covers the Qoder single-CLI form.

Do not regenerate this fixture from the new producer to make a failing migration
pass. Its removal requires an explicit end to schema 23 upgrade support, retaining
a documented supported recovery path.


## Windows path matrix

`schema23-worker-operations-windows.json` is a **test-only platform projection**,
not an export from a released Windows product. Windows cannot admit the original
Unix `/opt` paths as native absolute paths. The original fixture above remains
unchanged and its byte SHA is asserted on every host.

`derive-windows-schema23.py` changes only `/opt/...` installation paths to
`C:/opt/...` and their accepted, step-input and exact-selection-byte SHA-256
proofs. It first independently verifies those formulas against every original
frozen proof. It retains all SQL columns, state/checkpoint coverage, row order,
Operation identities, opaque request digests, timestamps and JSON member order;
it does not invoke the current Rust serializer or producer. The explicit
`producer_revision` identifies the projection. This tests the same raw SQL
migration/replay/compensation boundaries under valid native Windows paths,
without treating an invalid path as a production admission exception.

Frozen projected fixture SHA-256:
`5876ea437bd9160de1e0c179b4e78ae19d9cac54fea0756f5031d95c9d3543b2`.
Run the independent derivation twice when editing its rules; the second run must
leave the fixture bytes unchanged. Actual native Windows execution is required
for the typed consumer; this derived fixture is not evidence of a past release.
