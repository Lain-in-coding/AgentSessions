# O7 semantic review

- Date: 2026-10-15.
- Scope: the current O7 production and test diff against S2
  `e8e4975817f71647c158f6d802a7b98cee2f1b15` in the canonical product checkout.
- Mode: independent static review and existing-log inspection. No product
  edits, builds, tests, benchmarks, or Git mutations were performed.
- Read order: `check.jsonl` and its nine references, then `prd.md`, `design.md`
  and `implement.md`. O7 authority is design section 9, including its 500-key
  limit; the earlier 512-key research proposal is not the accepted design.

## Source identity

The checked worktree has S2 as HEAD and one modified product file:
`crates/agent-session-grep-adapters-sqlite/src/lib.rs` (+411/-64 lines).
Its SHA-256 was unchanged across three observations:
`62853001be4194b680e206a5ae66aa9ab0af60e858c0118afba2425bd9fbd68f`.
The saved S2 file has SHA-256
`2ec45e89d3c10e58690d60a1816b62779bd4263624893b4388530fb39f9c97a1`.
Line references below belong to that checked worktree, not to a future commit.

## Findings

No blocking correctness or design-compliance issue was found in this diff.
The only production changes are the new helper and replacement of the old
relation-delete loop with one helper call. The remaining changes introduce
four tests and share the existing full-table snapshot helper with those tests.

| Contract | Source proof |
| --- | --- |
| Contiguous same-kind runs | `lib.rs:6888` groups only adjacent Edge/Edge or Placement/Placement pairs. It never sorts or merges across another variant. The original relation vector is borrowed without mutation. |
| At most 500 bound keys | `BATCH_IN_CHUNK = 500` at `lib.rs:100`; `run.chunks(BATCH_IN_CHUNK)` at `:6917`; one placeholder and one bound ID per item at `:6918-6931`. It does not use the 512-row upsert constant. |
| Exact key set | Fixed variant-to-table/key mapping at `:6900-6902`, and only ID values are parameters. There is no caller-controlled SQL identifier or additional predicate. All keys of each run are traversed exactly once. |
| Activity/Usage unchanged | Their old single-row SQL and error mapping remain at `:6903-6914`. The grouping predicate rejects even adjacent Activity/Activity and Usage/Usage pairs, so these runs are singletons. |
| Empty input | The slice run iterator yields no run for an empty slice; therefore the helper issues no SQL and never indexes an empty run. The zero-count test also checks this through the statement counter. |
| Verification before deletion | The phase-2 transaction is created at `:6947`; `verify_pending_in_tx` remains at `:6950`, before the helper call at `:7089`. Stored manifest field comparisons and generation CAS at `:6370-6467` are unchanged. |
| Atomic failure | Every delete error is propagated through `map_err(backend)?`; the helper borrows the existing transaction and does not commit. The unchanged enclosing transaction is dropped on error. The late-chunk test exercises the full source-batch path. |
| Intent bytes and other writes | Manifest construction/validation, source replacement, projections, integrity checks and generation activation are outside the production diff. Duplicate and overlap rejection remain at `:838-867`. |
| Schema and dependency scope | `SCHEMA_VERSION = 19` at `:8912`; no migration, schema, Cargo, harness, CI, provider or CLI file is modified. Production schema definitions contain no foreign-key or delete-trigger dependency for the changed tables. |

Same-kind row order can differ inside an `IN` statement. This is the accepted
design boundary, not a newly preserved ordering guarantee. The schema guard
test checks the four relation tables for foreign keys and triggers; production
source inspection also found no incoming foreign-key declaration. Test-only
triggers make the cross-kind and rollback guarantees observable.

## Test assessment

- `relation_delete_batches_cover_boundaries_and_exact_keys` (`:13545`): tests
  0, 1, 500 and 501 keys for both Edge and Placement. It checks the exact
  surviving unrelated key, row count, statement count including BEGIN/COMMIT,
  and missing-key no-op with a full snapshot. It also checks the current
  no-foreign-key/no-trigger schema premise.
- `relation_delete_runs_preserve_cross_kind_order` (`:13640`): includes
  repeated Edge/Placement runs separated by other variants and two adjacent
  Activity and Usage deletions. Trigger-recorded kind sequence must match the
  input, and every target table must be empty afterward. It deliberately does
  not demand row order inside a same-kind run.
- `relation_delete_late_failure_rolls_back_prior_chunks` (`:13733`): starts
  from a real 502-placement source batch, then performs an empty replacement.
  A trigger on the second Placement chunk first verifies that the prior 500
  deletions actually happened, then raises ABORT. Full snapshots, including
  derived tables, must roll back; old outbox rows remain exact and exactly one
  new `building`/`intent` row remains with the original base generation.
- `relation_delete_manifest_tamper_fails_before_deletes` (`:13800`): changes
  stored `relation_deletes_json`, checks the durable-intent mismatch error,
  verifies zero Edge/Placement DELETE statements and compares a full snapshot.
- The moved `store_snapshot` helper (`:12105`) retains the old implementation;
  the existing identity-sidecar tamper test now uses it without weaker checks.

The tests address the required semantic risks and the change in statement
shape. They are not a substitute for the new candidate's strict oracle or
complete workspace gate.

## Existing O7 log observations

The following private files were inspected under `o7-prototype-01/`:

| File | SHA-256 | Observation |
| --- | --- | --- |
| `targeted-tests-01.log` | `5a3b4a2cbe2247beb8aec76e0da33615747cd85e7fd8287fb387c87e6adac415` | Four targeted tests passed; no failures or ignored tests. |
| `adapter-tests-01.log` | `418b36362a31e992d13cf3d6cc247219815ca9e7f41ba05eb92fe79aa5586930` | 308 unit plus 36 integration tests passed; zero failures or ignored tests. |
| `clippy-01.log` | `e64f620c1b264783d78e7fd18b58f6a6499d371f4840e104ebdea7a8dbc726a9` | Workspace crates reached the finished dev-profile line; no warning/error lines were found. |

These logs do not include a checked candidate/source-hash and command/exit-code
receipt. They support the implementation status but are not claimed here as a
new independently executed or fully source-bound gate. No release candidate or
build receipt was present at the last artifact inventory.

## Remaining validation and spec follow-up

1. Build the isolated candidate and bind its source, binary hash and build
   receipt. A/B control must be the S2 formal frozen binary with SHA-256
   `03c168a0d146ec9d0e7cfc3d49602468457e9e3643bd17f443e0407ee70bd6cd`.
2. Preserve every alternating A/B sample and the independent trace. Only a
   candidate with a material, stable benefit advances to new strict-oracle and
   complete quality-gate validation.
3. Run fresh fmt, workspace Clippy/type checking, workspace tests and the
   applicable evidence/governance suites after scheduling with the root agent.
   No S2 receipt is reused as proof for the O7 source.
4. If O7 is retained, extend the adapter's changed-sync spec to distinguish
   512-row upserts from 500-key contiguous Edge/Placement deletes, including
   cross-kind order, verification-before-write and rollback tests.
5. Freeze S3 only after those gates. The new formal suite and unchanged R4
   comparison remain required. S2 shrink's failed 12.3222% improvement must
   stay disclosed; this review does not establish performance success.
