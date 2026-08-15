# Historical Session Discovery & Resume Metadata — Planning Record

> Scope: product planning that is now settled. This is NOT an implementation
> task. It records the confirmed decisions for the implementation phase that
> will follow only after explicit owner approval.

## Relation to other active tasks (boundaries)

- `08-13-search-match-guidance` (`why_matched` + `suggested_next_commands`):
  an **already-implemented, in_progress** sibling feature. It is a separate
  scope; do not fold Resume Metadata into it. Its suggested commands must not
  be silently changed by Resume; Resume availability can be an additive
  conditional hint only.
- `08-13-competitor-borrowings`: the umbrella for filters/noise/aggregation/
  around/guidance. It is the integration parent for the already-landed streams
  (provider/time filters, get_message, context levels, guidance). Resume
  Metadata is a NEW feature beyond this umbrella.
- `08-13-hardening-backlog`: owns MCP R2.1/R2.2/R2.3 (initialize gate, jsonrpc
  validation) plus perf P1s and evidence/install script fixes. The MCP
  hardening prerequisites in Phase 2 below partially overlap with this task;
  coordinate so they are not done twice. `08-13-ux-review-fixes` must land
  first (same shared files).
- `08-14-session-resume-metadata` (this dir): planning only; no Trellis task
  has been created yet, and there is no prd/design/implement/task.json. Create
  them at implementation start, after owner approval, and only then run
  `task.py start`.

## Current repository / integration state (snapshot)

- Worktree: `C:/AgentSessions/.claude/worktrees/integration-08-13-four-features-v2`,
  branch-based, 29 modified + 4 untracked paths on top of commit `f4175a0`.
- Uncommitted production work in this worktree includes:
  - Fixes for 8 confirmed review findings (see below);
  - the 4-feature integration (filters, get_message, context levels, guidance);
  - `guidance.rs` (new);
  - `docs/adr/ADR-0009-session-resume-metadata.md` (new);
  - `.codegraph/` index (new).
- No commit has been made this session; all commits require explicit owner
  instruction (CLAUDE.md).

### Already-fixed review findings this session (8, in worktree, not yet committed)

1. `group_by_session` pagination infinite loop (high) — added an empty-hits
   guard for `has_more`.
2. `smoke.sh` doctor duplicated `--db` (high) — removed the duplicate.
3. `split_zone` byte-slice panic (medium) — added `is_char_boundary` guards.
4. TUI result list had no scrolling (medium) — switched to ratatui `ListState`
   + `render_stateful_widget`.
5. `parse_instant` overflow (low) — checked arithmetic.
6. `parse_output_mode` skip list missed new value flags (low).
7. `--db ""` empty path (low) — rejected as usage error.
8. TUI raw-mode leak on init failure (low) — added `ratatui::restore()`.

## Goal

Let a user or an agent find an old conversation via full-text search and see,
directly in the search result table, everything needed to continue that
session in its provider: **Provider**, **native Provider Session ID**, and the
session's **working directory**. Search and resume work through the same
read-only tool; no command is executed and no terminal is launched.

## Confirmed product contract

### Dual IDs

- Keep canonical `session_id` (`ses_v1_*`) for catalog association and
  navigation. Do not overload or replace it with the provider-native ID.
- Persist exact, nullable `provider_session_id` separately for provider
  resume.
- Add `provider_id` so an agent knows which provider command applies.

### Path semantics

- Return the **Original Working Directory** recorded by the provider. Do not
  derive it from transcript path, transcript directory, repository root, or
  source locator.
- `original_working_directory` may be missing or stale; that is metadata, not
  an execution or liveness guarantee.
- Never expose transcript/source path in ordinary responses.

### Search and resume

- Search returns `resume_available` for a session.
- An explicit `get_session_resume(session_id)` returns canonical session ID,
  provider ID, provider session ID, original working directory, availability,
  and unavailable reason.
- Fields are always present; unknown/ambiguous values are `null`, never
  guessed or omitted. History stays searchable even when not resumable.

### First version capability

- Return structured data only. Do not build a shell command string, do not
  execute a command, do not launch a terminal, do not auto-copy.

### Conflict avoidance from the source

- Derive canonical Session ID from Provider + installation namespace +
  Provider Session ID to prevent cross-provider collisions.
- Do not try to recover a native ID by stripping `ses_v1_`.
- Accept `cwd` only from an authoritative provider field.
- Keep the association between ID and cwd; never pair independently observed
  values.
- Do not use "first ID wins" when a single source carries multiple Session
  IDs: split when reliable, otherwise keep history but do not emit any
  potentially wrong ID/cwd.
- Treat a session's normal directory change as the earliest authoritative
  cwd, not as a conflict.
