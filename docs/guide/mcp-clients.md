# MCP client setup

`agent-session-grep mcp` runs the same binary as a stdio MCP server, so any MCP
client can search your indexed agent history. This page gives a copy-pasteable
configuration for the common clients, describes all nine tools, shows a real
request/response pair, and explains the one operational caveat that surprises
everyone: **nothing keeps the index fresh for you.**

Everything below was verified by driving a release build over stdio, not by
reading the source alone.

- Flags and exit codes: [CLI reference](../reference/cli.md)
- Error messages: [troubleshooting](troubleshooting.md)
- Deeper agent-facing integration notes:
  [`skills/agent-session-grep/SKILL.md`](../../skills/agent-session-grep/SKILL.md)
  — written for an agent consuming this tool, it goes further than this page on
  the two-ID contract, cursor lifecycle, and per-tool output shapes.

## Before you configure a client

The MCP server is a **read path**. Read paths never create a store, so the
database must exist before a client launches the server. If it does not, the
process exits immediately with code 4 and never speaks JSON-RPC at all — from
the client's side that looks like "the server failed to start", with no protocol
error to inspect:

```
error [not_found]: no store at /path/to/asg.db (from --db).
Build one with: asg --db /path/to/asg.db sync --discover
Reads never create a store, so a mistyped --db fails here instead of returning an empty result.
```

So do these two things first.

**1. Build the index.**

```
asg sync --discover
```

**2. Find out where the store landed**, because the client configuration needs
an absolute path:

```
asg config paths
```

The default store is `asg.db` inside the reported `data` directory. You can also
put it anywhere you like and pass `--db <path>` explicitly, which is what the
examples below do — an absolute path in the client configuration is far easier to
debug than a platform default.

Always use an absolute path. MCP clients launch the server with a working
directory you do not control, so a relative path will resolve somewhere
unexpected.

## Claude Code

Add it from the command line. Everything after `--` is passed to the server
process:

```
claude mcp add agent-session-grep -- agent-session-grep --db /path/to/asg.db mcp
```

On Windows, use the Windows path form:

```
claude mcp add agent-session-grep -- agent-session-grep --db C:\path\to\asg.db mcp
```

`claude mcp add` defaults to stdio transport and to `--scope local` (this project
only). Use `--scope user` to make it available in every project, or
`--scope project` to write it into the project's `.mcp.json` for your whole team.

Verify with `claude mcp list`, then confirm the tools are visible in a session.

If you would rather write the file yourself, project scope is `.mcp.json` at the
repository root, in the same `mcpServers` shape as the other clients below. Note
that this repository gitignores `.mcp.json` on purpose — it can hold local paths
and credentials — so a project-scoped entry here stays local to your checkout.

## Claude Desktop

Edit `claude_desktop_config.json`:

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Windows | `%APPDATA%\Claude\claude_desktop_config.json` |

```json
{
  "mcpServers": {
    "agent-session-grep": {
      "command": "agent-session-grep",
      "args": ["--db", "/path/to/asg.db", "mcp"]
    }
  }
}
```

Restart Claude Desktop after saving; it only reads this file at startup.

If `agent-session-grep` is not on the `PATH` that the desktop app inherits — a
common situation on macOS, where GUI apps do not get your shell's `PATH` — give
the absolute path to the binary as `command` instead of the bare name.

## Cursor

Create `~/.cursor/mcp.json` for every project, or `.cursor/mcp.json` inside one
project. Same shape:

```json
{
  "mcpServers": {
    "agent-session-grep": {
      "command": "agent-session-grep",
      "args": ["--db", "/path/to/asg.db", "mcp"]
    }
  }
}
```

## Cline

