# Implementation plan — Performance optimization and pre-release hardening

## 1. Batch-scoped alias regeneration (P0)

- [x] Compute touched-source set from `prepared_sources`; regenerate aliases
      only for entities whose claimers intersect the batch sources.
- [x] Compare rebuilt payload bytes with stored bytes; UPDATE only on
      difference.
- [x] Tests: multi-batch first ingest touches each entity once; unchanged
      re-sync performs zero alias UPDATEs (WAL does not grow).
      (`alias_regeneration_is_scoped_to_batch_sources_and_skips_unchanged_rows`)

## 2. Single filtered relation-state load (P0)

- [x] Superseded by the fingerprint skip + early no-op check: unchanged
      re-syncs never reach the heavy path (no relation-state load at all),
      which is strictly better than "load once". The heavy path keeps its
      per-batch loads; measured impact of deduplicating them was ~1% of
      first ingest, so the remaining duplication is left as-is.

## 3. Cheap no-op check first (P0)

- [x] Run the batch-current check (entries + source membership point
      queries) before merge/claim/manifest work; return `Ok(false)` early
      when current.
- [x] Tests: unchanged re-sync returns in milliseconds and does not advance
      generation; changed batch still commits.
      (`sources_are_current` + `sync_commits_then_reports_unchanged_on_resync`)

## 4. Batch-scoped integrity verification (P1)

- [x] Scope the three integrity scans to the batch's touched placement/edge
      ids.
- [x] Tests: injected dangling placement inside batch fails; outside batch
      is untouched.

## 5. Prepared statements and batched inserts (P1)

- [x] Hoist `tx.prepare` out of per-entity loops (catalog/FTS/fts_ids
      statements). Multi-row VALUES for FTS/membership was not pursued —
      prepared-statement reuse already removes the dominant constant cost
      and the parameter-limit batching adds complexity.
- [x] Tests: batch commit produces identical catalog/FTS bytes (full
      workspace suites green).

## 6. Single manifest computation (P2)

- [x] Evaluated and skipped: the third computation in `verify_pending_in_tx`
      is the deliberate "durable intent matches current input" semantic
      check and must stay; deduplicating the first two saves ~1-2 s per
      batch on 200K payloads (~1% of first ingest). Not worth the signature
      churn for the acceptance target.

## 7. Read-path N+1 elimination (P2)

- [x] `load_session_graph`: single `WHERE wire_id IN (…)` for fts_ids,
      single `WHERE id IN (…)` for payloads (new `stable_id_from_wire`
      mirrors `stable_id_from_store` against the batched map).
- [x] Tests: context output byte-identical on a multi-hundred-message
      session (full workspace suites green).

## 8. Correctness / reporting fixes (P1)

- [x] CLI sync per-source committed/unchanged accounting (fingerprint-skip
      sources report their stored message counts as unchanged).
- [x] Empty source file syncs as legal empty batch (whole-source tombstone).
- [x] MCP schema minimums aligned to runtime (`max_bytes` 4096, `max_items`
      1, `limit` 1).
- [x] Harness INV-REBUILD-STABLE: failed search = mismatch; entity-set
      comparison; empty sample = fail.
- [ ] e2e: stderr-empty assertions; exit 5/6/7/70 coverage; single-envelope
      robot output. (Deferred — covered by unit-level protocol tests;
      full e2e hardening is a follow-up task.)

## 9. First-run UX

- [x] Human-mode sync progress frames verified meaningful (per-source
      counters); robot stays envelope-only.

## 10. Validation gates

