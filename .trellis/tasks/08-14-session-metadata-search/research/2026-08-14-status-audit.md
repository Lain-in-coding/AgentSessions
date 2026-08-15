# Research: 08-14-session-metadata-search status audit

- **Query**: Confirm current implementation status of the `08-14-session-metadata-search` task — what is done, what is not.
- **Scope**: internal
- **Date**: 2026-08-14

## Task metadata

- `task.json` status: `planning`; `worktree_path: null`; `branch: null`; `completedAt: null`.
- The task directory (`C:/AgentSessions/.trellis/tasks/08-14-session-metadata-search/`) contains only `prd.md`, `task.json`, and the empty `implement.jsonl` / `check.jsonl` scaffolds. No `design.md` or `implement.md` exists for this task.
- The referenced `implement.md` roadmap lives in the sibling task `08-14-resume-protocol-prerequisites` research file (`2026-08-14-audit-convergence.md`), which describes "Step 1 of roadmap" — the Human table real date + title work. That step belongs to the umbrella parent (`08-14-historical-session-discovery-resume`), not specifically to this metadata-search child task.
- Worktree under audit: `C:/AgentSessions/.claude/worktrees/integration-08-13-four-features-v2`.

## PRD acceptance criteria vs. implementation

The PRD (`prd.md`) lists five acceptance criteria. Below each is mapped to the actual code in the worktree.

### AC1 — Metadata-only queries return Sessions by Provider Session ID, title, summary, and working directory

**Status: NOT DONE (the search-indexing half is absent).**

- The FTS5 `searchable_text` function (`crates/agent-session-grep-adapters-sqlite/src/lib.rs:103-120`) extracts ONLY the message payload's `text` field for indexing. There is no code path that indexes Provider Session ID, Session title, structural summary, or Original Working Directory into FTS.
- Grep for any FTS/MATCH interaction with `provider_session_id`, `title`, `working_directory`, or `summary` columns returned **no matches** in the sqlite adapter.
- The `SearchIndex` port trait (`crates/agent-session-grep-ports/src/lib.rs:266`) exposes `query_filtered(query: SearchQuery, limit)` — the query is a literal string matched against FTS text only. No metadata-query method exists on the port.
- What IS done: Resume Metadata (Provider Session ID + Original Working Directory) is **persisted** in `source_session_resume_claims` (schema v8) and **resolved** via `resume_of` (ADR-0009). But it is only *attached to results*, never *searched*.

Conclusion: the metadata is stored and resolvable, but it is NOT queryable. A user typing a Provider Session ID, a title, or a working directory into `search` will get zero hits unless that string also appears in a message body. AC1 is unmet.

### AC2 — No Source path is indexed, returned, or diagnosable

**Status: DONE (confirmed by privacy audit).**

- The `2026-08-14-audit-convergence.md` privacy-contract-audit concluded "ALL PASS": no transcript/source path exposure, resume metadata isolated from FTS/payload/diagnostics/progress/errors.
- `searchable_text` indexes only `text`, never `source_path`.
- Resume metadata (`SessionResumeMetadata`) exposes only `session_id`, `provider_id`, `resume_available`, `provider_session_id`, `original_working_directory`, `unavailable_reason` — no source locator.

### AC3 — Session hits deduplicate and join with `resume_available`

**Status: DONE.**

- `assemble_resume_availability` (`crates/agent-session-grep-application/src/lib.rs:1331-1364`) batch-resolves `resume_of` for all page hits and maps `resume_available` back onto each `SearchHit`.
- The grouped/per-session collapse path also carries `resume_available` (test `search_group_by_session_carries_resume_availability`).
- Deduplication by Canonical Session is handled by the existing grouped-search path (`occurrences` counts per-session).

### AC4 — Budgets enforce JSON-escaped byte accounting for new fields

**Status: PARTIALLY DONE.**

- `RESUME_AVAILABLE_FIELD_BYTES` (24 bytes) is reserved in the budget calculation (`crates/agent-session-grep-application/src/lib.rs:40`), so the `resume_available` boolean field is accounted for.
- However, the PRD's "new metadata fields" in the context of AC1 (title, summary, working directory as *searchable* fields) have no budget accounting because they are not indexed/searched at all.
- The Human-mode `session_resume_rows` projection (date/title/working_directory/session_id) is a display-only projection that does not enter the machine-mode byte budget — by design (Human-mode only). This is consistent with the spec note "No port/DTO/schema change; Robot/MCP output unchanged."

