# Local Rust verification

The repository pins Rust and Cargo to `1.97.1` through `rust-toolchain.toml`.
Use the pinned toolchain for checks and do not replace it with an ambient stable
installation.

## Edit loop

Run a package-scoped check while changing one crate:

```powershell
cargo check -p <package> --all-targets --locked
```

The wrapper provides the same mode and locates the repository root from the
script path:

```powershell
pwsh ./scripts/dev/verify.ps1 -Package agent-session-grep-domain
```

Package check mode runs only the targeted check. It is the default feedback
loop for editing; it is not an integration result.

To test one package, add `-Test`:

```powershell
pwsh ./scripts/dev/verify.ps1 -Package agent-session-grep-domain -Test
```

Package-test mode runs that package's tests and its doctests. Native Cargo tests
are selected with `-NativeTests`:

```powershell
pwsh ./scripts/dev/verify.ps1 -Package agent-session-grep-domain -Test -NativeTests
```

## Test runner selection

Test modes use a valid `cargo-nextest 0.9.149` or newer. When installed, the
pinned user-local runner is the stable default. The wrapper resolves the runner
before running Cargo, using this order:

1. `-NextestPath <path>`;
2. `ASG_NEXTEST=<path>`;
3. automatic candidates under
   `%LOCALAPPDATA%\ASG\tools\nextest\nextest-*\bin\cargo-nextest.exe`,
   on `PATH`, and under either `.trellis/.runtime/nextest-*/bin/cargo-nextest.exe`
   or one task-directory level at
   `.trellis/.runtime/*/nextest-*/bin/cargo-nextest.exe`.

Automatic candidates choose the highest valid version. At the same version, the
user-local install root wins over `PATH` and checkout-local runtime copies.

An explicit path must exist, report a parseable version, and satisfy the minimum
version. An invalid explicit path fails instead of falling back. If automatic
discovery finds candidates but none is usable, the wrapper also fails; native
fallback happens only when no candidate exists. `-NativeTests` is the explicit
bypass.

Install the pinned runner into its user-local default once:

```powershell
pwsh ./scripts/dev/install-nextest.ps1
```

The installer verifies the release archive SHA-256, exact `0.9.149` version, and
Windows x86_64 MSVC host identity before an atomic move into
`%LOCALAPPDATA%\ASG\tools\nextest\nextest-0.9.149`. A second run verifies the
install manifest and executable hash instead of downloading again. It never
changes `PATH` or global Cargo configuration. CI can keep the same versioned
layout under an isolated root with `-DestinationRoot <path>`.

`verify.ps1` itself never installs nextest. A different local executable can be
selected without exposing it globally:

```powershell
pwsh ./scripts/dev/verify.ps1 -NextestPath C:\tools\cargo-nextest.exe
```

Nextest runs with eight test threads, four Cargo build jobs, and
`--no-fail-fast`:

```text
cargo-nextest nextest run --workspace --locked --test-threads 8 --build-jobs 4 --no-fail-fast
```

Nextest does not run doctests, so the wrapper follows a successful nextest run
with native Cargo doctests. Package-test mode uses `-p <package>` for both
commands; full mode uses `--workspace`. The native fallback uses one
`cargo test` command, which includes doctests:

```text
cargo test --workspace --locked -j 4 --no-fail-fast
```

## Serial integration gate

Run one full gate after the edit set is ready:

```powershell
pwsh ./scripts/dev/verify.ps1
```

With no `-Package`, the script runs these stages in order and stops at the first
nonzero exit:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
nextest workspace tests and separate Cargo doctests, or native Cargo tests
```

Use `-NativeTests` to force the final stage to native Cargo. The script sets
`CARGO_BUILD_JOBS` to `4` by default, accepts a lower value with `-Jobs 1..4`,
and restores the previous process environment and working directory when it
exits. Local builds retain Cargo's configured incremental behavior. CI disables
incremental compilation because its runners start from a fresh checkout.

## Shared machine concurrency

Run one full integration gate at a time. If independent agents genuinely need
to compile concurrently, limit the machine to at most three Cargo processes and
give each process `-j4` (or `CARGO_BUILD_JOBS=4`). Additional full gates should
wait rather than increase compiler and disk pressure.

## CI

The Windows test matrix installs the same pinned runner through
`scripts/dev/install-nextest.ps1 -DestinationRoot <runner-temp>`, runs the fixed
eight-thread nextest workspace suite, and then runs workspace doctests with
Cargo. Linux and macOS keep native `cargo test` pending hosted-run evidence for
those platforms. Pull-request workflow groups use PR-scoped concurrency and
cancel an older in-progress run when a newer run for the same PR starts.

MBX and sccache are not defaults. Use them only for an explicitly scoped
measurement with isolated stores and recorded evidence; do not add a wrapper,
change global `PATH`, or treat a tool trial as a general build recommendation.
The CI workflow keeps the repository cache, binds its key to `Cargo.lock`,
`rust-toolchain.toml`, and `.cargo/config.toml`, and runs the semantic-candle
matrix separately from the main test matrix.

## Measured nextest trial

A local Windows/MSVC comparison on 2026-10-22 used Cargo/Rust `1.97.1`,
`cargo-nextest 0.9.149`, one clean source commit, a warm shared target, and five
paired baseline/nextest-t8 rounds. Nextest used eight test threads and four
build jobs; each measured total included the following Cargo doctest command.

| Metric | Native Cargo | Nextest t8 | Difference |
|---|---:|---:|---:|
| Median | 101.562 s | 72.072 s | 29.04% faster |
| Nearest-rank P95 | 103.130 s | 73.414 s | 28.81% lower |

The paired median ratio was `0.7133`, with a bootstrap 95% interval of
`0.7006–0.7470`. Nextest discovered 2001 tests, skipped 23, and executed 1978
per run. All five selected nextest-t8 pairs exited successfully.

Default-thread nextest was not selected. In its final round,
`agent-session-grep-adapters-sqlite::maintenance_queue
concurrent_first_open_is_serialized_inside_initialization_transaction` failed
with `WriterBusy("maintenance queue is busy")`; that round executed 1978 tests,
with 1977 passing, one failing, and 23 skipped. The fixed eight-thread candidate
is therefore the stable local default when nextest is installed, and Windows CI
now applies the same fixed concurrency. This remains a Windows-local performance
measurement: it does not establish a hosted speedup or justify replacing native
Cargo tests on Linux and macOS.

## Measured MBX trial

A Windows/NTFS isolated trial on 2026-10-22 used MBX 1.26.0, Cargo/Rust
1.97.1, four fresh worktrees, explicit target directories, and a private store.
Three native clean builds had a 66.554 s median. One MBX cold seed took 67.784 s;
three warm new-worktree builds had a 49.679 s median, 25.35% faster. Warm
nearest-rank P95 was 52.371 s versus 68.271 s. MBX reported 111 hits across 215
lookups and copied 274 output files because NTFS cannot use reflinks here.

The result is not a default adoption. One warm target plus the shared store used
5.793 GiB allocated versus 1.098 GiB for one native target, a 5.28x physical
footprint. `MBX_VERIFY=1` also reported 23 path-sensitive byte divergences among
85 verified outputs. MBX therefore remains an explicit, local, short-burst tool
for multiple fresh worktrees; it is not a single-checkout default and must not be
presented as a disk-saving mechanism.

Do not automate broad cleanup of MBX stores. A 2026-10-22 trial-store recycle
attempt had no recoverable Recycle Bin item even though the source disappeared;
cleanup stopped immediately. Future tool trials must use a dedicated volume or
an explicitly approved exact-disposal procedure.