- Only genuinely ambiguous damage surfaces as `—` in the table; diagnostics
  live in machine state and sync logs.

## Confirmed UX contract

### Human CLI output

- **One horizontal table**; no resume-command block, no long per-session
  explanation. Markdown only in the assistant-facing Skill, not as Robot/MCP
  protocol data.
- Columns and order:

  | 日期 | Provider | 会话标题 | 工作目录 | Session ID |
  |---|---|---|---|---|

- Date is **YYYY-MM-DD local**; header is "日期", not "时间".
- Provider and Session ID are always complete, never truncated.
- Title and working directory compress only in the human table; Robot/MCP
  carry the full values.
- Missing fields uniformly render as `—` (including Session ID), with no
  extra explanation.
- **Horizontal preferred; vertical layout is not used as a fallback.** If the
  fixed columns themselves cannot fit, the table keeps full rows; the
  terminal may wrap but the tool does not truncate IDs or switch layout.

### Search result behavior

- Show **all** matched sessions (deduplicated by Canonical Session), not a
  top-N cap. Pagination transmits blocks; it does not hide rows.
- Relevance first; most-recent-activity as tiebreak. Empty/browse queries
  sort by most-recent-activity.
- The agent automatically resolves metadata for the page (batched, no N+1),
  so the table appears directly; no extra user confirmation is required.
- Recommended column-width priority: date 10 cols full; Provider full;
  Session ID full; remaining ~35% title / ~65% working directory; title
  truncates from the end; working directory collapses from the middle
  (`C:/…/agent-session-grep`); newlines/tabs become spaces; `—` for missing.
- Title source priority: provider custom title → first valid user request →
  bounded summary → `—`. Never invented.
- Date/time: Human table uses local date; Robot/MCP keep full RFC3339.

### Entry surfaces

- Human CLI: responsive horizontal table.
- TUI: adaptive columns and a detail panel.
- Robot JSON/JSONL and MCP: full structured values, no Markdown.
- Project Skill: instruct agents to render MCP/Robot results as the same
  table.

## External research conclusions (CC-Switch, provider formats)

### CC-Switch (farion1231/cc-switch @ f748f3a, verified against upstream)

- Product model worth borrowing: `provider_id` + native `session_id` + optional
  `projectDir/cwd` + provider-specific resume capability; nullable metadata
  that differs per provider; resume via `claude --resume <id>` /
  `codex resume <id>`.
- Its search is an in-memory FlexSearch over bounded metadata/snippets, NOT a
  persistent transcript FTS index. Do not copy its scan model.
- Do NOT copy: raw shell `resumeCommand` strings; `providerId:sessionId:
  sourcePath` as identity; automatic terminal launch; stale-cwd handling
  (current code does not validate the directory despite docs claiming a picker);
  deletion of provider sessions (violates this project's source read-only
  contract).
- Session search covers ID/title/summary/projectDir/sourcePath; list rows show
  only title + activity time (no "why this matched" for metadata-only hits).

### Provider formats (verified in this worktree)

- Claude Code: top-level `sessionId` (native), top-level `cwd` present in
  fixtures but **currently discarded** by `RawLine`. First nonempty ID wins;
  later distinct IDs only produce a diagnostic.
- Codex: `session_meta.payload.session_id`; `session_meta.payload.cwd` present
  in fixtures but **currently discarded** by `RawPayload`. `turn_context.cwd`
  exists but is turn-scoped — do NOT use it as the working directory.
- Both adapters: missing → `None`; multi-session file → first ID wins today
  (must change to fail-closed for resume metadata).
- Current `ParseReport` has `session_native_id: Option<String>`; the raw value
  is consumed into `StableId` at composition and not separately persisted.

## Adversarial review conclusions (all read-only; none changed code)

### Key design risks confirmed (from the multi-agent review)

- **Do not overclaim**: `ses_v1_*` is a stable catalog identity, NOT a
  provider resume token. Never advertise it as such.
- **Provider Session ID is nullable**: it must stay `null` when absent; never
  reconstruct from filename/transcript dir/ses prefix/document ID/cwd.
- **Multi-document sessions are real**: observed real data had a Session
  contributed by 55 Sources; a Message can belong to 3 Sessions. A single
  scalar "folder" is insufficient; handle multiplicity, do not silently pick.
- **Search's `session_id` is a deterministic selected session** (MIN), not the
  only owner; provider metadata on a hit must describe that selected session.
- **Path privacy**: absolute source paths currently stored in SQLite; public
  DTOs are path-free. Exposing working dir or native ID is a NEW disclosure
  decision; keep it explicit and opt-in.
