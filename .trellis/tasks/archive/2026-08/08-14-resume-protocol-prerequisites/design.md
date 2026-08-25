# Resume protocol prerequisites — Design

## Boundaries

- Touches `cli/src/mcp.rs`, `cli/src/protocol.rs`, `schemas/robot/v1/`, runtime `SCHEMA_VERSION`, CONTRACT/SKILL tool lists, and cursor claims (domain/application `cursor.rs`).
- Does NOT add the Resume tool; that belongs to the core task.

## Robot 1.1

- Publish `schemas/robot/v1.1/envelope.schema.json` containing all additive runtime fields (`occurrences` integer optional, `resume_available` boolean always-present, `why_matched`/`suggested_next_commands` already declared in 1.0) with `additionalProperties` still tight. Provider/time predicates are query-side `SearchFilters` and are NOT hit fields.
- Keep `schemas/robot/v1/envelope.schema.json` (1.0) frozen.
- Runtime envelope `schema_version` → `"1.1"`; update the drift-guard test (`published_envelope_schema_contains_runtime_contract`) to compare against 1.1 while pinning 1.0 as frozen.

## Cursor discriminator

- Add a `result_set` claim (e.g. `"sessions_only"` vs `"all"`) to the cursor token claims struct in `application/src/cursor.rs`.
- Minting call sites (`list` vs `list_sessions`) set the discriminator.
- On resume, validate it; mismatch → `cursor_invalid` error instructing a fresh query (no silent restart).
- Cursor signing/verification already exists; add the claim to the digest and wire shape. Existing tokens without the claim fail closed as invalid (acceptable: pre-1.1 cursors).

## MCP JSON-RPC hardening (`cli/src/mcp.rs`)

- Envelope: reject `jsonrpc != "2.0"` with `-32600` before any method dispatch.
- ID: accept only string/number/null per spec; reject array/object/fractional/missing with `-32600`; business calls keep echoing a valid ID.
- `initialize`: require object `params` with `protocolVersion` (string), `capabilities` (object), `clientInfo` (object). Failed initialize sets a `gate_open: false` state that `notifications/initialized` cannot flip without a successful handshake.
- `ping`/`tools/list`: params must be absent or an object (reject arrays/primitives with `-32602`).
- `notifications/initialized`: require `jsonrpc == "2.0"` and object-or-absent params.
- Unknown method → `-32601`; unknown/extra params → `-32602` (already partly tested).

## Budget floors

- Add a protocol-layer validation before `AppRequest` construction: `max_bytes < 4096`, `max_messages < 1`, `max_items < 1`, `limit < 1` → `-32602`.
- Keep Application validation as defense in depth (existing tests stay green).

## Bounded errors + schemas

- Cap interpolation length of method/tool/id/param values in error messages.
- Add `maxLength`/`maxItems` to tool input schemas for unbounded string/array params.

## Dual-carrier budget

- Decision: `max_bytes` = the logical application payload bytes (the value that would be returned in `structuredContent`); the duplicated `content[0].text` wrapper is protocol overhead and is NOT counted against the application budget, but a guard test documents the ~2x frame and asserts the logical budget still binds.
- Add floor-minus-one tests per tool.

## Tests

- `mcp_e2e.rs` + unit matrix: envelope, id, initialize gate, params, floors, bounded errors, dual-carrier, 7-tool drift.
