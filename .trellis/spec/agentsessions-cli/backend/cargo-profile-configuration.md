# Cargo Development Profile Configuration

## Scenario: bounded development artifacts

### 1. Scope / Trigger

The repository applies a development-profile policy that reduces local target
allocation while preserving line-level panic locations and an explicit full
debugging profile.

### 2. Signatures

- Repository override: `.cargo/config.toml`
- Machine override: `%USERPROFILE%\.cargo\config.toml`
- Toolchain pin: `rust-toolchain.toml` with channel `1.97.1`
- Full debugging command: `cargo build --profile debugging`

### 3. Contracts

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false

[profile.debugging]
inherits = "dev"
debug = true
```

The repository override travels with clones and CI. The user override applies
to other local Rust projects but is not committed. The `test` profile inherits
`dev`; release remains unchanged. Workspace crates retain filename/line debug
information, dependencies omit debug information, and the custom profile
restores full debug information for the dependency graph.

### 4. Validation & Error Matrix

- Workspace build lacks `-C debuginfo=line-tables-only` -> configuration not loaded.
- Dependency build lacks `-C strip=debuginfo` -> package wildcard override not applied.
- `cargo build --profile debugging` fails -> custom profile invalid.
- Rust older than 1.71 -> line-tables-only unsupported; project must pin a supported toolchain or remove the user override.
- Stable `cargo config get` -> unstable command; use build logs instead.

### 5. Good/Base/Bad Cases

- Good: `cargo build --workspace --all-targets` succeeds with bounded debug data.
- Base: `cargo test --workspace` and Clippy pass under the same profile.
- Bad: placing profile settings in a member crate or relying only on an environment variable for persistent project behavior.

### 6. Tests Required

- Build verbosely and assert workspace `debuginfo=line-tables-only`.
- Assert dependency compilation uses `strip=debuginfo`.
- Run workspace tests and Clippy with warnings denied.
- Check the `debugging` profile on a representative crate.
- Verify a project outside the repository inherits the user-level profile.

### 7. Wrong vs Correct

#### Wrong

```toml
# crates/example/Cargo.toml
[profile.dev]
debug = "line-tables-only"
```

Cargo ignores profile tables outside the workspace root manifest and can leave
the developer believing the setting applied.

#### Correct

Keep the repository override in `.cargo/config.toml`, the machine override in
`%USERPROFILE%\.cargo\config.toml`, and verify the effective flags from a real
verbose build.
