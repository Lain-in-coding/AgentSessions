# Competitor borrowings: filters, noise, aggregation, around

## Goal

Borrow the highest-signal capabilities from the 12 competitor projects read
in the 2026-08-13 sweep (claude-historian, ctx, agent-sessions, sessiongrep,
hstry, memex, Recall, AgentRecall, cass, agentsview, agf, fast-resume):
filters, noise suppression, aggregation, around-context, and a message tool.
Everything stays inside the existing envelope/budget/cursor contracts.

## Requirements

### R1 Filters: provider + time range
- R1.1 `search` (CLI + MCP `search_sessions`) gains `--provider`
  (claude/codex, repeatable) and `--since`/`--until` (ISO-8601; CLI also
  accepts compact forms `1d`/`1w`/`1h` per hstry/memex precedent).
- R1.2 Filters push down into the FTS/application query; zero results from
  filtering are a clean empty result, not an error.
- R1.3 `list_sessions` filters to session entities only (fixes P2-4: pages
  were flooded by doc/msg entities).

### R2 System-noise default filtering
- R2.1 Messages that are system context (AGENTS.md / available skills /
  system prompt / compaction summaries) are excluded from search by default;
  `--include-system` opt-in. Implemented as an index-time field or
  post-filter flag; must not break literal-table behavior.

### R3 Aggregation: group-by-session + occurrences
- R3.1 `search` gains `group_by_session` (default false — additive); when
  set, hits collapse per session, best-scoring hit first, with an
  `occurrences` count field (hstry compact precedent).

### R4 Around/context window + get_message
- R4.1 MCP new tool `get_message(message_id, around=N, max_items=N,
  max_bytes=N)`: returns the hit message plus ±N neighbors (memex
  at/around precedent); `get_session_context` optionally gains `around`.
- R4.2 `search_sessions` hits carry `session_id` (already landed in
  core-usability R4 — this task depends on it).

### R5 Summary levels + hint (memex L3/L2/L0 precedent, light)
- R5.1 `get_session_context` gains `level` (raw default; talks=per-turn
  summaries; sessions=session-level summary) with automatic fallback when a
  level is empty; response includes a `hint` pointing to the next useful
  call. Summaries are assembled structurally (first user message, message
  count, file list) — no LLM, no regex heuristics (avoids
  claude-historian's fake-positive trap).

### R6 Search match guidance (ctx precedent)
- R6.1 Machine and MCP search hits gain deterministic `why_matched` and bounded
  `suggested_next_commands`; human output and ranking remain unchanged.
- R6.2 Guidance is derived only from the literal query and identifiers already
  present on the hit. It never invokes an LLM, exposes FTS syntax, or invents
  a session/message identifier.
- R6.3 Guidance bytes count toward `max_response_bytes`; generated message
  calls include `session_id` whenever available so shared message identities
  are not resolved by guessing.

## Acceptance Criteria

- [x] provider/time filters return correct subsets (unit + e2e).
- [x] `list_sessions` returns sessions only.
- [x] system noise excluded by default, opt-in restores.
- [x] group_by_session collapses with occurrences; default path unchanged.
- [x] get_message + around return the correct window within budget.
- [x] level summaries fall back and carry hints.
- [x] search hits explain literal matches and suggest valid bounded next calls.
- [x] Gates: fmt / clippy -D warnings / test --workspace / build --release.
- [ ] Gate D re-run green.

## Constraints

- Depends on core-usability R4 (session_id on hits) — sequencing: start
  after core-usability lands or branch after it merges.
- Additive contract changes only; no field removals; schema minor.
- No commit/push without owner authorization.
