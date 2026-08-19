# Providers

Two questions this page answers for each provider: **where does it keep its
transcripts**, and **what can our adapter not parse**.

All 14 implemented adapters are **Experimental** maturity. That is a fact about
evidence, not a hedge: each one has golden and property tests, but none has
cleared the cross-platform certification and per-provider rollback gates needed
for Beta. Two further providers (DeepSeek Harness, ZCode) are **deferred** — we
have no transcript sample, so there is no adapter and we make no claims.

Transcripts are opened read-only. `agent-session-grep` never writes to,
modifies, or uploads a provider's session data.

The authoritative sources for this page are
`crates/agent-session-grep-ports/src/capability.rs` (capabilities) and
`provider_root_subpath()` in `crates/agent-session-grep-cli/src/main.rs`
(discovery roots). If this page and those disagree, the code is right.

## Where your transcripts live

### Providers `sync --discover` scans

Six adapters register a discovery root, so `asg sync --discover` finds them
with no arguments. The root is the *same home-relative subpath on all three
platforms* — the code joins your home directory to the subpath with no
per-platform branching. Home is `$HOME`, falling back to `%USERPROFILE%` on
Windows.

| Provider | `provider_id` | Discovery root (relative to home) |
|---|---|---|
| Claude Code | `claude-code` | `.claude/projects` |
| Codex | `codex` | `.codex/sessions` |
| OpenClaw | `openclaw` | `.openclaw/agents` |
| Tencent CodeBuddy | `tencent-codebuddy` | `.codebuddy/projects` |
| Antigravity | `antigravity` | `.gemini/antigravity-cli/brain` |
| OpenCode | `opencode` | `.local/share/opencode` |

Within the Antigravity root, transcripts sit at
`brain/<uuid>/.system_generated/logs/transcript.jsonl`; the session identity is
the directory name, not a field in the file.

One caveat on OpenCode: `.local/share/opencode` is an XDG-style subpath, and it
is used verbatim on Windows and macOS as well. If your OpenCode installation
keeps its database somewhere else on those platforms, `--discover` will not see
it — pass the `opencode.db` path explicitly.

`--discover` identifies candidates by asking each adapter's `probe` to inspect
the file contents, not by extension. SQLite databases, whole-file JSON, and
Markdown chat logs are all discoverable.

### Providers you must point at explicitly

The other eight adapters register no discovery root. `sync --discover` will
never find them — it says so per provider, in these words:

```
  cursor: no discovery root — pass transcripts explicitly: asg sync <file>...
```

They are fully searchable once ingested; you just have to name the files:

```bash
asg sync path/to/transcript.jsonl
asg sync --from-file sources.txt
```

For most of these, **this repository has no verified evidence of the directory
they live in.** We know the file shape (it is what the adapter parses) but not
reliably the path, so no path is asserted below. Where a location is documented
in the adapter source, it is repeated here with its confidence stated.

| Provider | `provider_id` | What to point `sync` at | Location evidence |
|---|---|---|---|
| Grok Build | `grok-build` | `updates.jsonl` (ACP `session/update` stream; written per session next to a `summary.json`) | Directory not documented in this repo. |
| Pi | `pi` | Session JSONL (`type:"session"` header line, then `type:"message"` lines) | Not documented in this repo. |
| Kimi Code | `kimi-code` | `wire.jsonl` | Only `~/.kimi-code` is named in the adapter, as a directory that did not exist on the machine where the adapter was written. The transcript's location inside it is unverified. |
| Qoder | `qoder` | CLI transcript JSONL | Adapter targets `~/.qoder/projects/<project>/`, but records that whether files sit directly there or under a `transcript/` subdirectory is **unverified** — the reference matrix and Qoder's own CLI docs disagree, and settling it needs a real `qodercli` run. |
| Cline | `cline` | `api_conversation_history.json` | Adapter documents `~/.cline/data/tasks/*/`. |
| Hermes | `hermes` | `session_<id>.json` | Adapter documents `~/.hermes/sessions/session_<id>.json` — see the upstream-deprecation warning below. |
| Cursor | `cursor` | `state.vscdb` (the VS Code `workspaceStorage` SQLite KV file) | Directory not documented in this repo. |
| Aider | `aider` | `.aider.chat.history.md` | Filename only; the directory it is written to is recorded as an unverified candidate. |

