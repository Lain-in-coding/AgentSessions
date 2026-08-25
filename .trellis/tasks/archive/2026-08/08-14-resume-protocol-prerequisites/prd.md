# Resume protocol prerequisites

## Goal

Evolve the Robot schema to a published `1.1` without mutating the frozen `1.0`, bind list cursors to their result set, and harden the MCP JSON-RPC envelope, parameter, error, and response-budget contracts before any Resume tool is added.

## Requirements

### R1 Robot schema evolution
- Publish `1.1` with all additive fields including the already-runtime `occurrences`; declare search-hit `occurrences`/`resume_available` alongside the existing `why_matched`/`suggested_next_commands`. (Provider/time are query-side `SearchFilters`, NOT hit fields.)
- Freeze and keep the published `1.0` unchanged.
- Advance the runtime `SCHEMA_VERSION` used in envelopes to `1.1`; the `schemas/robot/v1/envelope.schema.json` path stays for compatibility or gains a documented `1.1` sibling without breaking existing consumers.
- The published schema still contains the entire runtime contract (drift guard keeps the current parity test green).

### R2 Cursor result-set binding
- Cursors minted by `list` over one sequence are rejected when used against `list_sessions`, and vice versa.
- A result-set discriminator is added to the cursor claims and validated on every resume.
- Rejections are explicit `cursor_invalid`-style errors instructing a fresh query, never silent restarts.

### R3 MCP JSON-RPC envelope and parameters
- Validate `jsonrpc == "2.0"` on every request; non-conforming requests fail with the correct JSON-RPC protocol error.
- Strictly validate request `id` type (no array, object, fractional, or missing id).
- Validate `initialize` params (`protocolVersion`, `capabilities`, `clientInfo`).
- A failed `initialize` must not open the gate through `notifications/initialized`.
- Validate `ping` and `tools/list` params shape, and `notifications/initialized` jsonrpc + params.
- Unknown methods and unknown/extra params continue to map to the correct JSON-RPC codes.

### R4 Budget floors at protocol layer
- Reject `max_bytes < 4096`, `max_messages < 1`, `max_items < 1`, and `limit < 1` with JSON-RPC `-32602` before constructing `AppRequest`.
- Keep the Application budget validation as defense in depth; the protocol-layer rejection is the primary path.

### R5 Bounded error messages
- Bound interpolation of unknown method, tool, id, and parameter values in error messages.
- Add string `maxLength` and array `maxItems` constraints to tool input schemas.

### R6 Dual-carrier byte accounting
- Decide and test whether `max_bytes` means the logical payload or the full MCP frame given that the same payload appears in `structuredContent` and `content[0].text`.
- Add floor-minus-one budget tests for every affected tool.

### R7 Tool-list documentation drift
- CONTRACT and SKILL currently list 6 tools and omit `get_message`; update both to 7 before any Resume tool makes them 8.

## Acceptance Criteria

- [ ] Robot 1.1 published with `occurrences` and additive fields; 1.0 frozen; runtime version 1.1.
- [ ] Cursor discriminator rejects cross-sequence cursors; existing cursors stay compatible.
- [ ] MCP rejects non-2.0 envelopes, invalid IDs, malformed initialize/ping/tools/list/notification params, and gate-open-by-notification.
- [ ] Budget floors are JSON-RPC `-32602` protocol errors before Application construction.
- [ ] Error messages and schema constraints bound interpolation.
- [ ] Dual-carrier budget meaning is decided and tested.
- [ ] CONTRACT and SKILL list exactly 7 tools before Resume adds the eighth.
- [ ] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- Additive only where the schema permits; no field removals; published 1.0 stays intact.
- MCP business errors remain distinguishable from protocol errors.
- No commit or push without explicit owner authorization.
