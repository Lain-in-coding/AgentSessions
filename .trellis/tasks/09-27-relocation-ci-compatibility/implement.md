# Relocation CI compatibility implementation plan

## Current verification summary (2026-09-28)

- Source and dependency implementations are complete on
  `fix/relocation-ci-test-fixtures`; local gates and independent review passed,
  and the change is ready for standard commits and a fast-forward PR update.
- The Windows legacy-locator negative regression now refuses through ingest,
  explicit sync and discovery; the targeted SQLite and CLI suites passed in
  debug/release before the dependency patch.
- With rusqlite 0.40.2/libsqlite3-sys 0.38.2, Windows Rust 1.90.0 passes
  `check --workspace --all-targets --all-features --locked --offline` and the
  SQLite BLOB regression (`1 passed`). The earlier MSRV failure below is
  historical reproduction evidence, now repaired.
- Final-lock cargo-deny and cargo-audit both exit 0. Cargo audit still reports
  the existing `paste` RUSTSEC-2024-0436 unmaintained warning; dependency policy
  and semantic features were not changed. The separate advisory task remains open.
- Independent full-scope review of all 21 files passed with no findings or
  reviewer edits; fmt and workspace all-target Clippy passed. Final-lock stable
  workspace debug/release and semantic gates passed. Hosted PR #12 remains pending.


## Ordered steps

1. Read the SQLite backend spec and shared cross-layer guide; verify the worktree is based on `origin/fix/session-relocation-identity` and contains no unrelated files.
2. Add the task context entries for the SQLite spec, cross-layer guide, and recorded CI failure evidence.
3. Replace `chunks_exact(4)` with `as_chunks::<4>()` in `bytes_to_f32_vec`.
4. Expand `f32_blob_round_trips_and_drops_partial_tail` for empty input and 1/2/3-byte tails while preserving the existing value assertions.
5. Run the affected crate fmt/Clippy/tests, then workspace debug/release and semantic-candle checks.
6. Run `git diff --check`, review the allowlist, and commit with `fix(sqlite): support current Clippy slice lint`.
7. If hosted CI exposes an unrelated cross-platform regression in the affected PR, fix it in the same task only when the root cause is a test/platform contract mismatch; rerun the complete local gate.
8. Push with `git push origin HEAD:fix/session-relocation-identity`; monitor PR #12 required checks and merge only after all required jobs are green.

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

- No `#[allow]`, CI-policy, schema, API, cursor, or new relocation capability changes; the documented legacy lookup correction restores fail-closed behavior.
- No review reports, semantic advisory PRD, or `09-04` task files in the PR diff.
- Hosted generic CI must reach and pass its downstream test, Web, semantic, Robot, and Python steps after Clippy is fixed.

## Progress on 2026-09-27

- Steps 1–5 complete. Local SQLite Clippy/tests pass (265 unit tests, 4 process tests); workspace debug Clippy/tests and release tests pass; semantic-candle reports 287 passed.
- Independent Trellis check passed for implementation semantics, scope, little-endian behavior, and tail coverage; exact Rust 1.90 compilation was not directly verified.
- Rust 1.90 toolchain installation was attempted but static.rust-lang.org downloads failed with TLS handshake EOF. Rust 1.90 was therefore not directly executed; retain this as a validation limitation.
- Steps 6–7 remain pending the final local diff gate, commit, push, and PR required checks.

## CI follow-up on 2026-09-27

- Hosted Ubuntu and macOS generic tests exposed an existing CLI test that unconditionally expected Windows backslash normalization. The implementation intentionally normalizes separators only on Windows, so the test assertion is now `#[cfg(windows)]` while portable root-matching cases remain unconditional.
- Windows generic CI passed the original workspace test, Web tests, semantic-candle feature, entrypoint, Robot, release manifest, and evidence harness stages on the first compatibility push.

## Continued verification on 2026-09-28

- Local commits `f31aebb` and `53d5c53` correspond to remote commits `ada5f210b649b86df1395a9907c1cb63c61fec64` and `805735cfac88dc7c3330880e0ab3454f831ac63d`. The Git Data API advanced the existing PR ref with `force=false` after checking its parent and tree because local Git HTTPS/credential-helper transport was failing.
- The final workspace release gate remains open: an earlier complete run had CLI e2e failures, while the isolated release e2e run passed. A focused pass does not replace a full final-tree release gate.
- Complete second-run Ubuntu/macOS failure evidence is recorded in `research/ci-blocker.md`. A Trellis implement agent owns the narrow CLI fixture/assertion repair; an independent research agent checks the legacy identity proof boundary.
- Next: run both failed cases in debug/release, then final-tree fmt, workspace Clippy, workspace debug/release, and semantic-candle tests; run an independent Trellis check; capture spec lessons; commit and update the existing PR without history rewriting. Merge only after the new head passes all required hosted checks.

