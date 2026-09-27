# Relocation CI compatibility implementation plan

## Ordered steps

1. Read the SQLite backend spec and shared cross-layer guide; verify the worktree is based on `origin/fix/session-relocation-identity` and contains no unrelated files.
2. Add the task context entries for the SQLite spec, cross-layer guide, and recorded CI failure evidence.
3. Replace `chunks_exact(4)` with `as_chunks::<4>()` in `bytes_to_f32_vec`.
4. Expand `f32_blob_round_trips_and_drops_partial_tail` for empty input and 1/2/3-byte tails while preserving the existing value assertions.
5. Run the affected crate fmt/Clippy/tests, then workspace debug/release and semantic-candle checks.
6. Run `git diff --check`, review the allowlist, and commit with `fix(sqlite): support current Clippy slice lint`.
7. Push with `git push origin HEAD:fix/session-relocation-identity`; monitor PR #12 required checks and merge only after all required jobs are green.

## Validation commands

```text
cargo fmt --all --check
cargo clippy -p agent-session-grep-adapters-sqlite --all-targets --offline -- -D warnings
cargo test -p agent-session-grep-adapters-sqlite --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline --no-fail-fast
cargo test --workspace --release --offline --no-fail-fast
cargo test -p agent-session-grep-application --features semantic-candle --offline
git diff --check
```

## Review gates

- No `#[allow]`, CI-policy, schema, API, cursor, or relocation behavior changes.
- No review reports, semantic advisory PRD, or `09-04` task files in the PR diff.
- Hosted generic CI must reach and pass its downstream test, Web, semantic, Robot, and Python steps after Clippy is fixed.

## Progress on 2026-09-27

- Steps 1–5 complete. Local SQLite Clippy/tests pass (265 unit tests, 4 process tests); workspace debug Clippy/tests and release tests pass; semantic-candle reports 287 passed.
- Independent Trellis check passed for implementation semantics, scope, little-endian behavior, and tail coverage; exact Rust 1.90 compilation was not directly verified.
- Rust 1.90 toolchain installation was attempted but static.rust-lang.org downloads failed with TLS handshake EOF. Rust 1.90 was therefore not directly executed; retain this as a validation limitation.
- Steps 6–7 remain pending the final local diff gate, commit, push, and PR required checks.
