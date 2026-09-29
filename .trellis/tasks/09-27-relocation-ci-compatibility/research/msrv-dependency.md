# Rust 1.90 dependency compatibility evidence

Verified on 2026-09-28 against official GitHub and crates.io metadata.

## Reproduction

`cargo +1.90.0 check --workspace --all-targets --all-features --locked --offline`
fails in locked `libsqlite3-sys 0.38.1` at `build.rs:110`: E0658 for
`cfg_select!`. The current lock contains `rusqlite 0.40.1`; Cargo manifests
allow `0.40` in the SQLite adapter, CLI dev-dependencies, Cursor and OpenCode.
This is an actual compiler failure after installing the complete Rust 1.90.0
toolchain, not merely an inferred MSRV concern.

## Published fix

- crates.io reports non-yanked `rusqlite 0.40.2` and `libsqlite3-sys 0.38.2`,
  both published on 2026-08-08. These are the latest stable versions returned
  during this dated query, not a prediction of future releases.
- Official release `v0.40.2` is explicitly an MSRV compatibility patch. Commit
  `c922ca5b716b5a226df6eba9eea84c6320c60311` adds local `cfg_select` macros
  in the upstream crates to support Rust 1.88.0. The tagged release advances
  the normal libsqlite3-sys dependency to `0.38.2`.
- The same-tag diff retains bundled SQLite 3.53.2 and includes no SQLite
  amalgamation changes. The earlier `v0.40.1` SAVEPOINT-name SQL-injection fix
  remains present. Downgrading to 0.40.0/0.38.0 would not be an acceptable
  substitute; the official forward patch avoids that regression.
- Updating only libsqlite3-sys would leave newer `cfg_select` calls in the
  rusqlite wrapper. Update both published packages together.

## Minimal implementation and checks

Raise the existing five rusqlite declarations in four Cargo manifests from
`0.40` to `0.40.2`, retaining every feature/default-feature choice. Use a
scoped Cargo update to lock rusqlite 0.40.2 and libsqlite3-sys 0.38.2, and
verify no unrelated packages change. Keep workspace `rust-version = "1.90"`.

Run actual Rust 1.90 workspace/all-targets/all-features compilation, the SQLite
BLOB regression on 1.90, final stable fmt/Clippy/workspace debug/release and
semantic-candle checks, plus cargo-deny/audit after the dependency update.
Do not vendor, patch upstream source, add nightly feature switches, ignore
advisories, or remove semantic functionality.

## Primary sources

- https://github.com/rusqlite/rusqlite/releases/tag/v0.40.2
- https://github.com/rusqlite/rusqlite/compare/v0.40.1...v0.40.2
- https://github.com/rusqlite/rusqlite/commit/c922ca5b716b5a226df6eba9eea84c6320c60311
- https://github.com/rusqlite/rusqlite/releases/tag/v0.40.1
- https://crates.io/api/v1/crates/rusqlite
- https://crates.io/api/v1/crates/libsqlite3-sys

The metadata was fetched directly with gh/curl; no private source, transcripts,
credentials, or user paths were sent to search services. The preliminary
research sub-agent hit a rate limit; the parent completed this source check.

## Implemented validation

The scoped patch changed only the five rusqlite declarations and the versions/
checksums for the two lock entries. `cargo fetch --locked` completed, followed
by successful Windows Rust 1.90.0 all-workspace/all-target/all-feature offline
compilation and the SQLite BLOB unit regression (`1 passed`). Final-lock
cargo-deny and cargo-audit both exited 0; the pre-existing optional semantic
`paste` maintenance warning remains a separate open task.
