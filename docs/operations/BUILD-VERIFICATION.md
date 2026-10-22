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

Package mode runs only the targeted check. It is the default feedback loop for
editing; it is not an integration result.

## Serial integration gate

Run one full gate after the edit set is ready:

```powershell
pwsh ./scripts/dev/verify.ps1
```

With no `-Package`, the script runs these commands in order and stops at the
first nonzero exit:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The script sets `CARGO_BUILD_JOBS` to `4` by default, accepts a lower value with
`-Jobs 1..4`, and restores the previous process environment and working
directory when it exits. Local builds retain Cargo's configured incremental
behavior. CI disables incremental compilation because its runners start from a
fresh checkout.

## Shared machine concurrency

Run one full integration gate at a time. If independent agents genuinely need
to compile concurrently, limit the machine to at most three Cargo processes and
give each process `-j4` (or `CARGO_BUILD_JOBS=4`). Additional full gates should
wait rather than increase compiler and disk pressure.

## Optional tooling

MBX and sccache are not defaults. Use them only for an explicitly scoped
measurement with isolated stores and recorded evidence; do not add a wrapper,
change global `PATH`, or treat a tool trial as a general build recommendation.
The CI workflow keeps the repository cache, binds its key to `Cargo.lock`,
`rust-toolchain.toml`, and `.cargo/config.toml`, and runs the semantic-candle
matrix separately from the main test matrix.

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