Some of these formats are byte-shaped alike. When two adapters `probe` a file
with equal confidence, `sync` **refuses the file rather than guessing**:

```
$ asg sync session.jsonl
error [invalid_request]: invalid request: ambiguous provider selection: 2 variants matched
with equal confidence (openclaw/session-jsonl-v3, pi/session-jsonl-v1); this source's shape
is not distinctive enough to attribute, so it is refused rather than guessed. Re-run with
`sync --provider <id> <file>` to name the provider explicitly
```

Exit code is `2`. Break the tie yourself:

```bash
asg sync --provider pi session.jsonl
```

Pi and OpenClaw genuinely collide this way. Naming the provider is the intended
fix, not a workaround.

## Capabilities

Derived from `ProviderCapabilityMatrix::current()`. `native` means the provider
supplies it directly, `derived` means we compute it deterministically from
content, `partial` means it is available in some cases, `unsupported` means the
provider has no such concept, and `unknown` means we have not evaluated it and
will not claim it.

`asg --robot providers` prints this matrix as JSON, which is the copy you
should script against.

| Provider | Maturity → target | discover | parse | resume | tool activity | source span | incremental | context |
|---|---|---|---|---|---|---|---|---|
| `claude-code` | experimental → certified | native | native | derived | partial | native | native | native |
| `codex` | experimental → certified | native | native | derived | partial | native | native | native |
| `grok-build` | experimental → beta | unsupported | native | derived | unsupported | native | unsupported | unsupported |
| `pi` | experimental → beta | unsupported | native | derived | unsupported | native | unsupported | unsupported |
| `openclaw` | experimental → beta | native | native | unsupported | unsupported | native | unsupported | unsupported |
| `tencent-codebuddy` | experimental → beta | native | native | unknown | unsupported | native | unsupported | unsupported |
| `antigravity` | experimental → beta | native | native | unknown | unsupported | unsupported | unsupported | unsupported |
| `opencode` | experimental → beta | native | native | unknown | unsupported | unsupported | unsupported | unsupported |
| `kimi-code` | experimental → beta | unsupported | native | unknown | unsupported | native | unsupported | unsupported |
| `qoder` | experimental → beta | unsupported | native | unknown | unsupported | native | unsupported | unsupported |
| `hermes` | experimental → beta | unsupported | native | unknown | unsupported | unsupported | unsupported | unsupported |
| `aider` | experimental → experimental | unsupported | native | unsupported | unsupported | derived | unsupported | unsupported |
| `cline` | experimental → experimental | unsupported | native | unsupported | unsupported | unsupported | unsupported | unsupported |
| `cursor` | experimental → experimental | unsupported | native | unknown | unsupported | unsupported | unsupported | unsupported |
| `deepseek-harness` | unsupported (deferred) | unknown | unknown | unknown | unknown | unknown | unknown | unknown |
| `zcode` | unsupported (deferred) | unknown | unknown | unknown | unknown | unknown | unknown | unknown |

`probe` and `search` are `native` for all 14 implemented adapters, and `handoff`
is `unsupported` for all of them, so those columns are omitted.

Reading the table:

- **`incremental: native` for only Claude Code and Codex.** For every other
  provider, re-syncing a changed file re-reads it whole. Correct, just not
  cheap.
- **`source_span: unsupported`** (OpenCode, Hermes, Antigravity, Cursor, Cline)
  means hits from that provider carry no byte offsets into the source file, so
  you cannot verify a quote against a file position. SQLite-backed and
  whole-file-JSON formats have no in-file byte range to point at.
