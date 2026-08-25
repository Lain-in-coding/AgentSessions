# Context summary levels and hints

## Goal

Add structural context levels and deterministic next-call hints without changing the existing raw context path.

## Requirements

- `get_session_context.level` is optional and accepts `raw`, `talks`, or `sessions`; omission is `raw` and preserves current behavior.
- `talks` groups a user message with the following assistant/tool messages until the next user message.
- `sessions` returns one structural overview using typed context data: first user message, message/turn counts, and file references already represented structurally.
- Summaries use no LLM, regex classifier, or content heuristic.
- Empty levels fall back only toward detail: `sessions -> talks -> raw`, `talks -> raw`; `raw` never falls back.
- Responses add bounded `requested_level`, `effective_level`, and `hint`; summary and hint bytes count toward the existing response budget.
- Hints name a deterministic next useful call or level and never invent identifiers.

## Acceptance Criteria

- [x] Omitted level and explicit `raw` preserve the existing raw response and ordering.
- [x] `talks` and `sessions` return correct structural summaries.
- [x] One-way fallback reports the requested and effective levels.
- [x] Hints are deterministic, bounded, and use only actual identifiers.
- [x] Summary/hint bytes participate in truncation and `max_response_bytes`.
- [x] MCP schema and end-to-end tests cover valid/invalid levels, fallback, and budgets.

## Constraints

- Depends on the typed message/around primitive; integrate after message-around-context.
- Additive contract changes only; no field removals or raw ranking/selection changes.
- No commit/push without owner authorization.