Open the MCP servers pane in the Cline sidebar, open the **Configure** tab, and
click **Configure MCP Servers**. That opens `cline_mcp_settings.json`:

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json` |
| Windows | `%APPDATA%\Code\User\globalStorage\saoudrizwan.claude-dev\settings\cline_mcp_settings.json` |

Cline's standalone CLI reads `~/.cline/mcp.json` instead. Either way the entry is
the same `mcpServers` shape:

```json
{
  "mcpServers": {
    "agent-session-grep": {
      "command": "agent-session-grep",
      "args": ["--db", "/path/to/asg.db", "mcp"],
      "disabled": false,
      "autoApprove": ["search_sessions", "get_session_context", "get_status"]
    }
  }
}
```

`autoApprove` is optional. Every tool here is read-only and touches only the one
store named in `--db`, so auto-approving the read tools is a reasonable default.

## Zed

Zed is the one client that does **not** use `mcpServers`. Its key is
`context_servers`, in `settings.json` (`zed: open settings file`, or
Settings → AI → MCP Servers → Add Server, which writes the same entry):

```json
{
  "context_servers": {
    "agent-session-grep": {
      "command": "agent-session-grep",
      "args": ["--db", "/path/to/asg.db", "mcp"],
      "env": {}
    }
  }
}
```

Older Zed versions nested this as `"command": { "path": ..., "args": [...] }`.
If the flat form above is not picked up, check your Zed version's documentation
for which shape it expects.

## The nine tools

`tools/list` returns exactly these. Every tool declares
`additionalProperties: false`, so an unrecognized argument is rejected rather
than ignored.

| Tool | Required arguments | Optional arguments |
| --- | --- | --- |
| `search_sessions` | `query` (≤4096 chars) | `limit` (default 20), `cursor`, `max_items`, `max_bytes` (≥4096), `providers`, `since`, `until`, `include_system`, `group_by_session`, `sidechain` (`include`/`main_only`/`subagent_only`), `tool_kind` (`file`/`command`/`web`/`query`/`unknown`), `tool_name`, `mode` (`lexical`/`semantic`/`hybrid`) |
| `get_session_context` | `session_id` (`ses_v1_…`) | `policy` (`mainline`/`full`), `level` (`raw`/`talks`/`sessions`), `max_messages`, `max_bytes` |
| `get_session_resume` | `session_id` | — |
| `get_message` | `message_id` (`msg_v1_…`) | `session_id`, `around` (default 0), `max_items`, `max_bytes` |
| `list_sessions` | — | `limit` (default 20), `cursor`, `max_items`, `max_bytes` |
| `generate_handoff` | `query` | `limit` (default 50), `max_evidence`, `max_tokens`, `max_bytes`, `providers`, `since`, `until` |
| `list_providers` | — | — |
| `get_status` | — | — |
| `doctor` | — | — |

Notes that matter when you write agent instructions around these:

- **`search_sessions` returns messages, not sessions.** Each hit is a
  `msg_v1_…` id with the owning `session_id`. The reading path is
  `search_sessions` → `get_message` (one message plus neighbors) or
  `get_session_context` (the whole branch).
- **`list_sessions` returns only session entities**, in stable wire-id order.
  Messages and documents are not listed.
- **`since` and `until` accept absolute ISO-8601 timestamps only** over MCP, for
  example `2026-08-01T00:00:00Z`. The compact `1h`/`1d`/`1w` durations the CLI
  accepts are rejected here: the protocol carries no shared clock reference. The
  interval is half-open, `[since, until)`.
- **`mode: "semantic"` or `"hybrid"` needs `asg index embeddings` to have been
  run.** When the vector index is not ready, the response reports
  `retrieval_mode: "lexical_fallback"` with a warning rather than silently
  answering a different question.
- **`get_session_resume` returns metadata, never a command.** If you want to
  resume a native session, take `provider_session_id` and run the provider's own
  resume flow yourself.
- **`generate_handoff` reports budget truncation as `outcome: "partial"`** rather
  than as success.

## A real exchange

One JSON message per line, in both directions. Below is an actual session,
reformatted only by line-wrapping.

Handshake — request:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"demo","version":"1"}}}
```

Response:

```json
{"id":1,"jsonrpc":"2.0","result":{"capabilities":{"tools":{}},"protocolVersion":"2025-06-18","serverInfo":{"name":"agent-session-grep","version":"0.1.0"}}}
```

Then the client sends the initialized notification, which gets no reply:

```json
{"jsonrpc":"2.0","method":"notifications/initialized"}
```

