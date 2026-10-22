# Local test-runner experiment

- Date: 2026-10-22 Asia/Shanghai
- Source commit: `bbbf9c5cc9b40dd6e374694387167b37f3f9a4f5`
- Platform: Windows / x86_64-pc-windows-msvc
- Toolchain: Cargo/Rust `1.97.1`
- Candidate runner: `cargo-nextest 0.9.149`
- Decision: fixed eight-thread nextest is the stable local default when installed;
  Windows CI uses it, while Linux/macOS CI remain native pending hosted evidence

## Question

Measure whether cargo-nextest materially improves the warm local workspace test
loop without losing correctness or doctest coverage. Keep the comparison on one
source commit and one worktree so source identity, target cache, toolchain and
ambient machine state stay as close as practical.

## Protocol

The screen compared native Cargo with default, four-thread and eight-thread test
concurrency, and nextest with default, four-thread and eight-thread concurrency.
Native candidates used `cargo test --workspace --locked -j4 --no-fail-fast`.
Nextest candidates used:

```text
cargo-nextest nextest run --workspace --locked --build-jobs 4 --no-fail-fast
cargo-nextest nextest run --workspace --locked --test-threads 4 --build-jobs 4 --no-fail-fast
cargo-nextest nextest run --workspace --locked --test-threads 8 --build-jobs 4 --no-fail-fast
```

After each successful nextest run, Cargo doctests ran separately:

```text
cargo test --workspace --locked --doc -j4 --no-fail-fast
```

Measured totals therefore include doctests for nextest candidates. Native
`cargo test` already includes doctests. The final comparison used five paired
baseline/nextest-t8 rounds with alternating order, and required no external
Cargo/Rust process before a candidate and a clean worktree after the campaign.
Nextest discovered 2001 tests, skipped 23, and executed 1978 per successful run
across 75 binaries.

## Result

| Metric | Native Cargo | Nextest t8 | Difference |
|---|---:|---:|---:|
| Median | 101.562 s | 72.072 s | 29.04% faster |
| Nearest-rank P95 | 103.130 s | 73.414 s | 28.81% lower |

Native paired times, in seconds: `103.130`, `96.188`, `101.850`, `101.562`,
`99.103`.

Nextest-t8 paired times, including doctests, in seconds: `72.256`, `71.856`,
`72.072`, `73.414`, `70.689`.

The paired median ratio was `0.7133`. The seeded paired bootstrap 95% interval
for the median ratio was `0.7006–0.7470`.

## Correctness and disqualification

All five nextest-t8 pairs exited zero. Default-thread nextest was disqualified:
its final round failed
`agent-session-grep-adapters-sqlite::maintenance_queue
concurrent_first_open_is_serialized_inside_initialization_transaction` with
`WriterBusy("maintenance queue is busy")`. That run reported 1978 tests, with
1977 passed, one failed, and 23 skipped. The failure is the reason the local
workflow fixes concurrency at eight test threads rather than using nextest's
machine-derived default.

## Workflow decision

`scripts/dev/verify.ps1` supports package tests with `-Test` and uses a valid
cargo-nextest `0.9.149` or newer. Explicit selection through `-NextestPath` or
`ASG_NEXTEST` takes precedence. Automatic discovery checks the stable
`%LOCALAPPDATA%\ASG\tools\nextest` root, `PATH`, and current-checkout
`.trellis/.runtime` candidates, choosing the highest valid version and preferring
the user-local install at an equal version. Invalid explicit paths/versions and
automatic candidate sets with no usable version fail rather than silently
falling back; native tests are used only when no candidate exists. `-NativeTests`
remains the explicit bypass. Nextest runs with eight test threads and four build
jobs, then Cargo runs doctests separately.

The runner is now the stable local default when installed, rather than an
experimental opt-in. `scripts/dev/install-nextest.ps1` pins the Windows x86_64
`cargo-nextest 0.9.149` release archive to SHA-256
`bf904b85c7c5c2a29c340053a8d9d8e8027ff0d55a2bf7cef7d23032f8166438`, verifies
the extracted version and host, and installs under
`%LOCALAPPDATA%\ASG\tools\nextest\nextest-0.9.149` without changing `PATH` or
global Cargo configuration. The Windows CI test matrix applies the measured t8
path through an isolated installer root and then runs workspace doctests. Linux
and macOS CI remain native pending hosted evidence on those platforms.

## Evidence

- `.trellis/.runtime/build-feedback-infrastructure-20261018/nextest-trial-20261022/runner-summary.json`
- `.trellis/.runtime/build-feedback-infrastructure-20261018/nextest-trial-20261022/screen-results.json`
- `.trellis/.runtime/build-feedback-infrastructure-20261018/nextest-trial-20261022/finalist-continuation-results.json`
- `.trellis/.runtime/build-feedback-infrastructure-20261018/nextest-trial-20261022/logs/nextest-default-final-r2.log`
- `.trellis/.runtime/build-feedback-infrastructure-20261018/nextest-trial-20261022/timing-corroboration.json`

## Limitations

The result is Windows-local and used a warm target. It supports the fixed
eight-thread local default and the Windows CI configuration, but it is not a
hosted-CI speed claim. Linux and macOS still require their own hosted evidence
before replacing native Cargo tests.

The first finalist pair was originally captured in the console but not written
to a standalone JSON receipt. Its preserved log timestamps corroborate the two
Stopwatch values to within 0.04 seconds: 103.094s versus 103.130s for native and
72.242s versus 72.256s for nextest-t8. The corroboration receipt is retained with
the other evidence.

## Stable application verification

On 2026-10-22 the Windows x86_64 installer installed and verified
`cargo-nextest 0.9.149` at the stable user-local root. Its archive digest matched
`bf904b85c7c5c2a29c340053a8d9d8e8027ff0d55a2bf7cef7d23032f8166438`; the
installed executable digest was
`4a9731975449492e2377dfc1d87a04a7a2ba8e4b8c5bda91a35b877ec3992898`. A second
run verified the manifest, executable digest and version without downloading.
An isolated `-DestinationRoot` install also passed and was removed afterward.

`verify.ps1 -Package agent-session-grep-domain -Test` selected the stable
`LOCALAPPDATA/ASG/tools/nextest` candidate over the checkout runtime copy and
passed 70/70 tests plus package doctests. PowerShell 7 and Windows PowerShell
5.1 AST/help checks, YAML parsing, workflow contracts and task-context validation
all passed. A later project-wide verification run passed the full gate: 1978/1978 nextest tests across 75 binaries with 23 skipped, plus every workspace doctest target.
