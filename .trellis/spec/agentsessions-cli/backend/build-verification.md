# Build verification and CI cache contract

## Scenario: Local feedback and CI build verification

### 1. Scope / Trigger
Changing local Rust verification commands, Cargo job concurrency, test-runner
selection, the pinned nextest installer, CI cache identity, Rust toolchain
selection, or optional build-cache tooling. The contract prevents duplicate target-directory fan-out, silent
runner substitution, stale cache keys and unsafe cache cleanup from becoming the
default workflow.

### 2. Signatures
- `pwsh scripts/dev/verify.ps1 -Package <cargo-package> [-Jobs 1..4]`
- `pwsh scripts/dev/verify.ps1 -Package <cargo-package> -Test [-NativeTests | -NextestPath <path>] [-Jobs 1..4]`
- `pwsh scripts/dev/verify.ps1 [-NativeTests | -NextestPath <path>] [-Jobs 1..4]`
- `pwsh scripts/dev/install-nextest.ps1 [-DestinationRoot <path>]`
- CI jobs: `test`, `semantic-candle`, `installer`, `deny`.
- Environment: `CARGO_BUILD_JOBS`, `CARGO_INCREMENTAL`, `ASG_NEXTEST`,
  `LOCALAPPDATA`.

### 3. Contracts
- Package check mode runs exactly
  `cargo check -p <package> --all-targets --locked` with `CARGO_BUILD_JOBS`
  defaulting to 4. `-Package` without `-Test` keeps this behavior.
- Package-test mode uses the selected runner with `-p <package>`; when nextest
  is selected it then runs `cargo test -p <package> --locked --doc -j 4
  --no-fail-fast`.
- Full mode runs, in order and stopping at the first failure:
  `cargo fmt --all --check`;
  `cargo clippy --workspace --all-targets --locked -- -D warnings`;
  the selected workspace test command.
- The nextest command is
  `cargo-nextest nextest run --workspace --locked --test-threads 8
  --build-jobs 4 --no-fail-fast`, followed by
  `cargo test --workspace --locked --doc -j 4 --no-fail-fast`.
- The native test fallback is
  `cargo test --workspace --locked -j 4 --no-fail-fast`. Native `cargo test`
  includes doctests, so no separate doctest command follows it.
- `-NativeTests` bypasses nextest discovery. `-NextestPath` and `ASG_NEXTEST`
  are explicit candidates and must exist, parse a version, and be
  `0.9.149` or newer; an invalid explicit candidate is an error.
- Automatic discovery checks
  `%LOCALAPPDATA%\ASG\tools\nextest\nextest-*\bin\cargo-nextest.exe`,
  `PATH`, top-level `.trellis/.runtime/nextest-*/bin/cargo-nextest.exe`, and one
  task-directory level at
  `.trellis/.runtime/*/nextest-*/bin/cargo-nextest.exe`. It selects the highest
  valid version at or above the minimum; at the same version the user-local
  installer root wins. If candidates exist but none is valid, the script fails.
  Native fallback is allowed only when no candidate exists.
- The nextest installer supports Windows x86_64 only and defaults to
  `%LOCALAPPDATA%\ASG\tools\nextest\nextest-0.9.149\bin\cargo-nextest.exe`.
  It downloads the pinned `0.9.149` x86_64 MSVC release archive, verifies SHA-256
  `bf904b85c7c5c2a29c340053a8d9d8e8027ff0d55a2bf7cef7d23032f8166438`, checks
  the extracted version and host, and moves a complete staged install into
  place. `-DestinationRoot` overrides only the root. A repeat run validates the
  install manifest, executable hash, and version. The installer never changes
  `PATH` or global Cargo configuration and refuses an incomplete or unverified
  existing target.
- `verify.ps1` resolves the repository root from its own path, restores the prior
  working directory and `CARGO_BUILD_JOBS`, and never installs nextest or changes
  global `PATH` or Cargo configuration.
- Run one full gate at a time. Independent agent work may use at most three
  concurrent Cargo processes with `-j4` each.
