# Resume protocol prerequisites — Implementation

## Preconditions

- `task.py start` this task before editing.
- Work in the integration worktree on the verified baseline. This task is file-adjacent to the core task (`mcp.rs`); coordinate so only one of them edits `mcp.rs` at a time.

## Steps

1. **Robot 1.1 schema**: create `schemas/robot/v1.1/envelope.schema.json` mirroring 1.0 plus additive fields; freeze 1.0; update runtime `schema_version` and drift-guard test.
2. **Cursor discriminator**: add `result_set` claim to cursor claims struct + signing; validate on resume; mint correctly in `list`/`list_sessions`; add mismatch tests.
3. **MCP envelope**: `jsonrpc != "2.0"` rejection; strict `id` validation.
4. **MCP initialize gate**: params validation; failed initialize cannot be opened by `notifications/initialized`.
5. **MCP params**: `ping`/`tools/list`/notification params validation.
6. **Budget floors**: protocol-layer `-32602` for below-floor budgets.
7. **Bounded errors + schema constraints**: interpolation caps; `maxLength`/`maxItems`.
8. **Dual-carrier budget**: implement the decision; add floor-minus-one tests.
9. **Docs**: CONTRACT + SKILL to exactly 7 tools (`get_message` added).

## Validation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --release`

## Review gates

- `trellis-check` after edits; assert 1.0 schema unchanged (diff against git), 7-tool parity, no regression in business-vs-protocol error mapping.
- Do NOT commit/push.
