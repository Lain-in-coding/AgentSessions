# Provider source auto-discovery — Implementation

## Preconditions

- `task.py start` this task before editing.
- Work in `C:/AgentSessions/.claude/worktrees/integration-08-13-four-features-v2`
  on top of the verified Resume-protocol baseline.
- Research: `research/2026-08-14-discovery-scope-audit.md` (code locations).
- Design: `design.md` (decisions D1-D5).

## Steps

### 1. Schema v9 — `provider_id` on `source_scans`

- `adapters-sqlite/src/lib.rs`: add `provider_id TEXT` column to the
  `source_scans` CREATE (for fresh DBs) and a v8→v9 `ALTER TABLE
  source_scans ADD COLUMN provider_id TEXT` migration. Bump
  `PRAGMA user_version` to 9.
- Write `provider_id` into `source_scans` during `commit_source_batches_if_changed`
  (the batch already carries the provider via `resume_claim` or the staged
  report; if neither, leave NULL).
- New store method:
  `pub fn source_paths_for_provider(&self, provider_id: &str) -> PortResult<Vec<String>>`
  — `SELECT source_path FROM source_scans WHERE provider_id = ?1`.
- Test: v8 DB migrates to v9 with the column; backfill is NULL for legacy
  rows; `source_paths_for_provider` returns only that provider's paths.

### 2. Provider data-root resolver (CLI-local)

- `cli/src/main.rs`: add
  `fn provider_data_root(provider_id: &str) -> Option<PathBuf>`.
  - Resolve home dir via `HOME` / `USERPROFILE` (reuse the env-var pattern
    from `platform_paths_impl` at `~465-526`).
  - `claude-code` → `~/.claude/projects`
  - `codex` → `~/.codex/sessions`
  - Unknown provider → `None`.
- Test (unit): returns the right path for each known provider; `None` for
  unknown; does not panic when env unset.

### 3. Directory walker

- `cli/src/main.rs`: add
  `fn discover_provider_sources(root: &Path) -> Result<(Vec<String>, bool), CliError>`.
  - Recursively walk `root`, collect `*.jsonl` file paths (string, forward
    slashes).
  - Returns `(paths, complete)`: `complete = false` if any directory could
    not be read (permission error); on partial failure, include the paths
    that *were* collected and mark incomplete.
  - Does **not** follow symlinks (avoid loops / leaking outside the root).
  - Does **not** read file contents — just enumerates paths.

### 4. `--discover` flag in `sync` arm

- `cli/src/main.rs` `sync` arm (`~1054`): add
  `let discover = take_bool_flag(&mut args, "--discover");` before
  `sync_files`. When `--discover` is set:
  1. For each provider in `provider_registry()`, call `provider_data_root`
     + `discover_provider_sources`.
  2. Union all discovered paths into one `Vec<String>`.
  3. Query prior paths per provider via
     `store.source_paths_for_provider(provider_id)`.
  4. For prior paths no longer on disk, synthesize empty `SourceBatch`
     (`entries = []`, `relation_complete = true`, `fingerprint = None`).
     **Only** when the walk for that provider was complete; a partial walk
     skips synthesis (no tombstones — R2).
  5. Feed discovered paths into `sync_files` (or a shared helper). Relax
     the directory-rejection guard at `~1911` for the discover path
     (discovery already enumerated files, not directories).
- Update `subcommand_help_text` (`~737`) and top-level `help_text`
  (`~549`) with `--discover`. Register `--discover` in every prefix
  scanner that skips value-bearing flags (`command_name`, request-id /
  output-mode scans) so a value can't hide a later `--robot`/`--output`.

### 5. Result envelope

- `sync_files` result JSON gains a `discovery` object when `--discover`:
  `{ "complete": bool, "providers": [{ "id": "...", "found": N,
  "removed": M }] }`. Absolute paths are never included (privacy).
- Robot/MCP: no new tool. `--discover` is CLI-only for v1 (the MCP
  `sync` equivalent, if any, is a separate decision).

### 6. Tests

- `cli/tests/e2e.rs`:
  - `sync_discover_finds_and_syncs_provider_sources` — temp dir structured
    as `~/.claude/projects/x.jsonl` + `~/.codex/sessions/y.jsonl`, run
    `sync --discover`, assert both ingested, `discovery.complete == true`.
  - `sync_discover_tombstones_removed_source_on_complete_scan` — sync a
    source, delete the file, re-run `sync --discover`, assert the source's
    messages are gone (tombstoned).
  - `sync_discover_partial_scan_does_not_tombstone` — make a subdir
    unreadable, run `sync --discover`, assert `discovery.complete == false`
    and unseen sources are retained.
  - `sync_discover_re_runs_converge` — second run with no changes asserts
    no new generation (unchanged-skip path).
- No real transcript paths in fixtures (privacy); use synthetic content.

## Validation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --release`

## Review gates

- `trellis-check` after edits.
- Verify: no absolute transcript path in any result/error/progress frame;
  schema v9 migration non-destructive; `--discover` registered in all
  prefix scanners; directory guard relaxed only for the discover path.
- Do NOT commit/push.