- The Windows test matrix installs the pinned nextest through
  `scripts/dev/install-nextest.ps1` under `RUNNER_TEMP`, runs
  `nextest run --workspace --locked --test-threads 8 --build-jobs 4
  --no-fail-fast`, and then runs workspace Cargo doctests. Linux and macOS keep
  native `cargo test` pending hosted evidence on those platforms. CI sets
  `CARGO_INCREMENTAL=0`, explicitly installs Rust `1.97.1`, and binds each target
  cache key to `Cargo.lock`, `rust-toolchain.toml`, and `.cargo/config.toml`.
- Pull-request workflow concurrency is scoped to the PR and cancels an older
  in-progress run when a newer run starts.
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
- `-Test` without `-Package` -> explicit error before Cargo runs.
- `-NativeTests` with `-NextestPath` -> explicit conflict error.
- `-NextestPath` or `ASG_NEXTEST` missing, unexecutable, unparseable, or older
  than `0.9.149` -> explicit error; no native fallback.
- Automatic candidates exist but all are invalid -> explicit error; no native
  fallback.
- No nextest candidate exists -> native Cargo test fallback.
- Installer runs off Windows x86_64, receives a file destination root, finds a
  wrong archive/version/host/hash, or encounters an incomplete existing target
  -> explicit error with no PATH/global Cargo changes.
- Repository root cannot be found -> explicit error before Cargo runs.
- Any Cargo or nextest command returns nonzero -> script throws that command and
  exit code.
- CI cache key omits lockfile/toolchain/config identity -> invalid workflow.
- PR concurrency loses cancellation, Windows stops using pinned nextest t8, or
  Linux/macOS stop using native Cargo tests -> invalid workflow regression.
- Semantic-candle remains inside the main matrix -> invalid workflow regression.
- Recycle source disappears without matching Recycle Bin metadata -> hard stop;
  do not continue cleanup or claim recoverability.

### 5. Good/Base/Bad Cases
- Good: edit one crate with package check mode; use package-test mode for that
  crate; when the edit set is ready, run one serial full gate.
- Base: a user-local pinned nextest is installed and selected; without any
  candidate the full gate runs native Cargo tests, and an explicit nextest path
  is validated before it is used.
- Bad: silently fall back after an explicit invalid path, run nextest without
  doctests, launch several full workspace gates in parallel, cache only the
  lockfile, or use MBX as the default single-checkout build.

### 6. Tests Required
- Parse `install-nextest.ps1` and `verify.ps1` under PowerShell 7 and Windows
  PowerShell 5.1 and inspect their help output.
- Run the installer twice to prove archive/hash/version verification and
  idempotence, then assert `verify.ps1` selects the stable user-local path when
  checkout-local candidates also exist.
- Run one targeted package check, one package-test mode, and one complete full
  gate after a runner-path change.
- Exercise nextest discovery, invalid explicit path, pre-`0.9.149` rejection,
  native fallback, and both package/full doctest commands.
- Assert `-Jobs 5` is rejected and both the working directory and
  `CARGO_BUILD_JOBS` are restored.
- Parse CI YAML and assert four jobs, PR-scoped concurrency with cancellation,
  Windows pinned nextest t8 plus doctests, Linux/macOS native Cargo tests, the
  semantic-candle matrix, incremental disablement, pinned Rust, three cache
  inputs, retained gates and 40-character action SHAs.
- Validate MBX evidence and cleanup receipts against their raw runtime records.
- `git diff --check` must pass and no unrelated file may be staged.

### 7. Wrong vs Correct
#### Wrong
```text
Run `cargo test`, `cargo clippy` and several agent builds concurrently against
multiple full target directories; use an arbitrary old nextest; then recycle
cache stores automatically.
```

#### Correct
```text
Run `pwsh scripts/dev/verify.ps1 -Package <package>` while editing, use
`-Test` for package tests, then one `pwsh scripts/dev/verify.ps1` at the
integration boundary. Validate any nextest candidate and keep optional cache
tools isolated behind exact, recoverable evidence.
```
