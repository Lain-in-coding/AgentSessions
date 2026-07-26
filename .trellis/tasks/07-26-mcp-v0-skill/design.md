# Design: stdio MCP v0 and Skill beta

Frozen API spec for parallel agents. Deviations from this file must be
reported in the completion message, not silently applied.

## 0. Design decisions (pre-resolved deviation points)

1. **Protocol versions**: `SUPPORTED_PROTOCOL_VERSIONS = ["2025-06-18",
   "2025-03-26", "2024-11-05"]`, `LATEST_PROTOCOL_VERSION = "2025-06-18"`.
   Negotiation per MCP spec: requested ∈ supported → echo requested; else →
   reply `LATEST_PROTOCOL_VERSION`. Never lie about support.
2. **Tool results carry both** `content: [{type:"text", text: <serialized
   payload>}]` **and** `structuredContent: <payload object>` (2025-06-18
   field; older clients ignore unknown fields). No `outputSchema` in v0.
3. **Error split** (MCP spec): protocol-level problems (malformed JSON,
   unknown method, unknown tool, invalid/missing params, not initialized) →
   JSON-RPC `error` object; business failures during tool execution
   (cursor_invalid, not_found, catalog_error, ...) → successful JSON-RPC
   response whose result has `isError: true` + structured canonical error.
4. **`list_sessions` v0 limitation**: maps to `AppRequest::List`, which pages
   ALL catalog entities (messages, sessions, documents) in stable id order.
   The tool description states this honestly and points out the `ses_v1_`
   prefix. No id-prefix filtering in the handler (that would be a business
   rule outside the ADT).
5. **JSON-RPC batch arrays are rejected** with `-32600` (2025-06-18 removed
   batching; we never supported it).
6. **`--output`/`--robot` flags are ignored by `mcp`** — MCP framing IS the
   protocol; there is no human mode. `--request-id` is likewise irrelevant
   (JSON-RPC ids serve that purpose).
7. **Initialize gate**: `initialize` and `ping` are always allowed. Any other
   request before the `notifications/initialized` notification →
   `-32600, "server not initialized"`. Notifications with unknown methods are
   ignored (JSON-RPC: notifications never get responses).
   `notifications/cancelled` is a documented no-op (v0 is sequential).
8. **stdout purity**: every outgoing frame goes through
   `protocol::write_stdout_line` (EPIPE → exit 0, other write errors →
   stderr + exit 5). Diagnostics, if any, go to stderr only. EOF on stdin →
   clean shutdown, `Outcome::Success`, exit 0.
9. **Zero new dependencies.** serde_json + std only; e2e uses the existing
   `tempfile` dev-dep.

## 1. File ownership (frozen)

| Path | Owner |
|---|---|
| `crates/agentsessions-cli/src/mcp.rs` | agent **mcp-server** (full module incl. unit tests; a stub with the frozen signature exists when you start) |
| `crates/agentsessions-cli/tests/mcp_e2e.rs` | agent **mcp-e2e** (new file) |
| `skills/agentsessions/SKILL.md` | agent **mcp-e2e** (new file) |
| `crates/agentsessions-cli/src/main.rs` | main session ONLY (`mod mcp;` + run() interception, pre-wired before dispatch) |
| spec indexes, commits | main session ONLY |

Agents never run `git commit`/`git push`, never run `cargo fmt --all`
(only `rustfmt --check` on owned files), and verify every written file
survives (Defender hazard: re-read after write).

## 2. `mcp.rs` module contract (frozen)

```rust
/// Entry point called from run() after the store is opened read-only.
/// Serves newline-delimited JSON-RPC 2.0 over stdin/stdout until EOF.
pub(crate) fn serve(store: &SqliteStore) -> Result<protocol::Outcome, CliError>
```

Internal shape (names frozen so e2e/spec can reference them; signatures may
carry lifetimes as needed):

- `struct McpServer<'a> { store: &'a SqliteStore, initialized: bool }`
- `fn handle_line(&mut self, line: &str) -> Option<String>` — parse one
  incoming line; `None` for notifications/ignored input, `Some(frame)` for a
  response to write. Malformed JSON → `-32700` response with `id: null`.
  Non-object (incl. arrays) → `-32600`.