## Reproduced production repair (2026-09-28)

- CLI fixture repairs pass focused debug/release tests. The new raw-Windows-locator negative case fails in both profiles before production edits, proving a lookup bypass; its expected behavior remains refusal with unchanged catalog/registry/generation.
- Extend implementation to the shared SQLite namespace-resolution boundary as specified in `design.md`, then rerun focused ingest/sync/discovery and storage regressions before the complete gate. Parent owns task/spec updates and Git operations; the implement agent owns only the listed production/test files.
- Normal Git HTTPS fetch now succeeds with process-scoped authentication/TLS settings. A new local branch `fix/relocation-ci-test-fixtures` was created at remote `805735c` only after verifying its committed tree exactly matched the old local tree; all uncommitted edits were retained. No branch was reset or force-updated.
- Rust 1.90.0 was installed cleanly with the minimal profile after an incomplete installation and intermittent TLS download failures. Actual MSRV compilation is now possible and remains a required pending check.

## Actual MSRV compilation result (2026-09-28)

`cargo +1.90.0 check --workspace --all-targets --all-features --locked --offline`
failed in upstream `libsqlite3-sys 0.38.1` `build.rs:110` with E0658 for
`cfg_select!`, before project code was checked. This is a confirmed dependency
compatibility blocker, not a successful MSRV gate. An independent research
pass is checking released rusqlite/libsqlite3-sys combinations; no dependency
or MSRV declaration has been changed yet. Raw compiler logs stay local.

The SQLite and CLI specs now record the legacy-locator guard and regression
contracts. The final check must include these specs and both affected crates.

- Upstream verification found the official 2026-08-08 MSRV patch pair,
  rusqlite 0.40.2/libsqlite3-sys 0.38.2. The design and context now authorize
  this scoped forward update while retaining bundled SQLite and security fixes.
  A separate implement agent owns only those Cargo manifests and lockfile;
  the source-repair agent keeps ownership of SQLite/CLI code and regression tests.

## Independent full-scope check (2026-09-28)

The fresh Trellis check reviewed the full active-task diff from `5b232cd`,
including both previously committed compatibility changes and all source,
dependency, spec and task changes (21 files). No blocking or non-blocking code
findings remained, and the checker made no edits. It confirmed locator-guard
ordering/atomicity, exact and ambiguous legacy proof, all three ingress routes,
Windows/UNC/POSIX cases, canonical cwd assertions, the two-package dependency
patch and the documented cold-path cost. Fmt, workspace all-target Clippy and
`git diff --check 5b232cd` passed. Hosted Ubuntu/macOS validation remains required.

## Final local gate (2026-09-28)

All checks used the final source/dependency tree; the source fingerprint was
unchanged across the complete debug/release/semantic sequence.

| Check | Result |
| --- | --- |
| Rust 1.90 workspace/all-target/all-feature locked offline check | pass |
| Rust 1.90 SQLite BLOB regression | 1 passed |
| Current stable fmt and workspace all-target Clippy (`-D warnings`) | pass |
| Locked offline workspace debug tests | 1,766 passed, 0 failed, 20 existing ignored |
| Locked offline workspace release tests | 1,766 passed, 0 failed, 20 existing ignored |
| Locked offline semantic-candle application tests | 287 passed |
| Locked offline semantic-candle CLI/all-target check | pass |
| Embedded Web interaction tests | 11 passed |
| Python scripts / release / evidence | 18 (1 skipped) / 9 / 58, all successful |
| cargo-deny / cargo-audit | exit 0; existing paste maintenance warning retained |
| Trellis context validation / diff check / changed-file privacy scan | pass |
| Independent full-scope review | no findings; no reviewer edits |

The Python evidence suite used the newly built release binary; its synthetic
entrypoint smoke passed all 10 checks. Existing ignored/skipped cases were not
changed by this task. Raw logs and temporary fixtures stay outside version
control. New task/source/spec files contain no GitHub credential, private-key
block or personal absolute path. The root worktree's review artifacts remain
untouched and are excluded from both commit groups.

Commit groups: the four dependency manifests plus lockfile form
`fix(deps): restore Rust 1.90 SQLite compatibility`; the source guard, regression
fixtures, two executable specs and this task's evidence form
`fix(sqlite): reject ambiguous legacy source locators`. The user's existing
explicit commit/push authorization applies; no history rewriting is required.

Hosted checks must run on the new PR head before merge or task completion.
