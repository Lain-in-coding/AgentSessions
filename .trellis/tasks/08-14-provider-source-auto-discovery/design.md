# Provider source auto-discovery — Design

## Context

`sync` today accepts an explicit `paths: &[String]` slice. There is no way
to tell the tool "find my Claude Code and Codex transcripts under their
canonical data roots." This task adds `sync --discover`, a provider-aware
root scan that locates each provider's JSONL Sources under the current
user's known provider data roots.

Research (`research/2026-08-14-discovery-scope-audit.md`) established that:

- No home-dir → provider-data-root resolver exists. `platform_paths_impl`
  (`main.rs:465-526`) resolves *this tool's* data root, not provider roots.
- `sync_files` (`main.rs:1898`) is explicit-path only; the `SourceDiscovery`
  port (`ports/src/lib.rs:64`) has only test stubs, no production impl.
- `ProviderAdapter` (`ports/src/lib.rs:631`) exposes `provider_id()` but
  no `data_root()`.
- The tombstone derivation (`lib.rs:2161-2198`) already satisfies R2
  (incomplete scan = no tombstone) via `relation_complete`; no store change
  is needed there.
- `capture` (`source_fs.rs:32`) fails on a *missing* file; the empty-file
  path in `stage_with_registry` (`main.rs:1503`) handles 0-byte files but
  not missing ones.

## Decisions

### D1 — Root resolution stays CLI-local (no trait change)

Add `data_root(provider_id) -> Option<PathBuf>` as a **CLI-local** function
in `main.rs`, mirroring the home-dir logic in `platform_paths_impl`. Do
**not** add a `data_root()` method to `ProviderAdapter` — that would force
every future provider to implement home-dir resolution even when it has no
canonical root, and would leak filesystem assumptions into the provider
crate (which is currently path-agnostic by design).

Rationale: the provider crates know *how to parse* a transcript; the
composition root knows *where to find* them. Keeping root resolution in the
CLI preserves the hexagonal boundary: provider crates stay portable, and
the composition root remains the single place that binds "which providers"
to "where their data lives."

### D2 — Discovery returns `Vec<String>`, feeds existing `sync_files`

Discovery walks each provider root recursively, collects `.jsonl` file
paths, and hands the resulting `Vec<String>` into the existing `sync_files`
pipeline. We do **not** introduce a production `SourceDiscovery` impl or
route through the port — that would be a larger abstraction than the task
needs, and `sync_files` already owns the capture/fingerprint/skip/commit
flow. Discovery's only job is to produce the path list.

### D3 — Deleted sources via prior-path diff (complete-scan only)

To tombstone a source that was deleted from disk (not just emptied),
discovery must:

1. Enumerate **prior** source paths from the store, filtered by the
   provider namespace being scanned. New store method:
   `source_paths_for_provider(provider_id) -> PortResult<Vec<String>>`,
   joining `source_scans` against `source_session_resume_claims`'s
   `provider_id` (or, if cleaner, a `provider_id` column on `source_scans`
   itself — see D4).
2. Diff prior paths against discovered paths.
3. For each prior path no longer on disk, synthesize an empty `SourceBatch`
   (`entries = []`, `relation_complete = true`) — the existing tombstone
   path then removes its catalog entries.

This only happens when the root walk completed without error. A partial
walk (permission error mid-scan) sets `relation_complete = false` on the
affected sources and emits **no** synthetic empty batches, so unseen
sources are never tombstoned — exactly R2.

### D4 — `provider_id` on `source_scans` (schema v9, additive)

`source_scans` currently has no `provider_id` column. To diff prior paths
per provider without joining through `resume_claims` (which would miss
sources that never had a resume claim), add `provider_id TEXT` to
`source_scans` (nullable for migrated rows; backfilled on next re-scan).
Bump `PRAGMA user_version` 8 → 9 with a non-destructive `ALTER TABLE ...
ADD COLUMN` migration.

Alternative considered: join `source_scans` to `resume_claims` on
`source_path`. Rejected because sources without resume metadata (legacy,
unresolved) would be invisible to the diff and never tombstoned.

### D5 — Root completeness recorded, not a new schema field

Whether a discovery run was complete is encoded in the
`relation_complete` flag already on each `SourceBatch` — no new field.
The CLI reports `discovery: {complete: bool, providers: [...]}` in the
sync result JSON for human/robot transparency.

## Boundaries

- **Read-only**: discovery never writes, moves, renames, or deletes a
  provider source file. `capture` + `verify_snapshot` enforce read-only
  with pre/post fingerprint checks (unchanged).
- **No path leakage**: discovered paths are internal; the result JSON
  reports counts and provider IDs only, never absolute transcript paths
  (consistent with the privacy contract).
- **Explicit**: `--discover` is opt-in. Plain `sync` and `search` never
  discover implicitly.

## Compatibility

- Additive CLI flag only. Existing `sync <paths...>` behavior unchanged.
- Schema v9 migration is additive (`ADD COLUMN`); v8 catalogs migrate
  forward; `provider_id` backfills to NULL until re-scanned.
- `list_providers` MCP tool unchanged (still reads `provider_registry`).

## Rollout / rollback

- Rollback: revert the `--discover` flag + resolver + store method. The
  schema v9 column is harmless if unused (nullable, additive). A v9→v8
  downgrade is not required for correctness; the column is ignored by v8
  code paths.
- Rollout sequence: schema migration first, then CLI flag, then tests.
