# Execution
- [ ] Read curated specs and parent baseline.
- [ ] Recheck P0-4/P0-5/P0-6/P0-7/P0-8; application part P1-10; P3-1/P3-2 at 2edf2dc; add failing regression.
- [ ] Implement smallest root-cause fix in exclusive scope.
- [ ] Run package tests/Clippy and report exact commands/results.
- [ ] Independent check and shared-boundary integration.

No child commit, push or recursive agents. Coordinate shared Cargo target builds.

## 2026-10-03 bounded execution
- Current integration base: 0395394 (first remediation batch pushed; 1895 passing workspace tests).
- First slice: P0-4 filtered search ownership in SQLite and P1-10 Context port error propagation in application, with disjoint implementation ownership and serialized Cargo tests.
- P0-5/P0-6/P0-7/P0-8 are not implicitly included or completed by this slice. Migration checkpoints were refreshed in design.md before attempting source-authority changes.
- SQLite reproduced incorrect repo-filtered ownership before the fix; 12 focused filtered tests passed after the first patch. The strengthened matrix also covers lexical fallback, mixed main/sidechain MainOnly exclusion, no-placement legacy behavior, metadata representatives and grouped results. Final full integration is pending.
- Independent review identified a related existing mismatch in application current-repo boost. It now uses the selected hit owner, preserving None fallback and existing ranking weights. Red tests proved owner mismatch and pre-fix cursor acceptance; all three owner/cursor tests then passed. The search digest revision is now `search-v2-rrf60-signals-v3-matched-owner`; old search cursors are rejected, not silently resumed. List cursors are unchanged.
- Application all-feature suite: 313 passed; scoped all-feature/all-target Clippy passed. Main workspace fmt/Clippy passed and full tests/MSRV are running. Source-authority migration remains design-only.
- Context counterexample was executed with the original `unwrap_or_default`: 2 focused tests passed and `context_activity_read_errors_propagate` failed because it received successful empty activity output. Restoring `?` made all 3 pass; usage errors already propagated and legitimate absent projections remain successful empty output.
- Final independent static review passed the bounded slice, including matched-owner scoring, old-cursor rejection, fallback and MainOnly compatibility. The reviewer did not run Cargo; full integration evidence belongs to main's gate below.
- Main final gate passed: 1904 workspace tests, 0 failures, 22 ignored; fmt, all-target/all-feature Clippy and Rust 1.90 workspace check passed (offline/locked). Python 21/11/54 tests (two skips in total), Node 11, privacy scan and real synthetic debug smoke 10/10 passed. No schema migration or source-projection implementation in this checkpoint.
