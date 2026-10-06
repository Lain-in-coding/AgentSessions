# Research: CI Status for PR #12

- Query: Audit PR #12 and its GitHub Actions failures; distinguish product/code failures from environment or toolchain/configuration failures and compare them with local verification claims.
- Scope: mixed (GitHub Actions status/logs plus local workflow/source/toolchain inspection)
- Date: 2026-09-27

## Findings

### Current PR state

- PR: `https://github.com/qin-devs/AgentSessions/pull/12`
- Title: `feat: preserve session identity across explicit relocation`
- Head: `5b232cdedbff33a251e7fb266558be7af8496e1a`
- Base: `main`
- State: open, mergeable, but `mergeStateStatus=UNSTABLE` because the required `ci` test matrix is failing.
- Latest CI run: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979340`
- Latest run result: 3 failed jobs and 9 passed checks.

### Exact failures

All three failures are the `Clippy` step (step 7) of the same `ci` workflow:

| Runner/job | Job URL | Failure evidence |
|---|---|---|
| `macos-latest` | `https://github.com/qin-devs/AgentSessions/actions/runs/36321979340/job/108627448119` | `error: using \`chunks_exact\` with a constant chunk size`; `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304:10`; help: `as_chunks::<4>().0.iter()`; lint `clippy::chunks_exact_to_as_chunks` implied by `-D warnings`; exit code 101 |
| `ubuntu-latest` | `https://github.com/qin-devs/AgentSessions/actions/runs/36321979340/job/108627448147` | Same diagnostic at `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304:10`; exit code 101 |
| `windows-latest` | `https://github.com/qin-devs/AgentSessions/actions/runs/36321979340/job/108627448123` | Same diagnostic at `crates\\agent-session-grep-adapters-sqlite\\src\\lib.rs:8304:10`; exit code 1 |

The failing source is:

```rust
bytes
    .chunks_exact(4)
    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
```

The GitHub logs explicitly link the new lint documentation at `https://rust-lang.github.io/rust-clippy/rust-1.98.0/index.html#chunks_exact_to_as_chunks` and report that the lint is implied by `-D warnings`. This is a real source/toolchain compatibility failure: CI cannot pass its configured quality gate with the current source. It is not a network, cache, OS, or hosted-runner failure because the same source diagnostic reproduces on macOS ARM, Ubuntu x64, and Windows MSVC.

### The failure predates the archive-only follow-up commit

The immediately previous PR run for commit `4a4b486c05a54c07633c6f28d8f46f13d6f50179` failed at the identical Clippy step on all three OSes:

- macOS: `https://github.com/qin-devs/AgentSessions/actions/runs/36321738590/job/108626775714`
- Ubuntu: `https://github.com/qin-devs/AgentSessions/actions/runs/36321738590/job/108626775860`
- Windows: `https://github.com/qin-devs/AgentSessions/actions/runs/36321738590/job/108626775884`

Therefore the `chore(task): archive session-relocation-aliases` commit did not introduce the failure. The source was already incompatible with the CI-resolved Clippy version before that commit.

### Why the local verification claim passed

Repository-local evidence differs from CI in the toolchain:

- `rust-toolchain.toml:1-4` declares `channel = "stable"`, but does not pin a version.
- `.github/workflows/ci.yml:24` installs `dtolnay/rust-toolchain` with the `stable` channel and `.github/workflows/ci.yml:46` runs `cargo clippy --workspace --all-targets -- -D warnings`.
- Local command run on 2026-09-27: `cargo clippy --workspace --all-targets --offline -- -D warnings` passed.
- Local toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)` and `clippy 0.1.97`.
- GitHub logs identify the lint documentation as Rust/Clippy `1.98.0`; the CI `stable` channel had moved beyond the local pinned toolchain.

The earlier review artifact `research/review-report.md` records the local Clippy pass, but that is not equivalent to a green remote CI quality gate. The discrepancy is explained by the unpinned `stable` channel and the new Clippy lint.

### Checks that did pass

The latest PR run passed:

- `core-beta-evidence` on Windows x64/MSVC, Ubuntu 22.04/GNU x64, macOS Intel/x64, and macOS Apple Silicon/ARM64. Run: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979300`.
- `security-audit` / Cargo dependency audit. Run: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979306`.
- `ci` installer smoke on Ubuntu, Windows, and macOS.
- `ci` cargo-deny supply-chain check.

The platform evidence jobs include release builds, direct binary smoke, SQLite adapter tests, provider evidence tests, benchmark/schema checks, and storage/source-identity spikes. Their successful job URLs are:

- Windows: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979300/job/108627447956`
- Ubuntu: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979300/job/108627448071`
- macOS Apple Silicon: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979300/job/108627448076`
- macOS Intel: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979300/job/108627448098`

The successful installer and supply-chain job URLs are:

- Installer Ubuntu: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979340/job/108627448161`
- Installer Windows: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979340/job/108627448008`
- Installer macOS: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979340/job/108627448165`
- Cargo deny: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979340/job/108627448217`
- Dependency audit: `https://github.com/qin-devs/AgentSessions/actions/runs/36321979306/job/108627447879`

