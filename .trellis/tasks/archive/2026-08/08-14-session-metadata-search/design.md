# Session Metadata Search — Design

## Context

PRD R1 requires that a query token may match the Provider Session ID,
Session title, structural summary, or Original Working Directory — not just
message bodies. The status audit (`research/2026-08-14-status-audit.md`)
confirmed the core gap: `searchable_text` (`adapters-sqlite/src/lib.rs:103`)
indexes ONLY message payload `text`. Resume Metadata is persisted and
resolvable (`resume_of`, `source_session_resume_claims`) but is never
*searchable*. AC2 (no source path), AC3 (dedup + resume_available join),
and AC5 (quality gate) are already done.

## Decisions

### D1 — Separate session-FTS table, not extending the message FTS

Add a second FTS5 table `session_fts(session_wire UNINDEXED, text)` that
indexes the *concatenated searchable metadata* of each canonical Session,
derived from `source_session_resume_claims` (provider_session_id,
original_working_directory) and the session title/summary projection.

Rationale: the existing `fts` table is message-grained and its `bm25`
ranking, `fts_ids` rowid sidecar, and conflict-authority rules are all
message-shaped. Mixing session metadata into the same `text` column would
pollute message ranking and break the catalog-as-truth rebuild invariant
(session metadata is a derived projection, not a message fact). A separate
table keeps the message FTS untouched and makes session metadata a clean,
rebuildable projection from the catalog + claims.

### D2 — UNION message hits and session-metadata hits with mode-aware dedup

`search_filtered` runs two queries and merges:
1. Existing message-FTS path (`fts MATCH`) — message-grained results remain
   available for the default search contract.
2. New session-FTS path (`session_fts MATCH`) — returns canonical Session
   wires; for each metadata-only match, the deterministic first non-system
   Message is used as the representative when one exists. A Session with
   only system/developer Messages is not suppressed by that representative
   check; it remains available as a metadata-only Session hit.

The adapter removes a metadata candidate only when the same canonical Session
already has a matching non-system Message hit. The Application layer then
preserves the existing modes: default search is Message-grained, while
`group_by_session=true` performs canonical Session dedup and sets
`occurrences`. Merge ordering is deterministic by the backend score followed
by canonical entity wire ID; `fts` and `session_fts` scores are backend-local
and are not a cross-backend quality guarantee.

Rationale: this preserves the existing SearchHit and `group_by_session`
contract while making metadata-only Sessions discoverable and keeping the
existing resume-availability join unchanged downstream.

### D3 — Searchable metadata composition (privacy-preserving)

The `session_fts.text` column is the newline-joined concatenation of:
- `provider_session_id` (only when state = resolved)
- `original_working_directory` (only when `pair_observed` and state = resolved)
- the first chronological valid user request, as the current deterministic title-like projection

Provider-specific custom title and structural summary are intentionally omitted until
providers expose them through an authoritative Canonical contract. The index never
invents or scrapes opaque provider fields for these values.

**Never indexed**: `source_path`, transcript path, native IDs from other providers,
or anything outside the fixed privacy-filtered metadata shape. The build of
`session_fts.text` goes through the same privacy boundary as `resume_of` — no source
locator ever enters the index.

**Never indexed**: `source_path`, transcript path, native IDs from other
providers, or anything not in the fixed resume-metadata shape. The build of
`session_fts.text` goes through the same privacy boundary as
`resume_of` — no source locator ever enters the index.

### D4 — Rebuildable projection, schema v10 additive

`session_fts` is a rebuildable projection of the catalog + claims, exactly
like the message `fts`. A catalog rebuild (`rebuild`) populates it from
`source_session_resume_claims` joined to the session catalog. Bump
`PRAGMA user_version` 9 → 10 (after the v9 `source_scans.provider_id`
column from the auto-discovery task; if that task hasn't landed, v8 → v10
in one migration carrying both). Additive `CREATE VIRTUAL TABLE IF NOT
EXISTS` — no destructive change.

### D5 — Port surface: extend SearchIndex minimally

Add `query_sessions_filtered(query, limit)` to the `SearchIndex` port,
returning canonical Session wires. The Application search path calls both
`query_filtered` (messages) and `query_sessions_filtered` (sessions), then
merges. Alternatively, keep the merge inside the adapter behind the
existing `query_filtered` (returning message wires that belong to matching
sessions) — preferred, to avoid changing the port shape. Decision:
**adapter-internal merge** behind `query_filtered`, no new port method.
This keeps AC3's `assemble_resume_availability` unchanged.

## Boundaries

- No source/transcript path in `session_fts.text` or any result field (AC2).
- Session metadata is a projection: rebuildable, never authoritative.
- `pair_observed=false` cwd is suppressed from the index (matches the
  `resume_of` privacy rule).
- Budgets: the merged hit set still clamps to the response budget; a
  session-metadata match costs the same byte accounting as a message hit
  (it IS a message hit, just reached via a different index). AC4 is
  satisfied by reusing the existing budget path.

## Compatibility

- Additive FTS table + adapter-internal merge. Existing message-only
  queries behave identically when no session metadata matches.
- No port DTO change (D5 adapter-internal). No schema destructive change.
- Cursor result-set discriminator unchanged (still `search`).

## Rollout / rollback

- Schema v10 additive CREATE. Rollback: drop `session_fts`; message FTS
  unaffected.
- Depends on `source_session_resume_claims` (schema v8, already present in
  this worktree, currently uncommitted). Land after the Resume protocol
  commit, or in the same changeset if the commit hasn't happened yet.

## Open question (deferred)

Session title/summary projection: the Human table already derives a title
(highest-relevance hit `text`). For the *index*, a stable per-session
title is needed at index time — this requires a persisted title projection
or deriving it from the first user message at index time. **Decision**:
derive from the first chronological user message of the session at index
time (deterministic, no new persistence). If no user message exists, the
session's metadata-text omits the title slot; it can still match on
provider_session_id / cwd.
