# Implement — Context summary levels and hints

1. Extend application context request/response types with the level enum and additive requested/effective level plus hint fields.
2. Keep the existing raw context assembly as the single typed source.
3. Add structural talk grouping and session overview helpers without regex or LLM logic.
4. Implement `sessions -> talks -> raw` and `talks -> raw` fallback.
5. Include summaries and hints in byte estimates and explicit truncation.
6. Extend MCP `get_session_context` schema/validation/projection; keep omitted level compatible.
7. Add application tests for grouping, overview, fallback, hints, and budgets.
8. Add MCP unit/E2E tests for valid/invalid levels and raw compatibility.
9. Run `cargo fmt --all --check`, focused application/CLI tests, and Clippy with warnings denied.

Critical files:
- `crates/agent-session-grep-application/src/lib.rs`
- `crates/agent-session-grep-cli/src/main.rs`
- `crates/agent-session-grep-cli/src/mcp.rs`
- `crates/agent-session-grep-cli/tests/mcp_e2e.rs`

Dependency: integrate after message-around-context and reuse its typed placement/window primitive where applicable. Do not commit or push.