- `fn negotiate_version(requested: Option<&str>) -> &'static str`
- Tool dispatch builds `AppRequest` directly (typed), runs
  `App::new(store_ref(store), store_ref(store)).handle(req)`, projects via
  `crate::render(response)` — the SAME projection the CLI uses. No duplicated
  search/branch/pagination/budget logic. Access parent items as
  `crate::{render, store_ref, CliError}`; they are visible to child modules.

### Handshake frames

- `initialize` → result `{ "protocolVersion": <negotiated>, "capabilities":
  { "tools": {} }, "serverInfo": { "name": "agentsessions", "version":
  env!("CARGO_PKG_VERSION") } }`.
- `notifications/initialized` → sets `initialized = true`, no response.
- `ping` → result `{}`.
- `tools/list` → result `{ "tools": [ ...6 tools, contract §8 order... ] }`;
  each tool: `{ name, description, inputSchema }` with
  `inputSchema.additionalProperties: false`.
- `tools/call` → see §3. Unknown tool name → `-32602`.
- Unknown request method → `-32601`.

### Success payload (shared shape for all 6 tools)

`render()` returns `(outcome, data, page, warnings)`; the tool payload is:

```json
{
  "outcome": "success" | "partial",
  "data": { ... same data object as the robot envelope ... },
  "warnings": ["..."],
  "page": { "next_cursor": "tok" | null, "has_more": bool }
}
```

Result frame: `{ "content": [{ "type": "text", "text": <payload serialized>
}], "structuredContent": <payload>, "isError": false }`.

### Business-error payload

`CliError(ProtocolError)` from param mapping that is business-level (bad
wire id, bad cursor at App level, not found, ...) →

```json
{ "content": [{ "type": "text", "text": <message> }],
  "structuredContent": { "error": {
      "canonical_code": "<code.as_str()>", "message": "...",
      "retryable": bool, "details": { ... } } },
  "isError": true }
```

Note: structurally invalid params (missing required field, wrong JSON type,
unknown property) are protocol errors → `-32602` with
`error.data = { "canonical_code": "invalid_request", "retryable": false }`.
Values that parse but fail domain validation (e.g. `session_id` not a valid
wire id, `policy: "weird"`) are ALSO `-32602` (they are request validation,
same canonical_code in data). Everything that happens after a well-formed
`AppRequest` is built → `isError: true` result.

## 3. Tools (names/params frozen, contract §8)

| Tool | Params (JSON Schema properties) | Required | Mapping |
|---|---|---|---|
| `search_sessions` | `query: string`, `limit: integer(min 1)`, `cursor: string`, `max_items: integer`, `max_bytes: integer` | `query` | `AppRequest::Search { query, limit: limit.or(max_items).unwrap_or(20), cursor, budget }` — budget from `ResponseBudget::default()` overridden by max_items/max_bytes |
| `get_session_context` | `session_id: string`, `policy: string enum ["mainline","full"]`, `max_messages: integer`, `max_bytes: integer` | `session_id` | `StableId::from_wire` (invalid → -32602); default policy mainline; budget overrides max_messages/max_bytes → `AppRequest::Context` |
| `list_sessions` | `limit: integer`, `cursor: string`, `max_items: integer`, `max_bytes: integer` | — | `AppRequest::List { limit: limit.or(max_items).unwrap_or(20), cursor, budget }` |
| `list_providers` | — | — | no App call: enumerate `crate::provider_registry()` → `data = { "providers": [{ "id": provider_id }...] }`, outcome success, empty page/warnings |
| `get_status` | — | — | `AppRequest::Status` |
| `doctor` | — | — | no App call: store methods `schema_version` / `active_generation` / `interrupted_batch_count` → same JSON shape as CLI doctor with `"db": "ok"` |

Integer params: JSON numbers must be non-negative integers that fit `usize`;
anything else → `-32602`. Unknown extra properties → `-32602` (schemas say
`additionalProperties: false`; enforce in code too).