- **`tool_activity`** is `partial` for Claude Code and Codex and `unsupported`
  everywhere else. The `--tool-kind` and `--tool-name` search filters therefore
  only ever match Claude Code and Codex messages.
- **`resume: unknown`** is a deliberate refusal, not an oversight. `asg resume`
  reports `available: false` for those providers rather than inventing a command
  line. Only four have verified templates: `claude --resume <id>`,
  `codex resume <id>`, `pi --session <id>`, and `grok --resume <id>`.

## What each adapter cannot parse

These are the adapters' own declared `known_limitations`, quoted from the
provider crates.

### Claude Code (`claude-code`)

- Tool activity extraction is partial.
- `turn_context` metadata is not surfaced as canonical messages.

### Codex (`codex`)

- Tool activity extraction is partial.
- `turn_context` metadata is not surfaced as canonical messages.

### Grok Build (`grok-build`)

- Chunk grouping reconstructs roles; the ACP update stream carries no
  per-message native id, so message identity is reconstructed document-scoped by
  the ingestion layer (Unstable).
- Per-message timestamps come from the first chunk of each grouped message (the
  ACP `timestamp` envelope field, already RFC3339 — the message's creation time,
  not a replay time); chunks without a usable value carry no timestamp.
- Session identity falls back to the first ACP `promptId` seen, not a durable
  session id.

### Pi (`pi`)

- Non-conversational types (`session_info` / `compaction` / `custom_message`)
  are skipped.
- The format carries no per-message native id, so message identity is
  reconstructed document-scoped by the ingestion layer (Unstable).

### OpenClaw (`openclaw`)

- Resume is intentionally unsupported (gateway-managed).
- The format carries no per-message native id, so message identity is
  reconstructed document-scoped by the ingestion layer (Unstable).

### Tencent CodeBuddy (`tencent-codebuddy`)

- The root startup-keyword user message (content: `"code"`) is filtered out.
- No working-directory pair observation (no separate `cwd`-bearing header
  record).
- The format carries no per-message native id, so message identity is
  reconstructed document-scoped by the ingestion layer (Unstable).

### Antigravity (`antigravity`)

- Session identity lives in the `brain/<uuid>` directory name, not in the
  transcript file; `session_native_id` is left unset.
- `SYSTEM` / `CONVERSATION_HISTORY` steps and tool activity are never emitted as
  messages.
- `step_index` is only a per-transcript counter, not a globally unique message
  id, so message identity is reconstructed document-scoped by the ingestion
  layer (Unstable).

### OpenCode (`opencode`)

- SQLite source has no byte spans; messages are attributed without source
  offsets.
- Only text parts with role `user`/`assistant` are committed; tool and other
  parts are ignored.
- Per-message timestamps come from `message.time_created` (epoch milliseconds);
  rows whose value is missing, non-positive, or out of range carry no timestamp.
- ZCode's CLI database forks this schema (same `session`/`message`/`part`
  tables, same `$.role` and `$.type`=`'text'`/`$.text` shapes); the probe
  refuses it on the `schema_migration` vs `__drizzle_migrations` fingerprint
  rather than attributing it to OpenCode.

### Kimi Code (`kimi-code`)

- Only `context.append_message` records are parsed; all other record types are
  skipped, including `context.append_loop_event` (step/tool events),
  `metadata`, `config.update`, `turn.prompt`, and `usage.record`.
