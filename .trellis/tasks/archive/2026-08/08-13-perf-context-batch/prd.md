# Perf Top 5: context batching, mainline index, rebuild join

## Goal

Land the performance review's Top 5, all low-risk and aligned with the
already-merged batching direction: batch the remaining N+1s and kill the
O(n²) hot paths in context assembly and sync.

## Requirements

### R1 load_session_graph edge batching
- R1.1 The per-edge parent-identity lookup (adapters-sqlite:4069-4078,
  one query per edge today) moves into the existing batched
  payload/fts_ids load; orphan-parent wires join the batch set.
- R1.2 Identity grade preserved (no Native→Unstable downgrade for orphans).

### R2 select_mainline parent index
- R2.1 domain/src/thread.rs: build a per-message placement index once and
  resolve parents O(1) instead of scanning all placements per hop.

### R3 Timestamp pre-parse
- R3.1 domain/src/thread.rs: parse each message timestamp once into a map;
  comparisons read the map instead of re-parsing.

### R4 rebuild_index join
- R4.1 adapters-sqlite rebuild: single join over catalog+fts_ids (or
  hoisted prepare) instead of a per-row query.

### R5 changed-sync merged IN batch
- R5.1 adapters-sqlite: merged-entity loop reads via chunked IN
  (existing chunk_ids pattern) instead of per-entity get.

## Acceptance Criteria

- [ ] Query-count tests: context assembly issues a bounded number of SQL
      statements regardless of edge count; rebuild bounded per catalog row.
- [ ] select_mainline behavior identical (mainline choice unchanged) —
      property-tested against the pre-change selection.
- [ ] No identity-grade regressions in context assembly.
- [ ] Gates: fmt / clippy -D warnings / test --workspace / build --release.
- [ ] Gate D re-run green.

## Constraints

- Behavior-preserving optimizations only — no ranking/selection semantics
  changes.
- No commit/push without owner authorization.