## 4. e2e (`tests/mcp_e2e.rs`)

Helper: `fn mcp_session(db: &Path, inputs: &[serde_json::Value]) ->
Vec<serde_json::Value>` — spawn the compiled binary (`env!("CARGO_BIN_EXE_agentsessions")`)
with `--db <db> mcp`, pipe stdin, write one line per input, close stdin, read
stdout to end, parse EVERY line as JSON (purity assertion lives here: any
unparseable line panics), return parsed frames. Fixtures: reuse the CLI
`ingest`/`index` commands (spawn binary) to populate the db first, same
pattern as `e2e.rs` (Claude 4-line fixture with `isSidechain`).

Standard preamble for tool tests: `initialize` → `notifications/initialized`
→ then the calls under test.

Required scenarios (one test each unless noted):
1. initialize handshake: echoed version for "2024-11-05"; unsupported
   "9999-01-01" → "2025-06-18"; serverInfo/capabilities shape; ping → `{}`.
2. tools/list: exactly 6 tools, contract names, each has inputSchema.
3. search pagination: ingest fixture, `search_sessions {query, max_items:1}`
   → 1 hit + `page.next_cursor`; second call with cursor → different hit id,
   pages disjoint.
4. context: `get_session_context` on fixture session → mainline messages +
   evidence array present; `structuredContent.outcome == "success"`.
5. bad cursor: `search_sessions {cursor: "garbage"}` → `isError: true`,
   `structuredContent.error.canonical_code == "cursor_invalid"`.
6. unknown tool → JSON-RPC error `-32602`; unknown method `foo/bar` →
   `-32601`; malformed JSON line → `-32700` with `id: null`; batch array →
   `-32600`.
7. initialize gate: `tools/list` before initialize → `-32600`.
8. status/doctor/list_providers round-trip on a populated db: real counts,
   `db: "ok"`, providers contain `claude-code` and `codex`.
9. invalid params: `get_session_context {session_id: "not-a-wire-id"}` →
   `-32602` with `data.canonical_code == "invalid_request"`.

## 5. SKILL.md (`skills/agentsessions/SKILL.md`)

Frontmatter: `name: agentsessions`, one-line `description` (when to reach for
it: searching local AI coding-agent session history). Body teaches BOTH
surfaces with synthetic examples only (`C:/data/example.db`, fixture ids):

1. Robot CLI: `agentsessions --db <db> --robot search "<q>" --max-items 20`;
   envelope anatomy (ok/outcome/data/page/meta); pagination loop pseudo-code
   (`while page.has_more: pass --cursor`); exit codes table (0/2/4/5/6/7/9/
   10/70) with "10 = partial, results usable but truncated"; `context
   <ses-id> --policy mainline` + evidence span semantics (precision tiers,
   unknown → re-ingest); `--request-id` correlation; jsonl progress frames.
2. MCP server: `agentsessions --db <db> mcp` in an MCP client config; the 6
   tools and when to use which; cursor reuse across tool calls.
3. Cautions: read-only guarantees; cursors expire (15 min TTL) and die on
   generation change — retry from page 1; never parse human output, always
   `--robot`.

## 6. Integration (main session)

1. Pre-wire before dispatching agents: `mod mcp;` + stub `mcp.rs` (serve →
   `Err(CliError::usage("mcp: not yet implemented"))`), interception in
   `run()` after store open: `if rest.first() == Some("mcp") { return
   mcp::serve(&store); }` (read-only: "mcp" is not in the writes list), plus
   help text line. Workspace must stay green with the stub.
2. After mcp-server lands: full gates; after mcp-e2e lands: run
   `cargo test -p agentsessions-cli --test mcp_e2e`; reconcile mismatches by
   messaging the owning agent (or minimal main-session fix if mechanical).
3. Spec update: `agentsessions-cli/backend/index.md` gains the MCP section
   (stdout purity, error split, initialize gate, version set).
4. Commit slicing (each compile-green): (a) task planning docs; (b) mcp.rs
   module + main.rs wiring; (c) mcp_e2e.rs + SKILL.md; (d) spec docs.
