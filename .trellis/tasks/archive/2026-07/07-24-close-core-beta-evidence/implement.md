# Core Beta Evidence Closure Implementation Plan

## 1. Freeze evidence contracts

- [x] Clarify the benchmark report contract without changing the Draft governance status.
- [x] Use explicit evidence classes and keep smoke, local verification, CI configuration, target certification, and external blockers separate.
- [x] Preserve raw samples and deterministic synthetic dataset metadata.

## 2. Implement storage correctness evidence

- [x] Add process-level integration evidence through production `SqliteStore::open_for_write`.
- [x] Prove writer contention, forced-holder termination/reacquisition, durable-intent recovery, recovery invariants, idempotence, and stale-generation failure.
- [x] Make equal-length source replacement deterministically exercise fingerprint verification even when mtime is restored.
- [x] Make the WAL snapshot spike fail with a nonzero process status when an assertion fails.

Validation checkpoint:

```text
cargo test -p agentsessions-adapters-sqlite --all-targets
cargo run --release --manifest-path spikes/sqlite-snapshot-wal/Cargo.toml
```

## 3. Implement repeatable local benchmark evidence

- [x] Add a cross-platform, standard-library-only Python harness under `scripts/evidence/`.
- [x] Generate deterministic synthetic provider fixtures; never read real transcripts.
- [x] Build or accept an explicit release CLI binary and measure startup, sync workloads, search, show/get, storage, artifact size, peak RSS, and recovery evidence.
- [x] Emit authoritative JSON with raw samples, nearest-rank P50/P95/P99, mean, sample standard deviation, environment, dataset hash, binary hash, and limitations.
- [x] Add a reduced smoke mode and report validation mode for CI.
- [x] Run a full local Windows evidence capture satisfying documented sample counts.

Validation checkpoint:

```text
python scripts/evidence/core_beta_benchmark.py --help
python scripts/evidence/core_beta_benchmark.py run --profile smoke ...
python scripts/evidence/core_beta_benchmark.py validate-report <report>
```

## 4. Configure exact platform evidence

- [x] Keep the ordinary PR quality workflow intact.
- [x] Add a dedicated evidence workflow with explicit Windows x64, Linux GNU x64, macOS Intel, and macOS ARM64 runner/target pairs.
- [x] Build release binaries, execute direct smoke commands and production storage evidence, generate environment reports, and upload unsigned short-retention artifacts.
- [x] Label all unexecuted workflow entries `CI configured only`; do not claim run verification.
- [x] Record that static CRT, glibc 2.31, macOS 12, musl, signing, notarization, and clean-machine packaging require additional evidence.

## 5. Execute available platforms and spikes

- [x] Windows: full workspace gates, storage process tests, benchmark report, and relevant spikes.
- [x] WSL2 Ubuntu: install a user-local Rust toolchain if needed, execute build/test/smoke evidence, and label the result as WSL2 Ubuntu 22.04/glibc 2.35 local evidence only.
- [x] Preserve raw logs and machine-readable reports under ignored/generated evidence output; commit only the reviewed summary and reproducible harness/configuration.

## 6. Reconcile documentation and checklists

- [x] Replace the historical performance baseline with links to the new report and explicit limitations.
- [x] Add an evidence matrix mapping source snapshot, lease, WAL, CAS, recovery, multiprocess, SLI, and platform targets to exact status and artifact.
- [x] Update ADR/R0/spike summaries only with claims supported by executed evidence.
- [x] Check task acceptance criteria only when each criterion is actually supported.
- [x] Keep composite cross-platform/release certification open where required targets remain unverified.

## 7. Final quality and independent review

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
git diff --check
```

- [x] Run report schema validation and all new evidence commands.
- [x] Run an independent `trellis-check` review and fix confirmed issues.
- [x] Update executable code-specs with the durable evidence/testing contract learned in this task.
- [x] Archive this task only after all locally feasible work and truthful reconciliation are complete.
- [x] Stop for owner review; do not start the 0.3 Integration Beta task.

## Rollback points

- Benchmark/report changes are additive; remove the new harness and workflow if their schema cannot be made deterministic.
- Storage changes are tests/helper surfaces only; do not alter the schema or public CLI to satisfy evidence collection.
- CI evidence is isolated from the existing PR gate so an unstable benchmark cannot silently become a release/SLO gate.
