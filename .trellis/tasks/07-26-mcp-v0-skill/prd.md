# PRD: stdio MCP v0 and Skill beta

Parent: `07-24-advance-integration-beta` child 5. Normative source:
`docs/contracts/CONTRACT-cli-robot-mcp-draft.md` §8 (MCP contract), §5 (error
catalog). Prerequisites (landed): child 3 shared ADT, child 4 protocol
surfaces. MCP handlers map protocol to the ADT and MUST NOT duplicate
search/branch/pagination/budget business rules.

## Problem

AgentSessions has no MCP surface: AI agents can only shell out to the CLI.
Contract §8 defines the tool set and constraints, but nothing implements it.
There is also no Skill artifact teaching Claude how to drive the robot CLI.

## Requirements

- R1 `agentsessions mcp` subcommand: a stdio MCP server (JSON-RPC 2.0, one
  message per line) exposing exactly the §8 tools:
  `search_sessions`, `get_session_context`, `list_sessions`,
  `list_providers`, `get_status`, `doctor`.
- R2 Protocol basics: `initialize` (protocol version negotiation: reply with
  the client's requested version when supported, else our pinned latest),
  `notifications/initialized`, `tools/list` (versioned JSON schemas),
  `tools/call`, `ping`, graceful EOF shutdown. Unknown methods →
  JSON-RPC method-not-found; malformed JSON → parse error; stdout carries
  ONLY protocol frames (panics/diagnostics must not pollute it).
- R3 Handlers are thin: validate params → build `AppRequest` → reuse the
  existing App/`render` semantics. Cursor/budget/policy params pass through;
  truncation and warnings surface in tool results; errors map canonical codes
  into JSON-RPC error `data` ({canonical_code, retryable}).
- R4 Security: no arbitrary file read, no SQL, no command execution. The
  server opens ONLY the `--db` store given at startup, read-only
  (`SqliteStore::open`); write commands are not exposed.
- R5 Skill beta: `skills/agentsessions/SKILL.md` teaching an agent to drive
  the robot CLI (`--robot`, cursor pagination loop, exit codes, context
  command, evidence semantics). Synthetic examples only; no personal paths.
- R6 Tests: an e2e that spawns the real binary, performs initialize →
  tools/list → tools/call round-trips over stdio, and asserts error mapping +
  stdout purity. Unit tests for request parsing/dispatch mapping.

## Acceptance criteria

- [ ] `initialize` handshake completes against a scripted client; version
      negotiation honest (unsupported request → our version, never a lie).
- [ ] `tools/list` returns 6 tools with schemas; `search_sessions` round-trip
      returns hits + next_cursor; second call with cursor returns the next
      disjoint page (reusing ADT pagination).
- [ ] `get_session_context` returns branch messages + evidence for a real
      ingested fixture; `doctor`/`get_status`/`list_providers` return real data.
- [ ] Canonical error mapping: bad cursor → tool error carrying
      `canonical_code: cursor_invalid`; unknown tool → JSON-RPC error.
- [ ] stdout purity: nothing but JSON-RPC frames on stdout during the whole
      session (asserted by parsing every line).
- [ ] SKILL.md exists with working synthetic examples; no real paths.
- [ ] Full gates: fmt / clippy -D warnings / workspace tests / cargo deny;
      zero new dependencies.

## Out of scope

- MCP resources/prompts/sampling capabilities (tools only in v0).
- HTTP/SSE transports; concurrent in-flight request execution (v0 processes
  requests sequentially; cancellation → documented as best-effort no-op).
- TUI (child 6); installation/promotion (child 7).
- Marking the CONTRACT draft Accepted.