### AC5 — fmt, clippy `-D warnings`, test workspace, and release build are green

**Status: DONE (as of the audit convergence file).**

- The `2026-08-14-audit-convergence.md` records full workspace gate GREEN in the worktree (`CARGO_TARGET_DIR=C:/AgentSessions/target-resume-integration`): fmt pass, clippy pass, test pass (SQLite 135, Application 133, CLI 181, Claude 38, Codex 33, MCP 37, E2E 89), release build pass.

## Step-1 roadmap item (referenced in the task prompt)

The task prompt asked to confirm whether "Step 1 — wire real `date_ymd` and `title` into `attach_session_resume_rows` at `cli/src/main.rs:1435`" is done.

**Status: DONE.**

### `attach_session_resume_rows`

- File: `C:/AgentSessions/.claude/worktrees/integration-08-13-four-features-v2/crates/agent-session-grep-cli/src/main.rs:1439-1486`.
- Called only in Human mode (`main.rs:1105-1107`).
- Collects distinct canonical Session IDs from page hits, calls `store.resume_of(&session_ids)` for resume metadata, then calls `store.latest_activity_ymd_for_sessions(&session_ids)` for the 日期 column.
- Title is taken from the highest-relevance hit's `text` field on the current page (first matching hit for each session wire id) — `main.rs:1463-1472`. Missing → rendered as `—` downstream.
- Emits `session_resume_rows` array with keys: `date`, `provider`, `title`, `working_directory`, `session_id` — consumed by `human::render_session_resume_table`.

### `latest_activity_ymd_for_sessions`

- File: `C:/AgentSessions/.claude/worktrees/integration-08-13-four-features-v2/crates/agent-session-grep-adapters-sqlite/src/lib.rs:4826-4861`.
- Batched `MAX(json_extract(catalog.payload, '$.timestamp'))` per canonical Session, truncated to `YYYY-MM-DD` (first 10 chars).
- Chunked via `chunk_ids` / `BATCH_IN_CHUNK` (no N+1).
- Returns `HashMap<String, String>` keyed by session wire id.

### Human table rendering

- File: `C:/AgentSessions/.claude/worktrees/integration-08-13-four-features-v2/crates/agent-session-grep-cli/src/human.rs`.
- `render_search` (line 62) checks for `session_resume_rows`; if present and non-empty, delegates to `render_session_resume_table` (line 471).
- `render_session_resume_table` renders the frozen five-column header `日期 | Provider | 会话标题 | 工作目录 | Session ID` with column-width budgeting (TABLE_TARGET_COLS=100; title 35% / cwd 65% of remaining; tail-ellipsis for title; middle-collapse for cwd).
- E2E test `snippet_renders_in_human_search_but_is_stripped_in_machine_modes` (`tests/e2e.rs:3009`) asserts the header, a real Provider row, real date (`2026-07-26`), and real title (`snippet vis`).

## Summary of remaining unimplemented items

| PRD item | Status | Evidence |
|---|---|---|
| AC1: Metadata-only queries (Provider Session ID / title / summary / working dir searchable) | **NOT DONE** | `searchable_text` indexes only message `text`; no FTS path for metadata fields; no port method for metadata search |
| AC2: No Source path indexed/returned/diagnosable | DONE | Privacy audit ALL PASS; `searchable_text` excludes paths |
| AC3: Deduplicate + join `resume_available` | DONE | `assemble_resume_availability` batch-joins; grouped path tested |
| AC4: Budget JSON-escaped byte accounting for new fields | PARTIAL | `resume_available` bytes reserved; other metadata-search fields absent (no search → no budget needed yet) |
| AC5: fmt/clippy/test/release green | DONE | Audit convergence file records full gate GREEN |

## Conclusion

The task `08-14-session-metadata-search` is **NOT complete**. The core deliverable — making Provider Session ID, title, summary, and working directory **queryable** (PRD R1 / AC1) — has not been implemented. What exists is the supporting infrastructure built under the sibling/umbrella resume work:

1. Resume Metadata is persisted and resolvable (`resume_of`, `source_session_resume_claims`).
2. `resume_available` is joined onto search hits (AC3 done).
3. The Human-mode five-column table renders real date + title (Step 1 roadmap done).
4. Source-path safety is verified (AC2 done).

But the FTS index still only covers message `text`. There is no port method, adapter query, or test that demonstrates searching by Provider Session ID, title, summary, or working directory. The task remains in `planning` status with no `design.md` or `implement.md` of its own.
