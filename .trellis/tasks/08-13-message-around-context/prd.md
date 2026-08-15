# Message around context

## Goal

Add a bounded MCP `get_message` tool and typed neighbor windows without guessing among shared-message placements.

## Requirements

- Add `get_message(message_id, session_id?, around=0, max_items, max_bytes)`.
- If `session_id` is omitted, one session candidate auto-resolves; multiple candidates return explicit bounded ambiguity and never guess.
- Window selection uses typed placements and selected mainline order, not compatibility payload aliases.
- `around=0` returns the anchor only; larger values return up to N neighbors on each side, retain the anchor, and emit chronological order.
- Item and byte budgets apply to the complete result and truncation is explicit.
- `get_session_context` may gain an additive optional around parameter only if it reuses the same primitive and omission preserves the old path.

## Acceptance Criteria

- [x] Unique-session and explicit-session lookups return the correct anchor.
- [x] Multi-session lookup without session ID returns explicit ambiguity with bounded candidates/hint.
- [x] Around windows handle start/end boundaries, preserve anchor, and remain chronological.
- [x] `around=0`, item limits, byte limits, unknown IDs, and wrong-session IDs are covered.
- [x] MCP catalog/schema and E2E tests cover the new tool.
- [x] Existing context behavior is unchanged when optional around is omitted.

## Constraints

- Integrate after filters and before context levels.
- Additive contract only; typed relations remain authoritative.
- No commit/push without owner authorization.
