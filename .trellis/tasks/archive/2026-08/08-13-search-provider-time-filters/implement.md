# Implement — Provider and time search filters

1. Add normalized filter DTOs and extend `SearchIndex::query` plus forwarding/test implementations.
2. Extend `AppRequest::Search`; validate provider set and half-open range; bind canonical filters into cursor digest.
3. Add hand-written CLI parsing for repeatable provider and absolute/compact time inputs using the injected clock.
4. Add MCP schema/validation for provider array and absolute ISO-8601 bounds.
5. Implement SQLite FTS plus metadata predicate pushdown while retaining score/ID ordering and parameter binding.
6. Add port/testkit updates and focused SQLite tests for all filter combinations and boundaries.
7. Add application cursor-mismatch and empty-page tests.
8. Add CLI/MCP E2E tests for parsing, validation, subsets, and omitted-filter compatibility.
9. Run formatting, focused workspace tests, and Clippy with warnings denied.

Critical files:
- `crates/agent-session-grep-ports/src/lib.rs`
- `crates/agent-session-grep-application/src/lib.rs`
- `crates/agent-session-grep-adapters-sqlite/src/lib.rs`
- `crates/agent-session-grep-testkit/src/lib.rs`
- `crates/agent-session-grep-cli/src/main.rs`
- `crates/agent-session-grep-cli/src/mcp.rs`
- CLI/MCP E2E tests

Integrate first. Do not commit or push.
