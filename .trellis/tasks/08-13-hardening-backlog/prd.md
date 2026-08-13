# Hardening backlog (perf P1, MCP contract, evidence scripts)

## Goal

Owns the 2026-08-13 review findings that are real but outside the
ux-review-fixes scope. Starts only after `08-13-ux-review-fixes` lands
(they share the same five production files; sequencing avoids edit
conflicts).

## Requirements

### R1 Performance P1s (reproduced, pre-existing)

- R1.1 changed sync: bound relation reads to the touched batch (target
  O(batch), not full-catalog loads inside the write transaction);
  keep the current touched-only write optimization.
- R1.2 context: enforce read/work budget before full graph load —
  small `max_messages`/`max_bytes` must shrink backend reads and peak
  memory, not only the response.
- R1.3 read-open migration: a nominal read path must not run DDL /
  full-table `fts_rowid` backfill while holding no writer lease; either
  gate schema upgrades behind the writer lease or make them explicit.
- R1.4 deep pagination: replace offset-rescan with keyset/cursor-based
  fetch, and remove or document the 1<<20 reachability cap.
- R1.5 residual N+1: per-edge parent-identity lookups, per-source sync
  metadata lookups — batch them.

### R2 MCP contract gaps (pre-existing)

- R2.1 initialize failure must not open the gate (failed initialize →
  initialized notification → tools/list currently passes).
- R2.2 validate `jsonrpc == "2.0"` on every request; respond with the
  proper error, not a 2.0-masquerading reply.
- R2.3 protocol e2e covering both, plus the missing combined sequences.

### R3 Evidence / install scripts

- R3.1 `real_data_regression.py`: retry only `error.code ==
  "source_changed"`; other exit-5 codes must fail the gate, never be
  swallowed into `outcome: passed`.
- R3.2 Windows installer: detect the pre-rename default
  `%LOCALAPPDATA%\AgentSessions\bin` install, migrate/clean it (or
  document an explicit migration step); uninstaller covers the old
  prefix.
- R3.3 `core_beta_benchmark.py`: stop rewriting the historical v1 schema
  id in place — accept the legacy id or introduce a real v2 with
  migration; restore the historical evidence file's label.

## Acceptance Criteria

- [ ] R1: query-count/scaling tests demonstrate the claimed bounds;
      budget tests prove backend reads shrink; legacy-v7 first-open has
      no unleased writes.
- [ ] R2: gate and jsonrpc e2e pass.
- [ ] R3: real_data_regression fake-CLI test shows a non-source_changed
      exit 5 fails the gate; installer migration path tested; legacy
      report validates again.
- [ ] All quality gates green; Gate D re-run green.

## Constraints

- Sequencing: starts only after `08-13-ux-review-fixes` is archived.
- No new production features; repository stays private.