- **Path representation**: `to_string_lossy` is lossy; no canonicalization;
  equivalent spellings are distinct keys; do not promise lossless
  round-tripping from the current string.
- **Budget**: added fields must be charged by JSON-escaped serialized length;
  MCP duplicates payload in `structuredContent` + `content[0].text` (final
  frame may be ~2x); long Windows paths inflate escaping.
- **Schema version**: current `SCHEMA_VERSION` is hard-coded `1.0`; adding
  fields while keeping `1.0` repeats the existing incompatibility (runtime
  `occurrences` is already absent from the closed schema). Publish 1.1.
- **Grouping semantics**: do not change grouping to use provider Session ID;
  keep canonical `session_id` as grouping key.

### Alternative contract shapes considered (for the implementer)

- **A. Typed, plural Session metadata** (preferred): keep canonical
  `session_id`; add `providers:[{provider_id, provider_session_id}]`,
  `original_working_directories:[]`; arrays always present, empty when
  unknown; no silent selection.
- **B. Required nullable scalar + ambiguity/count fields**: smallest wire
  footprint but weakest fidelity; needs candidate counts to be honest.
- **C. Response-level session dictionary** for search (dedupe per-session
  metadata, avoid per-hit repetition).
- **D. Surface-specific placement**: search/grouped/context/list each attach
  metadata once where appropriate.
- **E. Store metadata in Session payloads**: convenient but bypasses the
  dedicated read port; re-sync needed; opaque payload risk.
- **F. Separate/opt-in source-location disclosure**: keep working-dir/native
  ID but gate full source paths behind explicit opt-in or a separate resolver.
- **G. Robot 1.1 additive vs 2.0 structural**: 1.1 preferred; 2.0 only if a
  nested/response-level shape requires exact-shape client protection.

## Prerequisite defects to fix first (from audit)

1. Provider/installation namespace for canonical Session identity, plus
   migration; multi-ID fail-closed instead of first-wins.
2. Robot schema evolution (1.1) including `occurrences`.
3. `list` vs `list_sessions` cursor result-set binding.
4. MCP JSON-RPC/envelope validation (jsonrpc, id, initialize, notification,
   params), budget-floor validation at protocol layer (`max_bytes`<4096,
   `max_messages`<1), bounded error messages, dual-carrier
   (structuredContent + content.text) byte accounting, and 7- vs 6-tool doc
   drift.

## Implementation checklist (comprehensive; the full work to be done)

### Phase 0 — Close current integration and confirm baseline
- [ ] Run the full quality gate in `integration-08-13-four-features-v2`:
      `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`, `cargo build --release`.
- [ ] Confirm which `08-13` tasks are archived vs still `in_progress`; record
      the current integration state so the Resume feature starts from a clean,
      known baseline.
- [ ] Note the integration worktree currently has 29 modified + 4 untracked
      paths (including `guidance.rs`, `ADR-0009`, `.codegraph/`) and is on top
      of commit `f4175a0`. Decide (with owner) whether to commit these or start
      the Resume feature in a fresh worktree after a merge.
- [ ] Do NOT run any `git commit`/`git push` without explicit owner
      instruction (CLAUDE.md rule).

### Phase 1 — Contract and terminology (ADR + glossary + schema)
- [ ] Keep canonical `session_id` (`ses_v1_*`) and do not overload or replace
      it with the provider-native ID.
- [ ] Persist exact, nullable `provider_session_id` separately.
- [ ] Add `provider_id` alongside it.
- [ ] Return the **Original Working Directory** recorded by the provider, and
      do not derive it from transcript path, transcript directory, repository
      root, or source locator.
- [ ] Document that `original_working_directory` is metadata, may be missing or
      stale, and is not an execution or liveness guarantee.
- [ ] Never expose transcript/source path in ordinary responses.
- [ ] `ADR-0009-session-resume-metadata.md` is `Proposed`; update it to
      `Accepted` when the contract is frozen and record final UX decisions.
- [ ] Robot schema evolution: publish `1.1`, include `occurrences`, declare all
      additive fields, advance the runtime `SCHEMA_VERSION`, keep old `1.0`
      frozen.
- [ ] Document that Markdown tables are assistant-facing rendering only, not
      Robot/MCP protocol data.

### Phase 2 — Prerequisite defects (fix first, before Resume metadata)
- [ ] **Identity namespace**: derive canonical Session ID from
      Provider + installation namespace + Provider Session ID; add migration;
      multi-Session-ID sources fail closed instead of "first ID wins".
- [ ] **Do not recover native ID from `ses_v1_*`** (stripping the prefix is
      lossy and unreliable).
- [ ] **list vs list_sessions cursor binding**: bind cursors to a result-set
      discriminator (`sessions_only`) so a cursor minted over one sequence is
      rejected against the other.
