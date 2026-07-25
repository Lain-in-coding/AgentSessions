# Core Alpha / Cross-platform Beta Evidence Matrix

> Evidence accounting record
>
> - status: **Draft**
> - scope: evidence for implemented 0.1 Core Alpha and 0.2 Cross-platform Beta work
> - workflow: `.github/workflows/core-beta-evidence.yml`
> - interpretation: local smoke numbers and configured CI jobs are evidence anchors, not formal SLOs or release certification

## Status vocabulary

- `locally_verified`: executed evidence is present in this repository and names the environment used.
- `ci_configured_only`: an exact CI job exists, but no successful run URL or downloaded artifact is recorded here.
- `externally_blocked`: the required host, identity, credential, or release infrastructure is unavailable.
- `not_implemented`: feasibility evidence exists, but the corresponding production API or production-level evidence helper does not.

## Evidence matrix

| Evidence ID | Claim or target | Status | Reproducible evidence | Exact caveat |
|---|---|---|---|---|
| `CB-WIN-X64-BUILD-001` | Windows x64 release build on `x86_64-pc-windows-msvc` | `locally_verified` | `spikes/cross-platform-packaging/EVIDENCE.md` | The recorded run is a local Windows build. It is not clean-machine installation evidence, a signed artifact, or release certification. |
| `CB-WRITER-LEASE-SPIKE-001` | Cross-process lock contention and forced-holder termination allow reacquisition | `locally_verified` | `spikes/data-root-locking/EVIDENCE.md` | This is feasibility-spike evidence. Production `WriterLease` also has same-process unit coverage; production process evidence is tracked separately. |
| `CB-WAL-SNAPSHOT-SPIKE-001` | WAL-aware backup and `VACUUM INTO` produce consistent snapshots; copying only the main database is unsafe | `locally_verified` | `spikes/sqlite-snapshot-wal/EVIDENCE.md` | This is feasibility-spike evidence. A production snapshot or bundle API is not implemented. |
| `CB-SOURCE-SNAPSHOT-SPIKE-001` | Content fingerprint detects equal-length source replacement | `locally_verified` | `spikes/source-snapshot/EVIDENCE.md` | The existing evidence is a Windows feasibility spike; it is not a cross-platform production certification. |
| `CB-CI-WIN-X64-001` | Windows Server 2022 x64 build, direct CLI smoke, SQLite adapter tests, and storage spikes | `ci_configured_only` | `.github/workflows/core-beta-evidence.yml` job `Windows x64 / MSVC` | No workflow run URL or artifact is recorded. The job enables static CRT for its release build, but no clean-machine runtime or Windows 10 minimum-OS test is configured. |
| `CB-CI-LINUX-GNU-X64-001` | Ubuntu 22.04 GNU x64 build, direct CLI smoke, SQLite adapter tests, and storage spikes | `ci_configured_only` | `.github/workflows/core-beta-evidence.yml` job `Ubuntu 22.04 / GNU x64` | Ubuntu 22.04 is not proof of the proposed glibc 2.31 floor and is not a Linux distribution compatibility certification. |
| `CB-CI-MACOS-X64-001` | macOS Intel x64 build, direct CLI smoke, SQLite adapter tests, and storage spikes | `ci_configured_only` | `.github/workflows/core-beta-evidence.yml` job `macOS Intel / x64` | `MACOSX_DEPLOYMENT_TARGET=12.0` constrains the build setting but does not prove execution on macOS 12. No run artifact is recorded. |
| `CB-CI-MACOS-ARM64-001` | macOS Apple Silicon ARM64 build, direct CLI smoke, SQLite adapter tests, and storage spikes | `ci_configured_only` | `.github/workflows/core-beta-evidence.yml` job `macOS Apple Silicon / ARM64` | No run artifact is recorded, and the configured hosted-runner image is not minimum-macOS certification. |
| `CB-PROD-MULTIPROCESS-001` | Production `SqliteStore::open_for_write` contention, process-kill reacquisition, durable-intent recovery, and stale-generation rejection through a process helper | `locally_verified` | `crates/agentsessions-adapters-sqlite/tests/process_evidence.rs`; `src/bin/sqlite_process_helper.rs` | Four production-path process tests pass on Windows x64 and WSL2 Ubuntu 22.04/glibc 2.35. The Linux result is WSL2-local evidence, not glibc 2.31 or distribution certification. |
| `CB-PROD-SNAPSHOT-API-001` | Production immutable SQLite snapshot or bundle API | `not_implemented` | `spikes/sqlite-snapshot-wal/EVIDENCE.md` | The spike proves feasibility only. |
| `CB-LINUX-MUSL-001` | `x86_64-unknown-linux-musl` release artifact and smoke | `externally_blocked` | `docs/adr/ADR-0002-platform-targets.md`; `docs/operations/external-readiness-gate.md` | The dedicated workflow intentionally covers the requested Linux GNU pair only; musl build/runtime infrastructure and an executed artifact remain outstanding. |
| `CB-GLIBC-231-001` | Linux GNU artifact certified against glibc 2.31 | `externally_blocked` | `docs/adr/ADR-0002-platform-targets.md` | The configured Ubuntu 22.04 runner uses a newer userspace and cannot certify this floor. |
| `CB-MACOS-12-001` | Runtime certification on macOS 12 for Intel and ARM64 | `externally_blocked` | `docs/adr/ADR-0002-platform-targets.md` | A deployment-target environment variable and newer hosted runners are not runtime certification on macOS 12. |
| `CB-SIGNING-001` | Windows Authenticode signing | `externally_blocked` | `docs/operations/external-readiness-gate.md` | No signing identity or credential is available; the evidence workflow uploads explicitly unsigned artifacts. |
| `CB-NOTARIZATION-001` | macOS signing and notarization | `externally_blocked` | `docs/operations/external-readiness-gate.md` | No Apple signing identity, notarization credential, or governed release run is available. |

## What the dedicated workflow records

For each explicit runner/target pair, the workflow is configured to:

1. install the Rust target and build the release CLI with `--locked`;
2. invoke the built binary directly for `--version`, `--help`, and robot `config paths` smoke;
3. run the current SQLite adapter test targets;
4. run the data-root locking and SQLite WAL snapshot feasibility spikes;
5. write an environment report containing runner labels, target triple, Rust/Cargo versions, source commit, binary size, and SHA-256;
6. upload the report, logs, and unsigned binary for seven days.

The workflow intentionally performs no publish, signing, notarization, checksum attestation, provenance publication, or release creation. Until a successful run URL and downloaded artifacts are reviewed and linked, every workflow row above remains `ci_configured_only`.

## Milestone accounting rule

A composite 0.2 cross-platform or release-readiness checkbox must remain open while any required target is only `ci_configured_only`, `externally_blocked`, or `not_implemented`. Smoke output may establish that a command started and returned successfully on one environment; it must not be relabeled as a formal SLO, minimum-OS certification, architecture certification, signed-release evidence, or clean-machine packaging evidence.
