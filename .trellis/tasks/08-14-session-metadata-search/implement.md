# Session Metadata Search — Implementation

## Preconditions

- `task.py start` this task before editing.
- Work in the task's isolated worktree.
- Depends on `source_session_resume_claims` (schema v8) being present.
- Research: `research/2026-08-14-status-audit.md` (gap confirmation).
- Design: `design.md` (decisions D1-D5).

## Steps

### 1. Schema v10 — `session_fts` table

- `adapters-sqlite/src/lib.rs`: add `CREATE VIRTUAL TABLE IF NOT EXISTS
  session_fts USING fts5(session_wire UNINDEXED, text);` in the migration
  chain. Bump `PRAGMA user_version` to 10 (carry v9 `source_scans.
  provider_id` if auto-discovery hasn't landed; otherwise v9→v10).
- Add a `session_fts_ids` sidecar mirroring `fts_ids` (wire_id ↔ fts_rowid)
  for O(1) delete-by-rowid.
- Migration is additive; existing v8/v9 catalogs migrate forward.

### 2. Build session-FTS text (privacy boundary)

- New `fn session_searchable_text(store, session_wire) -> String`:
  - Resolve resume metadata via the existing `resume_of` path (or a direct
    claim read). Include `provider_session_id` only when state=resolved;
    `original_working_directory` only when `pair_observed=true`.
  - Derive title from the first chronological user message of the session
    (deterministic query against `message_placements` + catalog payload,
    bounded length). If none, omit the title slot.
  - Space-join all included fields. Never include source_path.
- Apply `bigram_cjk` at query time (same as message path), not at index
  time — keep the index raw text for rebuild simplicity.

### 3. Populate + maintain `session_fts`

- On `commit_source_batches_if_changed`, after claims are written, rebuild
  the affected sessions' `session_fts` rows: delete by wire_id (rowid via
  sidecar), insert fresh `session_searchable_text`.
- On `rebuild` (catalog rebuild), repopulate `session_fts` entirely from
  claims + catalog — fully rebuildable (AC: rebuild stable).

### 4. Adapter-internal merge in `search_filtered`

- `adapters-sqlite/src/lib.rs` `search_filtered` (line ~4925): after the
  existing message-FTS query, run a second `session_fts MATCH` with the
  same `safe_fts_query`. Collect matching `session_wire` ids.
- For each session-metadata-only match (not already represented by a matching non-system Message hit), pick the deterministic first non-system Message of that session as the representative. If no non-system Message exists, return the canonical Session identity directly as a metadata-only hit.
- Merge message hits and metadata hits without changing the existing mode contract: default search remains Message-grained; `group_by_session=true` performs canonical Session dedup and sets `occurrences`. A matching system/developer Message alone must not suppress a metadata-only Session that the Application will later hide by default.
- The merged list then flows through the existing limit/budget/cursor path unchanged — `assemble_resume_availability` (AC3) and budgets (AC4) need no change.

### 5. Tests

- `adapters-sqlite` unit:
  - `session_fts_indexes_provider_session_id_and_cwd` — ingest a session
    with a known native id + cwd, assert `search "native-id"` and `search
    "cwd-dir"` both return that session.
  - `session_fts_suppresses_cwd_when_pair_not_observed` — pair_observed=false
    cwd must not match.
  - `session_fts_never_indexes_source_path` — ingest, assert searching any
    substring of the source path returns nothing (AC2).
  - `session_metadata_and_message_hits_dedup_by_session` — a token matching
    both a message body and a session native id returns the session once.
  - `session_fts_rebuild_is_stable` — rebuild, assert same results.
- `cli/tests/e2e.rs`:
  - `search_by_provider_session_id_returns_session` — end-to-end.
  - `search_by_working_directory_returns_session`.
- No real transcript paths in fixtures.

## Validation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --release`

## Review gates

- `trellis-check` after edits.
- Verify: no source/transcript path in `session_fts.text` or any result;
  `pair_observed=false` cwd suppressed in index; rebuild stable; dedup by
  Session; budget path unchanged; message-only queries behave identically
  when no metadata matches.
- Do NOT commit/push.
