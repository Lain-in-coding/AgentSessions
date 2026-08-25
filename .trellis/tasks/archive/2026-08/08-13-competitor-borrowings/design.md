# Design — Competitor borrowings P0

## Scope

This task is the integration parent for four independently implemented P0 streams:

1. provider and time search filters;
2. typed message lookup with an around window;
3. structural context levels with fallback and hints;
4. deterministic search-match explanations and next-command guidance.

System-noise filtering and group-by-session aggregation were initially left in
the parent backlog and not part of this execution wave; they have since been
implemented on this branch to close the parent PRD acceptance criteria
(post-filter on payload `role` in Application for R2; windowed group-by-session
with `occurrences` for R3 — both additive, default paths unchanged).

## Architectural boundaries

- `agent-session-grep-ports` names backend-independent search-filter and typed-placement capabilities. It never exposes SQL rows, `rusqlite`, or compatibility JSON aliases.
- `agent-session-grep-application` owns normalized requests, cursor binding, response budgets, ambiguity handling, structural summaries, hints, and deterministic guidance.
- `agent-session-grep-adapters-sqlite` pushes FTS and metadata predicates into one bounded query while preserving catalog authority and FTS as a rebuildable projection.
- `agent-session-grep-cli` keeps the hand-written parser, maps additive CLI/MCP parameters into `AppRequest`, and projects shared `AppResponse` values. MCP handlers do not duplicate search, context, ordering, or budget logic.

## Shared contracts

### Compatibility

Omitted parameters preserve the `fc2f2d4` behavior. Existing fields are not removed, human search output remains unchanged, raw context remains the default, ranking and total-order tiebreaks remain unchanged, and schema changes are additive.

### Budget and cursor integrity

Every added response string or collection is charged to `max_response_bytes`. Existing item/message limits still apply before the byte limit and truncation remains explicit. Normalized provider/time filters participate in the search cursor digest so a cursor cannot be replayed under different filters.

### Typed placement authority

Message windows and session disambiguation use `ContextGraphStore` placements and the selected mainline. They never infer authority from legacy `session`, `parent`, or `span` payload aliases. A stable message in several sessions is ambiguous unless the request names a session; the application never guesses.

## Stream designs

### 1. Provider and time filters

Introduce a normalized filter DTO crossing `SearchIndex::query`. Repeatable providers form an OR set; provider and time predicates combine with AND. `since` is inclusive and `until` is exclusive. CLI compact durations are resolved through the injected application clock before the request reaches the port; MCP accepts absolute ISO-8601 only. SQLite joins or correlates the FTS hit with authoritative catalog/identity metadata and applies predicates before limit/offset over-fetching. A filtered zero-match result is a normal empty page.

### 2. Message and around window

Add an application message-context request and an MCP `get_message` tool. Resolve the requested message to one typed placement in the named session or to the only session candidate. Select a bounded mainline window around that placement, retain the anchor, emit chronological order, then apply item and byte budgets. `around = 0` returns only the anchor. An optional additive context-around parameter may reuse the same primitive only when the current context path remains identical when omitted.

### 3. Context levels and hints

Extend context requests with `raw | talks | sessions`. Omission is `raw` and preserves the current response shape. `talks` groups each user turn with following assistant/tool messages; `sessions` emits a structural overview from already typed data (first user message, counts, structurally represented file list). No LLM, regex classification, or content heuristic is introduced. Empty higher levels fall back only toward detail: `sessions -> talks -> raw`, `talks -> raw`. Add bounded `requested_level`, `effective_level`, and `hint` metadata.

### 4. Match guidance

Add deterministic, bounded `why_matched` and `suggested_next_commands` to machine/MCP hits. Explanations use the literal query and fields already assembled for the hit; they do not expose FTS syntax. Suggested calls use only actual hit identifiers and prefer the final `get_message(message_id, session_id, around)` schema, followed by `get_session_context(session_id)`. Guidance does not affect score, ranking, cursor order, or human output.

## Integration order

1. Filters establish the final search request/port shape.
2. Message/around establishes typed message lookup and the final MCP tool schema.
3. Context levels reuse the typed window/context assembly.
4. Match guidance targets the final search and `get_message` schemas.

All four implementations may run in parallel in isolated worktrees, but integration follows this order. Conflicts in `application/src/lib.rs`, `cli/src/main.rs`, and `cli/src/mcp.rs` are resolved by preserving the earlier stream's typed primitive and adapting the later stream to it, not by choosing either whole file.

## Rollback

Each stream is additive and independently revertible. No catalog migration is required unless filtered pushdown proves impossible without persisted metadata; such a migration is out of scope and must return to planning rather than being improvised. Reverting all four restores the `fc2f2d4` request and response contracts.
