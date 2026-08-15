# Implement — Search match guidance

1. Extend the machine-facing search hit DTO with bounded `why_matched` and `suggested_next_commands` collections.
2. Add deterministic literal evidence assembly in the application search path while the full catalog payload is available; share the canonical literal-token analysis and never reuse/expose the private FTS-quoted query.
3. Generate next calls only from actual hit IDs and target the final `get_message` schema.
4. Charge the JSON-serialized/escaped size of both fields in search item byte estimates before `clamp_items`; CLI/MCP must not append guidance afterward.
5. Serialize fields in Robot JSON/JSONL and MCP; leave human rendering unchanged.
6. Update additive envelope schema where required.
7. Add application tests for CJK/ASCII evidence, a match beyond the displayed text prefix, missing IDs, JSON-escaped byte bounds, ordering, and cursor stability.
8. Add CLI/MCP E2E tests for fields, valid commands, byte limits, and unchanged human output.
9. Run formatting, focused tests, and Clippy with warnings denied.

Critical files:
- `crates/agent-session-grep-ports/src/lib.rs` or the final integrated hit DTO owner
- `crates/agent-session-grep-application/src/lib.rs`
- `crates/agent-session-grep-cli/src/main.rs`
- `crates/agent-session-grep-cli/src/mcp.rs`
- `crates/agent-session-grep-cli/src/human.rs`
- `crates/agent-session-grep-cli/tests/e2e.rs`
- `crates/agent-session-grep-cli/tests/mcp_e2e.rs`
- `schemas/robot/v1/envelope.schema.json`

Dependency: integrate after filters, message-around-context, and context-summary-hints. Do not commit or push.