Run in order; stop to fix root causes on any failure:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo build --locked --release -p agentsessions-cli
python -m unittest scripts/evidence/test_real_data_regression.py
```

Then measure before/after on the reference corpus (novella2 directory and
the full 1,124-file corpus): first ingest target < 30 min; unchanged
re-sync < 5 s. Then re-run the full-corpus Gate D (six invariants must
still pass).

**Measured (2026-08-12, reference machine)**:

| Scenario | Before | After | Target |
|---|---|---|---|
| 46-file re-sync | 17.9 s | **0.1 s** | < 5 s ✅ |
| novella2 (246 files) re-sync | 876.5 s | **0.8 s** | < 5 s ✅ |
| novella2 first ingest | ~10+ min | 966.8 s (16.1 min) | < 30 min ✅ |
| 46-file first ingest | ~19-25 s | 25.5 s | n/a (no regression) |

Full-corpus Gate D re-run with the optimized binary is in progress
(2026-08-12); the fingerprint skip makes the sync phase of a re-run
near-instant, with context/rebuild stages dominating.

## 11.5. FTS delete rowid-ization (P0, added 2026-08-12)

- [x] Root cause: `DELETE FROM fts WHERE id = ?1 OR id = (SELECT id_json FROM
      fts_ids WHERE wire_id = ?2)` (lib.rs:2908) — the fts5 `id` column is
      content, not rowid, so every delete scans the whole virtual table.
      Batch cost is O(N²); full-corpus first ingest is ~7.5 h at 35万
      messages (EXPLAIN QUERY PLAN shows `SCAN fts VIRTUAL TABLE INDEX 0`;
      measured 15µs at 100 rows → 6,283µs at 10K rows; same-file control
      0.6s → 6.9s on a 9K-row store).
- [x] Fix: address deletes by fts rowid (deterministic rowid on insert, or
      keep the rowid in the fts_ids sidecar). Measured 8µs/delete at 20K
      rows (360× faster); full-corpus first ingest returns to ~15 min.
      Must stay compatible with `rebuild_index` (full clear + reprojection)
      and generation CAS semantics.
      (Implemented: `fts_ids.fts_rowid` sidecar column — additive within v7,
      `user_version` unchanged; all three delete paths — batch commit,
      `rebuild_index` reprojection, single-entity `SearchIndex::index` —
      record the fts5 rowid on insert and delete by it; legacy v7 DBs are
      backfilled on open by joining `fts.id` (= `id_json`) to `fts_ids`.)
- [x] Tests: unchanged re-sync still byte-identical; rebuild fidelity
      suites stay green; full workspace gates pass.
      (Regression tests added: `fts_rowids_track_rows_across_commit_rebuild_and_delete`,
      `large_store_delete_is_rowid_scoped_not_content_scanned` (10K-row
      delete < 3 s guard), `v7_open_backfills_fts_rowid_for_legacy_rows`.)

### 11.5.1. Measured (2026-08-12, 32 MB / 16,515-line real file,
reference machine, release binary)

| Scenario | Before | After |
|---|---|---|
| Empty-db first sync of the file | ~16.5 s | **2.79 s** |
| Re-sync modified copy on a ~9K-row fts store | 25.3 s / 6.9 s | **2.87 s** (≈ empty-db) |
| Unchanged re-sync (fingerprint skip) | — | **0.089 s**, generation unchanged |
| `index rebuild` (8,990 entities) | — | **0.71 s** |

### 11.5.2. Harness source_changed tolerance (added 2026-08-12)

- [x] Real corpus sync hits `source_changed` (exit 5) when a live session is
      appending to a transcript; the snapshot check is deliberately strict
      (prd: no weakening), so the harness now retries a changed chunk up to
      3 times with 3 s backoff (`SYNC_CHANGED_RETRIES` /
      `SYNC_CHANGED_BACKOFF_S` in `real_data_regression.py`). Persistent
      changes still fail the run verbatim; other errors are never retried.
- [x] Unit tests: `python -m unittest scripts/evidence/test_real_data_regression.py`
      → 12/12 OK.

### 11.5.3. Cross-source text-merge exemption (added 2026-08-13, owner decision)

- [x] Root cause: full-corpus Gate D failed with `catalog_error: message has
      conflicting projections across sources` at ~148K emitted records. Line
      bisection + per-source enumeration over 1,328 sources found exactly 5
      conflicting sources, all in `C--Users--Q`: `4fad0a5d`, `927fa4e1`,
      `dba565c0`, `de798dc2`, `e42572a3`. All 5 conflict on the SAME message
      uuid `d758f689`: the main source (`245d6a3a`) carries a truncated
      tool_result (single "NOT FOUND" block) while the 5 copies carry the
      full content (additional backup-listing block). Claude Code copies
      history into resumed/forked transcripts, so a copied message may carry
      more content blocks than the original — text legitimately differs
      across sources.
- [x] Fix: `text` is exempt from conflict authority in
      `merge_message_payloads` (same treatment as sessions/spans/parent/
      timestamp/seq); the merged value deterministically keeps the LONGER
      projection so no retrieved content is lost. Test
      `message_with_genuinely_different_content_still_conflicts` updated to
      `message_with_different_text_merges_to_longer_projection`.
- [x] Gates: fmt/clippy/test all green; release binary rebuilt 2026-08-13
      07:38; full-corpus Gate D re-run in progress.

### 11.5.4. Full-corpus Gate D GREEN (2026-08-13 07:51)

- [x] All six invariants PASS on 1,328 sources / 1,253,494,481 bytes:
      INV-SYNC-OK exit 0, 180,218 emitted, 0 skipped; INV-NO-PARSE-LOSS
      180,218 claims = emitted; INV-SESSION-PRESENT 242 sessions;
      INV-CONTEXT-NONEMPTY 242 sessions 0 failed 11 zero-placement;
      INV-SPAN-COVERAGE 630/630 byte; INV-REBUILD-STABLE 166,380 ->
      166,380 ids match. Outcome: passed. Acceptance criteria 1 and 6
      are closed.

## 11. Review and rollback points

- Gate A: batch-scoped regeneration + single load + no-op-first (1-3) with
  before/after timing — the O(n²)→O(n) core.
- Gate B: integrity scoping + prepared statements (4-5).
- Gate C: correctness/reporting fixes (8) + UX (9) before harness changes.
- Gate D: all code gates + full-corpus Gate D re-run.
- Rollback: revert individual commits; no schema change; semantics
  preserved.

No commit or push without explicit owner authorization; repository stays
private; no release/public until the owner orders it after self-test.