### Coverage consequence

Because the generic `ci` matrix runs `Format check`, then `Clippy`, then `Test` (`.github/workflows/ci.yml:44-49`), the three failed jobs did not reach the subsequent generic workspace test, Web client, semantic-candle, CLI robot, and Python helper steps. Those checks were executed locally according to `research/review-report.md`, and several overlapping release/platform/evidence tests passed remotely, but the generic CI test matrix is not green and its post-Clippy steps are not remotely proven for this PR head.

## Recommended correction

Treat this as a P1 release/merge blocker for the current branch, but a small source/toolchain compatibility fix rather than an infrastructure incident:

1. Replace the constant-size `.chunks_exact(4)` implementation with an equivalent Clippy-1.98-compatible `as_chunks::<4>()` path, preserving the documented trailing-partial-float behavior, and add/retain a regression test for partial trailing bytes.
2. Re-run local fmt, Clippy, workspace tests, semantic-candle, CLI/Web/Python suites, then re-run all PR checks.
3. Decide whether CI should pin an explicit Rust toolchain version or intentionally track `stable`. If tracking `stable`, treat new Clippy lints as expected compatibility work; if reproducibility is preferred, pin the version in both `rust-toolchain.toml` and CI and schedule controlled toolchain upgrades.

Do not “fix” the current red check by merely allowing the lint or removing `-D warnings`; that would hide the toolchain/source drift rather than resolve it.

## Files found

- `.github/workflows/ci.yml` — generic multi-OS quality gate; installs unpinned `stable` and runs Clippy before tests.
- `.github/workflows/core-beta-evidence.yml` — cross-platform release/evidence builds and targeted SQLite/provider/storage checks; all four jobs passed for the latest PR head.
- `rust-toolchain.toml` — repository toolchain declaration; channel is `stable` with no version pin.
- `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8298-8307` — `bytes_to_f32_vec`, including the Clippy-triggering `chunks_exact(4)` call.
- `.trellis/tasks/09-25-comprehensive-project-review/research/review-report.md` — prior local quality-gate claims and review evidence; records local Clippy pass but predates the remote CI failure.
- `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md` — SQLite package contracts and quality gate expectations.
- `.trellis/spec/guides/index.md` — review guidance requiring executable evidence and explicit verification limits.

## Code patterns

- `.github/workflows/ci.yml:24` uses the moving `stable` Rust channel.
- `.github/workflows/ci.yml:46` enforces `cargo clippy --workspace --all-targets -- -D warnings`.
- `.github/workflows/ci.yml:49` starts generic workspace tests only after Clippy succeeds.
- `rust-toolchain.toml:2` repeats `channel = "stable"` without a version.
- `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304` uses `.chunks_exact(4)`, rejected by Clippy 1.98's `chunks_exact_to_as_chunks` lint.

## External references

- GitHub Actions run and job logs listed above; the logs are the primary execution evidence for the exact failure.
- Clippy lint documentation cited directly by the CI diagnostic: `https://rust-lang.github.io/rust-clippy/rust-1.98.0/index.html#chunks_exact_to_as_chunks`.

## Related specs

- `.trellis/spec/agentsessions-adapters-sqlite/backend/index.md` and its database/quality guidance: quality gates include fmt, Clippy, and workspace tests.
- `.trellis/spec/guides/index.md`: executable evidence and explicit limitations are required; a local pass must not be presented as remote CI proof.

## Caveats / Not Found

- No product source was modified and no Git operation was performed for this audit.
- The CI logs do not expose a separate `rustc --version` line in the retained failure excerpt; the Clippy diagnostic itself identifies the Rust 1.98.0 lint page, while the local toolchain was directly observed as 1.97.1.
- The passed `core-beta-evidence` workflow materially overlaps the generic tests but is not a substitute for the generic `ci` matrix; its own workflow documents that it is evidence, not release certification.
- This audit confirms the current CI blocker; it does not establish that all possible product defects are absent.