- The skipped loop events are where tool activity lives, so no tool activity is
  extracted and `tool_activity` is declared Unsupported; extracting it needs a
  real sample first (the golden fixture's only loop event is a bare
  `step.begin`, so a `tool.call`'s shape is unobserved).
- Session id is rarely carried in `wire.jsonl`; `session_native_id` is usually
  left unset.
- Per-message timestamps come from the record-level `time` field (epoch
  milliseconds); records whose value is missing, non-positive, or out of range
  carry no timestamp.
- `context.append_message` records carry no per-message native id, so message
  identity is reconstructed document-scoped by the ingestion layer (Unstable).

### Qoder (`qoder`)

- Identity fields are matched leniently from `session_meta`
  (`session_id`/`cwd`).
- Non-conversational records (`progress`/`tool_use`/`tool_result`) are skipped.
- The format carries no per-message native id, so message identity is
  reconstructed document-scoped by the ingestion layer (Unstable).
- Targets the CLI transcript JSONL under `~/.qoder/projects/<project>/`;
  whether transcripts sit directly there or under a `transcript/` subdirectory
  is unverified (the reference matrix and Qoder's CLI docs disagree) and needs
  a real `qodercli` run to settle.
- The Qoder IDE's
  `~/.qoder/cache/projects/<project>-<hash>/conversation-history/<id>.txt` is
  plain text, not JSONL (verified on real files), and is **not** readable by
  this variant.

### Hermes (`hermes`)

- This variant reads a **historical layout**: upstream Hermes has made
  `~/.hermes/state.db` (SQLite + FTS5) the canonical session store and states
  that per-session files under `~/.hermes/sessions/` are no longer written or
  read, so **a newly created session produces nothing this adapter can read.**
- The current `state.db` store is **not** supported (no sample observed, schema
  unverified); only files retained from an older installation are parseable.
- Sibling `<id>.jsonl` files (partial recent state) are ignored; only
  `session_<id>.json` is parsed.
- No byte spans (whole-file JSON); message timestamps fall back to
  `session_start` when absent.
- Message objects carry no native id (only their array position), so message
  identity is reconstructed document-scoped by the ingestion layer (Unstable).

### Cursor (`cursor`)

- SQLite source has no byte spans.
- `chatdata`/`prompts` are multi-generation formats; version layering is not yet
  implemented.
- Bubbles and prompt records carry no per-message native id (tab and prompt ids
  are shared by several messages), so message identity is reconstructed
  document-scoped by the ingestion layer (Unstable).

### Cline (`cline`)

- No session id in the JSON array file; `session_native_id` is left unset.
- No byte spans (whole-file JSON array); the format carries no per-message
  native id, so message identity is reconstructed document-scoped by the
  ingestion layer (Unstable).
- Per-message timestamps come from the optional `timestamp` field (RFC3339
  string or epoch milliseconds); records without one, or whose value is
  non-positive or out of range, carry no timestamp **and therefore fall outside
  every `--since`/`--until` window.**

### Aider (`aider`)

- Spans are derived approximations (block start + text length), not byte-exact
  line slices.
- Tool and edit blockquote output is folded into assistant text — which is why
  Aider reports no tool activity at all.
- Session identity is the first `# aider chat started at` header timestamp.
- The Markdown chat log carries no per-message native id, so message identity is
  reconstructed document-scoped by the ingestion layer (Unstable).

### DeepSeek Harness (`deepseek-harness`) and ZCode (`zcode`)

Deferred. No adapter exists, because no transcript sample has been observed. We
list them so the count is honest, not to imply pending support.

## Two limitations that cut across providers

**Missing timestamps silently narrow time filters.** Several formats carry
optional or absent per-message timestamps (Cline, Kimi Code, OpenCode, Grok
Build, and Hermes as noted above). A message with no timestamp does not fall
inside *any* `--since`/`--until` window, so a time-filtered search will not
return it. If a search comes back emptier than you expect, drop the time
filter.

**Reconstructed message identity is Unstable.** Only Claude Code and Codex carry
per-message native ids. For every other provider the ingestion layer
reconstructs identity document-scoped, which means a `msg_v1_...` id from those
providers is not guaranteed to be stable across a re-ingest of a rewritten
source file. Session ids and search results are unaffected.

## See also

- [Quickstart](quickstart.md) — install, index, search.
- [Provider Maturity Matrix](../product/PROVIDER-MATURITY-MATRIX.md) — the
  16-provider roadmap, per-provider evidence, and the gaps blocking Beta.
