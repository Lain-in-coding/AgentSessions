# Search Match Guidance

## Goal

Explain deterministic literal search matches and suggest valid next calls without changing human output, ranking, or cursor behavior.

## Requirements

- Machine and MCP search hits add `why_matched` and `suggested_next_commands`.
- `why_matched` derives only from the literal query and hit text/fields after the existing plain-text/CJK transform; it exposes no FTS syntax and uses no LLM or regex classifier.
- Suggested commands use only real `message_id`/`session_id` values on the hit, prefer `get_message(message_id, session_id, around)` and then `get_session_context(session_id)`, and never invent identifiers.
- Both collections are bounded and counted under `max_response_bytes`.
- Human output, score, ranking, cursor total order, and omitted-field search behavior remain unchanged.

## Acceptance Criteria

- [x] Robot JSON/JSONL and MCP hits contain deterministic `why_matched` evidence.
- [x] Suggested commands are bounded and contain only actual identifiers.
- [x] Shared-message suggestions include `session_id` when available.
- [x] Small byte budgets truncate explicitly rather than exceeding the envelope.
- [x] Human output, scores, ordering, and cursors are unchanged.

## Constraints

- Depends on final provider/time filter request shape and final `get_message` schema; integrate last.
- Additive schema change only.
- No commit/push without owner authorization.