Until that notification arrives, only `initialize` and `ping` are answered.
Supported protocol versions are `2025-06-18`, `2025-03-26`, and `2024-11-05`; a
requested version outside that set negotiates down to `2025-06-18` rather than
falsely echoing what you asked for.

A tool call — request:

```json
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_sessions","arguments":{"query":"alpha","limit":3}}}
```

Response (the payload appears twice, once serialized in `content[0].text` and
once as `structuredContent`):

```json
{"id":4,"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"…"}],"isError":false,"structuredContent":{"data":{"generation":1,"hits":[{"id":"msg_v1_…","resume_available":false,"score":1.0e-6,"session_id":null,"text":null}],"retrieval_mode":"lexical","truncation":{"reason":null,"truncated":false}},"outcome":"success","page":{"has_more":false,"next_cursor":null},"warnings":[]}}}
```

Every successful tool result has the same envelope shape:
`{ outcome, data, warnings, page }`.

### Verify it by hand before configuring a client

Worth doing once: it separates "the server is broken" from "the client
configuration is wrong". Pipe the three lines in and expect three response lines
and a clean exit — the server shuts down on stdin EOF.

```
# macOS / Linux
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_status","arguments":{}}}' \
  | agent-session-grep --db /path/to/asg.db mcp
```

```powershell
# Windows PowerShell
@(
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}'
  '{"jsonrpc":"2.0","method":"notifications/initialized"}'
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_status","arguments":{}}}'
) | & "C:\path\to\agent-session-grep.exe" --db "C:\path\to\asg.db" mcp
```

A `get_status` result with a nonzero `catalog_count` means the binary, the store,
and the protocol path are all sound, and anything still wrong is in the client
configuration.

### How failures are reported

The server splits failures into two layers deliberately, and a client needs to
handle both:

| Failure kind | Shape | Example |
| --- | --- | --- |
| Protocol or argument problem | JSON-RPC `error` object, `-32602`, with `data.canonical_code` and `data.retryable` | `{"error":{"code":-32602,"data":{"canonical_code":"invalid_request","retryable":false},"message":"unknown tool: nope"}}` |
| Business failure after valid arguments | Successful JSON-RPC response with `isError: true` and a canonical error in `structuredContent` | `cursor_expired`, `not_found` |

Other protocol codes: `-32700` for unparseable JSON, `-32600` for a non-object
message (JSON-RPC batching is not supported), a wrong `jsonrpc` value, or a
malformed `id`, and `-32601` for an unknown method. Echoed values in error
messages are truncated at 128 characters, so a huge bad argument cannot inflate
the error frame.

This server implements the tools capability only. The served methods are
`initialize`, `ping`, `tools/list`, `tools/call`, and
`notifications/initialized`, plus `notifications/cancelled` as a best-effort
no-op. `resources/list` and `prompts/list` both return `-32601 method not
found` — that is expected, not a misconfiguration, and a client that treats a
missing resources capability as fatal will need its own handling.

## Keeping the index fresh

**This is the part with no automation.** There is no `watch` command, and the MCP
server is strictly read-only — it will never index anything for you. Whatever
`sync` last committed is exactly what your agent searches. If you configure MCP
once and never sync again, the agent is querying a frozen snapshot while
confidently reporting that it searched your history.

Re-run `asg sync --discover` on a schedule that matches how you work. Some
options:

- Manually, before a session where history matters.
- A scheduled task or cron job, for example every 15 minutes or hourly.
- A shell alias or hook that syncs before you start your agent.

Concretely, hourly:

```
# macOS / Linux — crontab -e
0 * * * * /usr/local/bin/agent-session-grep --db /path/to/asg.db --robot sync --discover
```

```powershell
# Windows — Task Scheduler
$action  = New-ScheduledTaskAction -Execute "C:\path\to\agent-session-grep.exe" `
                                   -Argument "--db C:\path\to\asg.db --robot sync --discover"
$trigger = New-ScheduledTaskTrigger -Once -At (Get-Date) `
                                    -RepetitionInterval (New-TimeSpan -Hours 1)
Register-ScheduledTask -TaskName "agent-session-grep sync" -Action $action -Trigger $trigger
```

