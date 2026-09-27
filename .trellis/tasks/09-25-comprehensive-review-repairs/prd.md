# Implement Comprehensive Review Repairs

## Goal

Repair all actionable P1-P3 defects and approved optimization opportunities identified by the 2026-09-25 comprehensive review, while preserving existing public contracts except where the approved plan explicitly requires a versioned failure or additive migration.

## Requirements

1. Fix the nine P1 defects: SQLite WAL logical snapshots, multi-session SQLite sources, semantic/hybrid filtering, MCP semantic wiring, provider filter coverage, injective native IDs, Unicode-safe redaction, strict time parsing, and read-only SQLite lifecycle.
2. Fix the approved P2/P3 defects: cursor state binding, semantic pagination/readiness/non-finite scores, rank fusion, no-op validation, provider SQL error propagation, calendar validation, relocation-safe identity migration boundary, integer casts, hook budget units, and Web API subset gaps.
3. Close embedded Web UI gaps confirmed during implementation: expose supported search filters, invalidate pagination state when search inputs change, and prevent stale asynchronous search/context responses from replacing newer user-visible state. Preserve offline single-file deployment and safe output projection.
4. Implement approved performance and dependency work after correctness: bounded embedding rebuild, bounded session metadata SQL, measured semantic scan improvements, and dependency upgrades that remove allowed RustSec warnings.
5. Preserve fail-closed behavior, privacy boundaries, source read-only guarantees, Robot/MCP error semantics, and synthetic-only test fixtures.
6. Synchronize user-facing contracts, schemas, CLI help, specs, and changelog for every behavior change.

## Acceptance Criteria

- [x] All findings in `.trellis/tasks/09-25-comprehensive-project-review/research/review-report.md` are repaired, explicitly deferred to a registered follow-up task, or shown to be a false positive with evidence.
- [x] The report's runtime reproductions become regression tests and kill targeted mutations.
- [x] CLI, Robot, MCP, and Web produce consistent canonical search behavior for provider filters, semantic modes, filters/facets, cursors, budgets, and errors.
- [x] Web UI exposes supported API search controls and remains correct under rapid filter/search/session changes, with accessible loading and error feedback.
- [x] SQLite OpenCode/Cursor ingestion observes WAL logical state and preserves distinct sessions and resume claims.
- [x] Read paths no longer create, migrate, or mutate a SQLite database without a writer lease.
- [x] Workspace format, lint, debug/release tests, semantic-candle tests, Python suites, cargo-deny, cargo-audit, and diff checks pass.
- [x] No real transcript, credential, private path, or provider source bytes enter the repository.

## Constraints

- No silent history rewrites: existing `ses_v1` IDs remain unchanged; relocation identity work must use an additive alias/migration design.
- Old cursor tokens affected by retrieval/model/ranking state must fail explicitly.
- Product source files may be modified only under the active repair task.
- Do not push, merge, or publish without a later explicit request.
