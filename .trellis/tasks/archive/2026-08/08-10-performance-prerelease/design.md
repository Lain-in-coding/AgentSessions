# Design — Performance optimization and pre-release hardening

## Problem

Each chunked sync batch pays O(whole-catalog) cost: alias regeneration
rewrites every fully-complete entity unconditionally, four relation tables
are fully loaded 2-3× per batch, three integrity scans run per batch, and
the no-op check happens only after all the heavy work. First ingest of
~10 batches is therefore O(n²) — 2.5 h on the 1.18 GB reference corpus.

## Design Principles

1. **Batch-scoped work**: everything that can be scoped to the batch's
   touched sources/entities must be. O(batch) per batch → O(n) total.
2. **No writes when bytes are unchanged**: compare before UPDATE.
3. **Cheap checks first**: no-op detection before any heavy loading.
4. **Reuse**: prepared statements, single manifest computation, single
   relation-state load per batch.
5. **No semantic change**: generation CAS, tombstone consensus, no-op
   detection, atomicity, and privacy all stay byte-for-byte equivalent.

## 1. Batch-scoped alias regeneration

`regenerate_compatibility_aliases_in_tx` currently:

- loads full `source_membership`, placement claims, placements, edges;
- for every fully-complete entity: SELECT payload → parse JSON → rebuild
  aliases → serialize → unconditional UPDATE.

**Change**:

- Compute the set of source paths touched by this batch
  (`prepared_sources`). A full re-scan of a source replaces that source's
  claims, so only entities whose claimers intersect the batch's sources can
  change. Regenerate only those entities (membership query filtered by
  `source_path IN (batch paths)`).
- After rebuilding the alias payload, compare with the stored bytes;
  UPDATE only when different.
- First ingest: each entity is touched by exactly one batch → O(n) total.

## 2. Single relation-state load, filtered

`source_batches_are_current` and `commit_source_batches_if_changed` both
fully load membership/placement/edge state; `regenerate` loads them a third
time.

**Change**: load the four state maps once per batch; pass the same maps into
the current-check, the claimer computation, and regeneration. Membership
reads use `WHERE source_path IN (batch paths)` for the batch's own sources
plus point queries (`WHERE message_id IN (…)`) for cross-source claimers
needed by tombstone/conflict consensus.

## 3. Cheap no-op check first

**Change**: before computing merges, claimer graphs, manifests, or payload
reads, run the current check against the batch's entries and its source
membership/placement rows (point queries). If everything is current, return
`Ok(false)` immediately. The heavy path runs only when something actually
changed.

## 4. Batch-scoped integrity verification

`verify_relational_integrity_in_tx` currently full-scans placements, edges,
and placement membership with NOT EXISTS subqueries.

**Change**: scope the three scans to the batch's touched placement/edge ids
(`WHERE placement_id IN (…)` / `child_placement_id IN (…)`). Invariants for
untouched rows are preserved by induction (each commit maintains them for
its own rows; deletes are only of this batch's claims).

## 5. Prepared statements and batched inserts

**Change**: hoist `tx.prepare` out of the per-entity loops (catalog upsert,
fts delete/insert, fts_ids delete/insert, membership insert, placement/edge
upsert). FTS and membership inserts use multi-row VALUES batches
(parameter-limit aware, e.g. 16K rows per statement).

## 6. Single manifest computation

`batch_manifest` is computed 3× per batch (pre-check, begin, verify).
**Change**: compute once; pass the manifest through begin; `verify` compares
the stored manifest string with the in-memory one (it validates
"declared == actual", no need to re-hash payloads).

## 7. Read-path N+1 elimination

`load_session_graph` issues per-entity payload reads and per-entity
`stable_id_from_store` point queries.
**Change**: single `WHERE wire_id IN (…)` for fts_ids and single
`WHERE id IN (…)` for payloads; assemble in memory.

## 8. Correctness / reporting fixes (from full-source review)

- **CLI sync accounting**: adapter returns per-source changed bits (or CLI
  compares each source against stored state) so `committed`/`unchanged` are
  honest per source instead of whole-batch.
- **Empty source file**: a 0-byte source syncs as a legal empty batch
  (`relation_complete: true`, 0 entries) so whole-source removal
  tombstones its entities.
- **MCP schema minimums**: align `max_bytes` (4096), `max_items` (1),
  `limit` (1) with runtime; `list_sessions limit:0` → `-32602`.
- **Harness INV-REBUILD-STABLE**: failed search (`-1`) counts as mismatch;
  compare entity id sets, not just counts; empty sample → fail.
- **e2e hardening**: stderr-empty assertions in robot/MCP helpers; exit
  5/6/7/70 coverage; single-envelope count assertions.

## 9. First-run UX

`sync` in human mode already emits progress frames; verify the frame
carries a meaningful per-source counter and that a long first ingest shows
progress without stalling. `--robot` stays envelope-only.

## Rollout / Rollback

- Each optimization lands with its own tests and a before/after timing
  check on the reference corpus; no schema change (v7 stays).
- Rollback = revert the specific commit; all changes are additive within
  the current schema and semantics.
- Full-corpus Gate D is re-run after the optimization set to prove the six
  invariants still hold at the new speed.
