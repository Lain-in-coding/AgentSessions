# Close 0.1 and 0.2 evidence

## Goal

Replace milestone assertions with reproducible evidence for the Core Alpha and Cross-platform Beta scope.

## Requirements

- Establish repeatable startup, sync, search, storage-size, memory, and recovery measurement procedures, including P50/P95/P99 latency, peak RSS, artifact size, and recovery duration where applicable.
- Validate formal target definitions and obtain real Windows/Linux/macOS build-and-run evidence where infrastructure is available; distinguish CI OS coverage from release target/architecture certification.
- Add source snapshot, writer lease, WAL snapshot, generation CAS, fault-injection, and multi-process evidence where locally or CI-feasible.
- Record externally blocked macOS/Linux/signing work without claiming it passed.
- Update milestone checklists only from actual evidence.

## Acceptance Criteria

- [x] Local benchmark reports satisfy the documented required fields and sample counts.
      Evidence: `docs/evidence/core-beta/88d86f4/core-beta-benchmark-full.json` (full
      profile: startup cold/warm 20 each, search/show/get 100 each, sync 3 workloads),
      self-validated by `scripts/evidence/core_beta_benchmark.py validate-report`.
- [x] Workspace and spike validation commands are reproducible.
      Evidence: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace` (154 pass), `cargo deny check` (advisories/bans/licenses/sources ok),
      `git diff --check` all green on this commit; WAL spike A/B/C/D PASS with exit 0.
- [x] Cross-platform evidence identifies verified, CI-configured-only, and externally blocked targets separately.
      Evidence: `docs/operations/core-beta-evidence-matrix.md` and
      `docs/evidence/core-beta/88d86f4/manifest.md`. Windows x64 is locally verified;
      WSL2 Ubuntu 22.04/glibc 2.35 provides local Linux release build/direct smoke/workspace-test
      evidence only and does not certify the glibc 2.31 floor or a distribution target. Formal
      Linux/macOS target rows remain `ci_configured_only`; signing/notarization and unavailable
      minimum-platform work remain `externally_blocked` / `not_implemented`.
- [x] 0.1/0.2 checklist claims match the resulting evidence.
      Evidence: `docs/performance-baseline-0.2.md` and `docs/adr/ADR-0002-platform-targets.md`
      updated to point at the evidence bundle; no composite cross-platform checkbox is
      closed while any required target remains only configured/blocked/not-implemented.
- [x] No smoke result is presented as a formal SLO or release certification.
      Evidence: every report, matrix, and manifest states results are local evidence anchors,
      not formal SLOs or release certification; `recovery` is `not_implemented` rather than
      inferred from a clean open.
