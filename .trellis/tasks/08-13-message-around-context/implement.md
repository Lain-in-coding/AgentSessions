# Implement — Message around context

1. Add or extend a typed context-port capability needed to resolve a message within a session without SQL/JSON leakage.
2. Add application request/response DTOs and explicit ambiguity handling.
3. Implement the reusable mainline window helper with anchor retention and chronological output.
4. Apply `max_items` and `max_bytes` through existing budget helpers with explicit truncation.
5. Add MCP `get_message` schema, validation, dispatch, and shared response projection.
6. Add optional context-around only if it reuses the helper and leaves omission byte-compatible.
7. Update testkit/SQLite port implementations as required.
8. Add application tests for unique/multiple sessions, wrong session, boundaries, ordering, and budgets.
9. Add MCP catalog/unit/E2E tests for success, ambiguity, invalid params, and not found.
10. Run formatting, focused workspace tests, and Clippy with warnings denied.

Critical files:
- `crates/agent-session-grep-ports/src/lib.rs`
- `crates/agent-session-grep-application/src/lib.rs`
- `crates/agent-session-grep-adapters-sqlite/src/lib.rs` if a new typed lookup is required
- `crates/agent-session-grep-testkit/src/lib.rs`
- `crates/agent-session-grep-cli/src/main.rs`
- `crates/agent-session-grep-cli/src/mcp.rs`
- `crates/agent-session-grep-cli/tests/mcp_e2e.rs`

Integrate after filters and before context levels. Do not commit or push.
