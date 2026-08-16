# Final Integration Release Rehearsal — Implementation (framework slice)

## Preconditions

- Work in the repository root (write set: task dir,
  `docs/release/`, `scripts/rehearsal/`, `tests/e2e_consistency*`).
- Design: `design.md` (decisions D1-D4).

## Steps

### 1. Rehearsal runbook — `docs/release/rehearsal-runbook.md`

Full procedure: clean-environment setup per platform (Windows/macOS/Linux;
clean VM or container image — explicitly NOT GitHub-hosted runners, per
`core-beta-evidence.yml` evidence rules) → install → ingest/index synthetic
corpus → search → context → resume dry-run → handoff pack → Web UI
walkthrough → uninstall → reinstall idempotency. Every step names its
expected evidence artifact and a run id. Feature-pending steps carry a
`[pending feature <subtask>]` marker.

### 2. Consistency script — `scripts/rehearsal/compare_entrypoints.py`

Stdlib-only. Adapter per entry point (cli, mcp, robot, web, tui); web/tui
default to explicit skipped records. Runs canonical operations (search,
list_sessions) against a synthetic fixture db through CLI robot JSON and MCP
JSON-RPC; strips transport envelopes; diffs closed canonical field sets;
emits a versioned report JSON with overall verdict and per-operation
divergences. Exit codes: 0 consistent, 1 divergent, 2 usage.

### 3. Rust smoke test — `crates/agent-session-grep-cli/tests/e2e_consistency.rs`

Shells the script in fixtures-only mode (temp db, `CARGO_BIN_EXE_` binary);
asserts report schema version, consistent verdict, five entry points present
as compared/aliased/skipped, pending = skipped with reason "not implemented",
privacy assertions, and `--help` runs. Separate test target; existing suite
untouched.

### 4. Templates — `docs/release/environment-manifest.template.json`,
   `docs/release/go-no-go.template.md`

Environment manifest: os/build, clean image id, installer hash, toolchain,
provider fixture license + redaction status, run id. Go/No-Go: privacy /
performance / materials final checks, five-entry consistency, provider
evidence, residual risks, owner sign-off block.

### 5. Task artifacts — `design.md`, `implement.md`, `check.jsonl`

Framework-slice design, implementation plan, and check entries recorded in
the task dir.

## Validation

- `python scripts/rehearsal/compare_entrypoints.py --help` runs.
- `cargo test -p agent-session-grep-cli --test e2e_consistency` green
  (with `CARGO_TARGET_DIR` set if a parallel worktree is building).
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets
  -- -D warnings` stay green.

## Review gates

- No absolute transcript path in any report/error/progress output.
- Existing tests untouched; new files only within the write set.
- Do NOT commit/push.
