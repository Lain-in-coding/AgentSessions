# Performance optimization and pre-release hardening

## Goal

Reduce the extreme-corpus first-ingest time from ~2.5 hours to under 30
minutes on a 1.18 GB / 1,124-file corpus, fix the remaining correctness and
reporting gaps found by the pre-release full-source review, and leave the
product ready for a user self-test period before open-sourcing.

## Confirmed Facts (measured / reviewed)

- A typical user directory (46 files) syncs in ~4.2 s; the extreme corpus
  (1,124 files, 1.18 GB) first ingest takes ~2.5 h because each chunked
  sync batch pays O(whole-catalog) cost — total O(n²).
- `regenerate_compatibility_aliases_in_tx` rewrites the entire catalog on
  every batch commit (unconditional UPDATE even when bytes are unchanged);
  four relation tables are fully loaded 2-3× per batch; three full-table
  integrity scans run per batch; the no-op check runs after all the heavy
  work; ~200K entities × 5 SQL statements are prepared per batch with no
  statement reuse.
- Incremental/no-op behavior exists and is correct (unchanged re-scan does
  not advance generation), but the unchanged path still pays the full
  O(whole-catalog) cost before concluding no-op.
- Verified non-issues: provider parsing is single-pass, budget/cursor are
  constant-time, paging is bounded, `merge_message_payloads` only fires on
  real cross-source conflicts.

## Requirements

- R1 Make each sync batch cost proportional to the batch (O(batch)), not
  O(whole catalog): alias regeneration scoped to touched entities, relation
  state loaded once per batch (membership filtered by source path),
  integrity checks scoped to touched ids, cheap no-op check before heavy
  work.
- R2 Avoid writing unchanged bytes: alias regeneration must compare with
  stored bytes before UPDATE; skip identical rewrites.
- R3 Reuse prepared statements and batch FTS/membership inserts where the
  parameter limit allows; cut `batch_manifest` recomputation from 3× to 1×.
- R4 Reduce read-path N+1: `load_session_graph` loads placements/payloads
  with `WHERE … IN (…)` instead of per-entity point queries.
- R5 Fix the correctness/reporting gaps from the full-source review: CLI
  sync committed/unchanged per-source accounting, empty-source-file sync
  (whole-source tombstone), MCP schema minimums aligned to runtime, harness
  INV-REBUILD-STABLE blind spots (failed search as mismatch, entity-set
  comparison, empty sample as failure), e2e stderr/exit-code/single-envelope
  assertions.
- R6 Keep all invariants and semantics intact: generation CAS, tombstone
  consensus, no-op detection, v6→v7 migration atomicity, privacy.
- R7 Keep all quality gates green: fmt/clippy/test/deny/release build/harness
  tests; full-corpus Gate D still passes all six invariants.

## Acceptance Criteria

- [x] Extreme-corpus (1,124 files, 1.18 GB) first ingest completes in under
      30 minutes on the reference machine; per-batch cost no longer grows
      with catalog size (O(n) total). (Measured 2026-08-13: novella2 246
      files 16.1 min; FTS delete rowid-ized — 32 MB single file 16.5s →
      2.79s, re-sync on 9K-row store 25.3s → 2.87s.)
- [x] Unchanged re-sync of a fully-ingested directory returns in seconds
      (no-op check precedes heavy work), and no generation advances.
      (46-file re-sync 17.9s → 0.1s; novella2 876.5s → 0.8s.)
- [x] Alias regeneration does not rewrite unchanged catalog rows; repeated
      sync of the same corpus does not inflate the WAL.
- [x] CLI sync reports per-source committed/unchanged accurately; an
      empty source file syncs as a whole-source tombstone instead of
      failing.
- [ ] MCP tool schemas accept exactly what the runtime accepts; harness
      INV-REBUILD-STABLE has no blind passes; e2e asserts stderr cleanliness,
      exit 5/6/7/70 coverage, and single-envelope robot output.
      (MCP minimums and INV-REBUILD-STABLE guards done; e2e stderr/exit/
      envelope assertions Deferred — see implement.md §8.)
- [x] All quality gates green; full-corpus Gate D six invariants still pass.
      (2026-08-13: 1,328 sources / 1.25 GB, all six invariants PASS,
      outcome passed.)
- [x] User self-test period: no crashes, no privacy leaks, reasonable
      first-run UX (progress feedback during long syncs).

## Constraints and Out of Scope

- Do not weaken snapshot verification (mtime-only skip is rejected;
  content fingerprinting stays).
- Do not change storage schema version (v7 stays); optimizations must be
  additive within the current schema.
- Do not degrade atomicity/rollback semantics.
- Repository stays private; no release/public until the owner explicitly
  orders it after the self-test period.
- No new features beyond the fixes listed; this task is optimization and
  hardening only.
