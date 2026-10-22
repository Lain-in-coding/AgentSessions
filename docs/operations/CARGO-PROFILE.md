# Cargo development profile policy

## Default behavior

The project uses Cargo profile overrides from:

- `.cargo/config.toml` in the repository for this checkout and CI clones.
- `%USERPROFILE%\.cargo\config.toml` for other Rust projects on this machine.

Both files currently apply:

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false

[profile.debugging]
inherits = "dev"
debug = true
```

Normal development commands therefore need no special flags:

```powershell
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Workspace members keep filename and line-number debug information. Dependency
debug information is omitted. The program runtime is not intentionally changed.

## Full debugging

Use the explicit profile when breakpoint variables and full type information
are required:

```powershell
cargo build --profile debugging
cargo test --profile debugging
```

This profile inherits `dev` and restores `debug = true` for the full dependency
graph.

## Toolchain policy

- This repository is pinned to Rust/Cargo `1.97.1` by `rust-toolchain.toml`.
- The machine-wide default remains `stable` and is repaired independently.
- Do not use `cargo config get` as a validation gate on stable; Cargo 1.97.1
  reports that command as nightly-only.

## Verification

`cargo build --workspace --all-targets --locked -v` must show
`-C debuginfo=line-tables-only` for workspace members and
`-C strip=debuginfo` for dependency builds. The current clean verification
produced a `target\debug` allocation of 2.354 GiB after the build and 2.869 GiB
after the workspace tests and validation completed.

The user-level configuration affects every local Rust project using a toolchain
that supports `line-tables-only` (Rust 1.71+). Projects pinned to older compilers
must temporarily remove or override the user-level profile.
