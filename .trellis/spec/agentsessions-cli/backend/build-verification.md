# Build verification and CI cache contract

## Scenario: Local feedback and CI build verification

### 1. Scope / Trigger
Changing local Rust verification commands, Cargo job concurrency, CI cache
identity, Rust toolchain selection, or optional build-cache tooling. The contract
prevents duplicate target-directory fan-out, stale cache keys and unsafe cache
cleanup from becoming the default workflow.

### 2. Signatures
- `pwsh scripts/dev/verify.ps1 -Package <cargo-package> [-Jobs 1..4]`
- `pwsh scripts/dev/verify.ps1 [-Jobs 1..4]`
- CI jobs: `test`, `semantic-candle`, `installer`, `deny`.
- Environment: `CARGO_BUILD_JOBS`, `CARGO_INCREMENTAL`.

### 3. Contracts
- Package mode runs exactly
  `cargo check -p <package> --all-targets --locked` with `CARGO_BUILD_JOBS`
  defaulting to 4.
- Full mode runs, in order and stopping at the first failure:
  `cargo fmt --all --check`;
  `cargo clippy --workspace --all-targets --locked -- -D warnings`;
  `cargo test --workspace --locked`.
- The script resolves the repository root from its own path, restores the prior
  working directory and `CARGO_BUILD_JOBS`, and never changes global Cargo
  configuration.
- Run one full gate at a time. Independent agent work may use at most three
  concurrent Cargo processes with `-j4` each.
- CI sets `CARGO_INCREMENTAL=0`, explicitly installs Rust `1.97.1`, and binds
  each target cache key to `Cargo.lock`, `rust-toolchain.toml`, and
  `.cargo/config.toml`.
- Semantic-candle tests and entrypoint checks run in a separate three-OS matrix
  job so optional-runtime coverage does not serialize the primary test job.
- MBX and sccache are opt-in experiments, never defaults. On Windows NTFS, MBX
  copies restored output and can multiply physical footprint; it is limited to
  bounded multi-worktree bursts with an isolated store.
- Broad automatic cleanup of cache stores is forbidden. A cleanup must identify
  exact paths, prove no active build or source/evidence dependency, use a
  recoverable disposal mechanism, and stop if the OS cannot prove recovery.

### 4. Validation & Error Matrix
- `-Jobs` outside `1..4` -> parameter validation failure before Cargo runs.
- `-Package` supplied but blank -> explicit error before Cargo runs.
- Repository root cannot be found -> explicit error before Cargo runs.
- Any Cargo command returns nonzero -> script throws that command and exit code.
- CI cache key omits lockfile/toolchain/config identity -> invalid workflow.
- Semantic-candle remains inside the main matrix -> invalid workflow regression.
- Recycle source disappears without matching Recycle Bin metadata -> hard stop;
  do not continue cleanup or claim recoverability.

### 5. Good/Base/Bad Cases
- Good: edit one crate, run package mode; when the edit set is ready, run one
  serial full gate.
- Base: CI restores a cache only when lockfile, toolchain and Cargo profile
  identity match.
- Bad: launch several full workspace gates in parallel, cache only the lockfile,
  or use MBX as the default single-checkout build.

### 6. Tests Required
- Parse `verify.ps1` under PowerShell 7 and Windows PowerShell 5.1.
- Run one targeted package check and one complete full gate.
- Assert `-Jobs 5` is rejected and both the working directory and
  `CARGO_BUILD_JOBS` are restored.
- Parse CI YAML and assert four jobs, the semantic-candle matrix, incremental
  disablement, pinned Rust, three cache inputs, retained gates and 40-character
  action SHAs.
- Validate MBX evidence and cleanup receipts against their raw runtime records.
- `git diff --check` must pass and no unrelated file may be staged.

### 7. Wrong vs Correct
#### Wrong
```text
Run `cargo test`, `cargo clippy` and several agent builds concurrently against
multiple full target directories; then recycle cache stores automatically.
```

#### Correct
```text
Run `pwsh scripts/dev/verify.ps1 -Package <package>` while editing, then one
`pwsh scripts/dev/verify.ps1` at the integration boundary. Keep optional cache
tools isolated and require exact, recoverable cleanup evidence.
```