Use `--robot` in a schedule so the output is a single parseable envelope, and use
an absolute path to the binary — a scheduler's `PATH` is not your shell's.

Two operational facts that make this cheap and safe to over-schedule:

- `sync` writes nothing when nothing changed, so no new generation is created
  and no reader is disturbed.
- Only one writer per data root at a time (an exclusive lease). Readers — MCP
  clients, `serve`, hooks — are unaffected and any number can run concurrently.
  But two overlapping `sync` runs will collide: the second fails with
  `writer_busy` (exit 6, retryable). If your schedule interval is shorter than a
  full sync takes, serialize it.

One consequence worth knowing: **paging cursors die when new data arrives.** A
`page.next_cursor` is a stateless signed token that survives a server restart,
but it carries the index generation. Once a `sync` advances the generation, the
token fails with `generation_mismatch`. Cursors also expire after 15 minutes.
Either way the recovery is the same — re-issue the query from page one, never
retry the token.

## When the server will not start

| Symptom | Cause | Fix |
| --- | --- | --- |
| Client reports the server exited immediately; no protocol error | Exit 4, `no store at <path>` — the database does not exist | Run `asg sync --discover`, and check the path in your client config for typos |
| Same, with `mcp takes no positional arguments` | Exit 2 — an extra argument after `mcp` | `mcp` takes no positional arguments. `--db <path>` belongs **before** `mcp`, not after |
| Client cannot find the executable | The GUI app's `PATH` does not include the install directory | Use an absolute path to the binary as `command` |
| Server starts but every search returns nothing | The store exists but is empty, or is not the store you think it is | Call the `get_status` tool: `catalog_count: 0` means nothing is indexed. `doctor` reports schema and generation |
| Tools work but results are stale | The index has not been synced recently | See [Keeping the index fresh](#keeping-the-index-fresh) |

Because `--db <path>` is a global flag it must precede the subcommand. The
argument order in every example above — `--db <path>` then `mcp` — is the only
one that works.

## Security properties

Worth stating explicitly, since you are handing an agent access to your entire
coding history:

- **Read-only.** The MCP server never modifies your history and never writes to
  the catalog.
- **One store, no filesystem access.** It opens only the `--db` store given at
  startup. There is no arbitrary file read, no SQL passthrough, and no command
  execution surface.
- **Redacted by default.** MCP output crosses a trust boundary into another
  agent's context, so it goes through cross-boundary redaction. Human CLI output
  does not.
- **No network.** Every operation is local.
- **Requests are sequential** in this version. `notifications/cancelled` is
  accepted but is a best-effort no-op.

## Known divergences

Verified against this build. These are defects here, not in your client.

### The declared `providers` enum is stale and unenforced

`search_sessions` and `generate_handoff` declare their `providers` argument as
`enum: ["claude", "claude-code", "codex"]` with `maxItems: 2`. The running server
is more permissive than that: it accepts every canonical provider id the CLI
accepts, and any number of them. A genuinely unknown value is rejected, but with
a message that understates the accepted set:

```
providers must contain only claude|claude-code|codex, got bogus
```

The accurate list is the CLI's: `claude` (an alias for `claude-code`), plus
`aider`, `antigravity`, `claude-code`, `cline`, `codex`, `cursor`, `grok-build`,
`hermes`, `kimi-code`, `openclaw`, `opencode`, `pi`, `qoder`, and
`tencent-codebuddy`. A strict client that validates arguments against the
declared schema before sending will reject provider filters the server would
have honored.

### `generate_handoff` redacts its own budget numbers

In the response, `data.budget.max_tokens` and `data.budget.used_tokens` come back
as the string `"[redacted]"`. The redaction ruleset matches any key whose name
contains `token`, and these two are caught by it even though they are plain
integers, not secrets. A caller cannot read back the token budget it just set,
and a client that expects a number there will fail to parse the field.

`max_bytes`, `used_bytes`, and `max_evidence` in the same object are unaffected,
so use `used_bytes` against `max_bytes` if you need to see how close a pack came
to its ceiling.

### `serverInfo.version` reports the crate version

The handshake returns `"version": "0.1.0"`, which is the crate version rather
than the release version. Do not use it for feature detection.
