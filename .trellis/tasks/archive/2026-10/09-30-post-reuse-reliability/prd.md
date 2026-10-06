# Post-reuse reliability

## Goal
Repair four reproduced correctness defects without reopening the completed post-reuse capability phase or expanding product scope.

## Background and approval
- Review baseline: `bbb7c79533c13db584fa5ca2e1e05990d990c8da` (origin/main, verified 2026-09-30).
- The six earlier phase tasks remain completed. Prior paired 1M improvement remains approximately 37.5%, not >=50%; Hermes/Cursor SQLite variants remain experimental.
- The owner selected all four fixes, partial Hermes ingestion with temporary old/new coexistence, successful no-op for a first unknown empty source, restricted atomic repair of historical empty bindings, and the three-class timestamp order.
- On 2026-09-30, after reviewing the final proposed plan, the owner explicitly instructed: `Implement the plan.` This authorized creating these tasks and implementing that reviewed scope, not remote publication at that time.
- On 2026-10-01 the owner separately authorized continuing planning and execution in full. This continuation covers task-scoped commits, branch publication, a PR and normal merge only after successful current-head checks. It does not lift the existing exclusions on visibility, billing, performance routes A-D, schema/dependencies, release/SLO contracts, or provider promotion.

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
- [x] Independent full-scope Trellis source review completed on 2026-10-01 (`research/independent-check.md`). The reviewer fixed two product defects, strengthened missing regression proof and propagated the new warning assertions; coordinator integration accepted the patch after fresh full offline gates passed. Historical failed gates remain recorded, not relabeled as success.
- [x] Historical CI evidence is retained separately from new-code verification and contains no committed private runner data.
- [x] PR #17 current head `a51a60e` passed 12/12 checks and merged normally as `124fb57e52c051b90e2f308a24487352fdb5ac5d`. The merge tree equals the tested head. Fresh main `ci` 36874887357 passed 7/7 jobs and `core-beta-evidence` 36874887468 passed 4/4 jobs, both on the actual merge SHA. Repository visibility remains PRIVATE; final receipts are in `research/check-report.md`.

## Out of scope
Performance routes A-D; schema/dependency/MSRV/release/SLO changes; beta promotion; wider provider bounds; Cursor dual-surface policy; identity redesign or content-based dedup; other windows' cleanup/archival. Repository stays PRIVATE.
