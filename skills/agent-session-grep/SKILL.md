---
name: agent-session-grep
description: Search local AI coding-agent session history (Claude Code, Codex) via the agent-session-grep robot CLI or MCP server.
---

# agent-session-grep

agent-session-grep indexes local AI coding-agent transcripts (Claude Code JSONL, Codex rollout JSONL) into a read-only searchable catalog: full-text search, session context assembly, and evidence spans that point back into the source files. Reach for it when you need to recall what happened in a past coding session.

Two machine surfaces exist. Prefer the MCP server when the client supports MCP; otherwise drive the robot CLI. Never scrape human-mode output.

All examples below are synthetic: `C:/data/example.db`, fabricated ids like `ses_v1_abc123`.

## Robot CLI

Always pass `--robot`. It emits exactly one stable JSON envelope on stdout, disables progress frames and color, and keeps diagnostics on stderr.

```
agent-session-grep --db C:/data/example.db --robot search "index rebuild" --max-items 20
```

### Envelope anatomy

```json
{
  "schema_version": "1.0",
  "frame_type": "response",
  "command": "search",
  "request_id": "cli-4242-1753500000000",
  "ok": true,
  "outcome": "success",
  "data": { "hits": [{ "id": "msg_v1_abc123", "score": -1.42 }], "generation": 3 },
  "warnings": [],
  "page": { "next_cursor": "eyJjb250cmFjdF9tYWpvciI6MX0.a1b2c3d4e5f60718", "has_more": true },
  "meta": { "duration_ms": 12, "generation": 3 }
}
```

- `ok` — false means an error envelope instead: `error: { code, message, retryable, details }`.
- `outcome` — `success` or `partial`; `partial` means the results are usable but truncated by a budget (see exit code 10).
- `data` — command-specific payload.
- `warnings` — honest degradations (for example unknown-precision evidence).
- `page` — pagination: feed `next_cursor` back via `--cursor` while `has_more` is true.
- `meta.generation` — index generation the result was computed against.

### Pagination loop

```text
cursor = null
loop:
  argv = [--db, DB, --robot, search, QUERY, --max-items, 20]
  if cursor != null: argv += [--cursor, cursor]
  env = parse_json(run(agent-session-grep, argv))
  consume(env.data.hits)
  if not env.page.has_more: break
  cursor = env.page.next_cursor
```

### Exit codes

| exit | meaning |
| --- | --- |
| 0 | success |
| 2 | validation error (bad arguments, invalid or expired cursor) |
| 3 | configuration error |
| 4 | not found |
| 5 | source / file IO error |
| 6 | catalog or index error (includes retryable `writer_busy`) |
| 7 | provider / adapter error |
| 9 | protocol or schema incompatibility (includes cursor generation mismatch) |
| 10 | partial success — results are usable but truncated; raise the budget knob named in `data.truncation.reason`, or paginate |
| 70 | internal error (bug signal) |

### Commands

| command | purpose |
| --- | --- |
| `search "<query>"` | full-text search over messages; hits carry `msg_v1_` ids |
| `list <limit>` | page catalog entities in stable id order |
| `context <session-id>` | assemble one session branch with evidence spans |
| `get <wire-id>` | raw stored payload of one entity |
| `show <wire-id>` | structured entity view (role, text, parent, session, span) |
| `status` | catalog entity count and active generation |
| `doctor` | health: db, schema, generation, interrupted_batches |

Budget flags (accepted where meaningful): `--max-items`, `--max-bytes`, `--max-messages`. Hitting a budget is reported as `outcome: "partial"` plus `data.truncation` and exit 10 — never a silent cut.

### Session context and evidence

```
agent-session-grep --db C:/data/example.db --robot context ses_v1_abc123 --policy mainline
```

- `--policy mainline` (default) follows the parent chain root to leaf and excludes sidechains; `--policy full` returns every message in sequence order.
- `data.evidence[]` has one span per returned message: source document id, source fingerprint, and location fields with `precision` tiers `byte`, `line`, `record`, or `unknown`.
- `unknown` precision means the location fields are null (data ingested before spans existed). Re-ingest the source to restore byte-precision spans; a warning is emitted alongside.

### Correlation and streaming

- `--request-id run.42:a` is echoed verbatim in every frame (charset `[A-Za-z0-9._:-]`, 1-128 chars); use it to correlate envelopes with your own logs.
- `--output jsonl` streams one complete frame per line and may emit `frame_type: "progress"` frames before the final response (long `sync` runs). `--robot` never emits progress frames.

## MCP server

Run the same binary as a stdio MCP server (tools only, sequential, read-only):

```json
{
  "mcpServers": {
    "agent-session-grep": {
      "command": "agent-session-grep",
      "args": ["--db", "C:/data/example.db", "mcp"]
    }
  }
}
```

| tool | when to use |
| --- | --- |
| `search_sessions` | full-text query; params: `query` (required), `limit`, `cursor`, `max_items`, `max_bytes` |
| `get_session_context` | pull one session branch: `session_id` (required, `ses_v1_...`), `policy` (`mainline` or `full`), `max_messages`, `max_bytes` |
| `list_sessions` | page catalog entities in stable id order (see caution below) |
| `list_providers` | which source formats are supported (`claude-code`, `codex`) |
| `get_status` | catalog count and active generation |
| `doctor` | health probe: `db: "ok"`, schema, generation, interrupted batches |

- Tool results carry the payload twice: `content[0].text` (serialized) and `structuredContent` = `{ outcome, data, warnings, page }` — the same shapes as the robot envelope.
- Business failures (bad cursor, not found) come back as `isError: true` results with `structuredContent.error.canonical_code`; malformed or invalid params are JSON-RPC errors (`-32602`).
- Cursors are stateless signed tokens: a `page.next_cursor` from one `search_sessions` call works in a later call — even across server restarts — as long as the index generation is unchanged and the TTL has not passed.
- v0 executes requests sequentially; `notifications/cancelled` is accepted but is a best-effort no-op.

## Cautions

- Read-only: search/context/MCP never modify your history. The MCP server opens only the `--db` store given at startup; it exposes no arbitrary file read, no SQL, no command execution.
- Cursor lifecycle: cursors expire after 15 minutes and die whenever new data is ingested (generation change). On `cursor_expired`, `cursor_invalid`, or `generation_mismatch`, do not retry the token — re-issue the query from page 1.
- `list_sessions` v0 pages ALL catalog entities in stable id order — expect interleaved `msg_v1_`, `ses_v1_`, and `doc_v1_` prefixed ids; filter for the `ses_v1_` prefix yourself.
- Never parse human-mode output (the default without `--robot`); its wording can change at any time. Machine consumption is `--robot`, `--output json` / `--output jsonl`, or MCP only.