- [ ] **MCP JSON-RPC/envelope validation**:
      - validate `jsonrpc == "2.0"` on every request;
      - strictly validate `id` type (no array/object/fractional/missing);
      - validate `initialize` params (`protocolVersion`, `capabilities`,
        `clientInfo`);
      - a failed `initialize` must NOT open the gate via
        `notifications/initialized`;
      - validate `ping`/`tools/list` params shape;
      - validate `notifications/initialized` jsonrpc + params.
- [ ] **MCP budget-floor validation at protocol layer**: reject
      `max_bytes < 4096`, `max_messages < 1`, `max_items < 1`, `limit < 1` with
      JSON-RPC `-32602` before constructing `AppRequest`; Application budget
      validation stays as defense in depth.
- [ ] **MCP bounded error messages**: bound unknown-method/tool/ID/param value
      interpolation; add string `maxLength`/array `maxItems` to input schemas.
- [ ] **MCP dual-carrier byte accounting**: the same payload is emitted in
      `structuredContent` AND `content[0].text`; decide and test whether
      `max_bytes` means logical payload or full MCP frame; add floor-minus-one
      tests for every affected tool.
- [ ] **7- vs 6-tool doc drift**: CONTRACT and SKILL currently list 6 tools and
      omit `get_message`; update them to 7.

### Phase 3 — Provider metadata extraction (parallel, independent)
- [ ] **Claude Code**: parse and retain top-level `sessionId`; capture `cwd`
      (currently discarded); missing/blank → `None`; multi-session file → emit
      bounded diagnostic, no first-wins as resumable metadata.
- [ ] **Codex**: retain `session_meta.payload.session_id`; capture
      `session_meta.payload.cwd`; ignore `turn_context.cwd` for the working
      directory; missing/blank → `None`; multi-session → diagnostic, no
      first-wins.
- [ ] Preserve the association between ID and cwd; never pair independently
      observed values across sources.
- [ ] Fixture/property tests cover missing, blank, repeated, conflicting,
      multi-session, and privacy-safe diagnostics for both providers.

### Phase 4 — Domain/ports + persistence (SQLite migration)
- [ ] Add typed metadata structures to domain/ports (separate from canonical
      Session payload; do not put sensitive fields into the opaque Session
      payload).
- [ ] Persist resume claims source-scoped; replace atomically within the same
      transaction/generation as source replacement; source removal clears
      claims.
- [ ] Add a batched read path (no N+1) for resolving availability across
      search page hits.
- [ ] SQLite schema bump + migration; legacy catalogs initially return
      `resume_available:false` / `null`; re-sync backfills even when bytes are
      unchanged (metadata projection revision guard).
- [ ] Aggregation is order-independent and fails closed on conflict; a
      session's normal directory change is the earliest authoritative cwd, not
      a conflict.

### Phase 5 — Application use cases + budgets
- [ ] `AppRequest::GetSessionResume` + `AppResponse::SessionResume`.
- [ ] Search hits gain `resume_available`; batched, one lookup per page.
- [ ] All new fields and arrays charged to `max_response_bytes` (JSON-escaped
      byte length, not raw chars).
- [ ] Ambiguity/count states preserved in structured data; table shows `—`.
- [ ] History stays searchable even when not resumable.

### Phase 6 — Entry surfaces
- [ ] **Human CLI**: responsive horizontal table; columns and order
      `日期 | Provider | 会话标题 | 工作目录 | Session ID`; date is
      `YYYY-MM-DD` local; Provider and Session ID never truncated; title and
      working directory compress (title from end, directory from middle);
      missing fields uniformly `—`; horizontal preferred, no vertical fallback.
- [ ] **TUI**: adaptive columns + detail panel; same field semantics.
- [ ] **Robot JSON/JSONL**: full structured values, no Markdown.
- [ ] **MCP**: new read-only `get_session_resume` tool; validate canonical
      `ses_v1_*` kind; bounded params; update tool list/descriptions.
- [ ] **Skill**: instruct agents to render MCP/Robot results as the same table;
      document the two-step auto-resolution (search → batched metadata → table).

### Phase 7 — Final verification
- [ ] All quality gates green; Gate D full-corpus re-run green.
- [ ] Windows/WSL paths, deleted/moved worktrees, copied sources, multi-root
      sessions, ID collisions, multi-Session transcript all have tests.
- [ ] Provider source files read-only; checksums unchanged before/after.
- [ ] Privacy: raw IDs/paths do not leak into search, diagnostics, progress,
      logs, or errors.
- [ ] Final review; commit only on explicit owner instruction; never push/merge
      automatically.
