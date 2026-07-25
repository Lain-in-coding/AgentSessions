# Core Beta Evidence Closure Design

## Scope

This task closes evidence gaps for the implemented 0.1 Core Alpha and 0.2 Cross-platform Beta scope. It may add benchmark/evidence harnesses, storage process tests, CI evidence jobs, and truthful reports. It does not add 0.3 product features or infer governance acceptance.

## Evidence model

Every conclusion uses one of these statuses:

- `locally_verified`: executed in this task with captured environment and results.
- `ci_configured_only`: workflow exists, but no run artifact or URL is available.
- `externally_blocked`: infrastructure, target host, signing identity, or credentials are unavailable.
- `not_implemented`: a spike proves feasibility but no production API exists.

Smoke thresholds and local numbers are evidence anchors, not formal SLOs or release certification.

## Work packages

### 1. Repeatable benchmark harness

Add a cross-platform, Python standard-library-only harness that:

- builds the release CLI with `cargo build --locked --release` or records a caller-supplied binary with an explicit provenance caveat;
- creates a deterministic synthetic Claude JSONL corpus;
- measures startup cold/warm samples, initial sync, no-op sync, search, show/get, storage size, artifact size, peak working set/RSS, and recovery status;
- computes P50/P95/P99, mean, and sample standard deviation;
- emits authoritative machine-readable JSON plus a Markdown review projection with all fields required by `SLI-AND-BENCHMARK-FORMAT.md`;
- validates the full commit SHA, binary/dataset hashes, sample counts, size consistency, RSS summaries, and binary provenance;
- never reads real provider transcripts.

Windows uses the process peak-working-set API. Linux reads `/proc/<pid>/status` and macOS uses `ps` at a 100ms interval; the report records that short-lived non-Windows peaks may be missed. Recovery duration remains `not_implemented` until a production fault-injection command exists and is never inferred from a clean open.

### 2. Production storage process evidence

Add an integration-test helper binary in the SQLite adapter crate plus process-level integration tests covering:

- two processes contending through production `SqliteStore::open_for_write`;
- forced holder termination followed by immediate reacquisition;
- a process exiting after durable intent creation, followed by automatic `open_for_write` recovery;
- recovery preserving generation/catalog state and being idempotent.

The helper is non-user-facing and exists only for integration evidence.

### 3. Deterministic source/CAS evidence

Strengthen tests where feasible:

- fingerprint detects equal-length replacement even when mtime is restored;
- competing stale generation intent fails closed without applying stale data;
- existing WAL spike exits nonzero if any assertion fails.

WAL snapshot remains feasibility evidence until a production snapshot/bundle API exists.

### 4. Platform evidence

- Windows x64: execute workspace gates, benchmark harness, production process tests, and storage spikes locally.
- Linux x64: use the available Ubuntu 22.04 WSL2 environment to install a user-local Rust toolchain, then execute build/test/smoke evidence. Classify it as WSL2 Linux runtime evidence, not release-host certification and not glibc 2.31 certification.
- macOS x64/ARM64 and Linux musl/glibc 2.31 release targets: retain `ci_configured_only` or `externally_blocked` unless real run artifacts become available.
- Signing/notarization remain externally blocked.

### 5. CI and reports

Extend CI so host-OS workspace tests and selected storage spikes are actually configured on Windows/Linux/macOS, and upload evidence logs where appropriate. Since this repository has no remote/run context, the workflow remains `ci_configured_only` until a real run URL and artifact exist.

Produce a single Core Beta evidence report mapping every requested item and milestone claim to an evidence ID/status.

## Compatibility and safety

- No database schema or public CLI contract changes.
- Test helper commands are isolated to a test-only binary.
- Benchmark datasets are deterministic and synthetic.
- No real transcript, credential, personal path, or external publication is used.
- No commit or push is performed.

## Validation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- production storage process integration tests
- storage spike tests/runs
- benchmark harness schema/content validation
- Linux WSL build/test/smoke commands
- `cargo deny check`
- `git diff --check`
- independent Trellis review
