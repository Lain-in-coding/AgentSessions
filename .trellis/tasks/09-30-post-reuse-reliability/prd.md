# Post-reuse reliability

## Goal
Repair four reproduced correctness defects without reopening the completed post-reuse capability phase or expanding product scope.

## Background and approval
- Review baseline: `bbb7c79533c13db584fa5ca2e1e05990d990c8da` (origin/main, verified 2026-09-30).
- The six earlier phase tasks remain completed. Prior paired 1M improvement remains approximately 37.5%, not >=50%; Hermes/Cursor SQLite variants remain experimental.
- The owner selected all four fixes, partial Hermes ingestion with temporary old/new coexistence, successful no-op for a first unknown empty source, restricted atomic repair of historical empty bindings, and the three-class timestamp order.
- On 2026-09-30, after reviewing the final proposed plan, the owner explicitly instructed: `Implement the plan.` This authorizes creating these tasks and implementing that reviewed scope. No remote push, merge, visibility or billing change is inferred.

## Requirements
- R1: Changed bounded-whole sources must reach their adapter, not be misclassified as truncated JSONL.
- R2: Unemitted Hermes message rows must be accurately counted; partial scans may add observations but must not delete prior claims.
- R3: Unknown first-empty input is a warned no-op without a fake provider binding. Known empty replacement preserves provider ownership in both ingest and sync. Proven empty-only historical bindings may be repaired atomically; real or ambiguous identity remains fail-closed.
- R4: Context comparison is a total order: missing, invalid raw, valid UTC; existing structural tie-breaks remain.
- R5: Preserve public fields, schema, dependencies, MSRV, snapshot/lease/outbox/CAS protections, discovery ownership, real native identity, and existing incomplete-context restrictions.
- R6: Complete the test matrix and independent checks before claiming delivery. Old successful CI is historical evidence, not validation of new code.

## Task map
- A: `09-30-hermes-partial-source-accounting` (P1).
- B: `09-30-source-lifecycle-correctness` (P1/P2), depends on A.
- C: `09-30-context-order-determinism` (P2), independent of A/B.

## Acceptance Criteria
- [x] A/B/C satisfy their acceptance criteria with regression tests for the reproduced failures.
- [x] Current debug/release workspace, semantic feature, Rust 1.90, Node/Python and lint gates pass locally offline.
- [x] Coordinator check completed against the specs, the diff, and the executable evidence (`research/check-report.md`); the three shared specs document the repaired contracts.
- [ ] Independent Trellis check sub-agent run remains an explicit gap: every AgentRouter spawn in this window failed with HTTP 402 (quota), so no independent agent reviewed the patch.
- [x] Historical CI evidence is retained separately from new-code verification and contains no committed private runner data.
- [ ] New PR/main CI are recorded when publication/merge is separately authorized; until then this external delivery gate stays pending.

## Out of scope
Performance routes A-D; schema/dependency/MSRV/release/SLO changes; beta promotion; wider provider bounds; Cursor dual-surface policy; identity redesign or content-based dedup; other windows' cleanup/archival. Repository stays PRIVATE.
