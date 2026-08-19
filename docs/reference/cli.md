# CLI reference

Complete surface of the `agent-session-grep` binary: every subcommand, every
flag, and the full exit-code table.

Everything below was verified against a release build
(`cargo build --release -p agent-session-grep-cli`) by running the binary, not
by reading `--help` alone. Where the built-in help text and the actual accepted
arguments disagree, this page documents what the binary really does and marks
the divergence — see [Help-text divergences](#help-text-divergences).

- Machine-readable envelope schema: [`schemas/robot/v1.1/envelope.schema.json`](../../schemas/robot/v1.1/envelope.schema.json)
- Canonical error catalog: [`schemas/robot/v1/error-catalog.json`](../../schemas/robot/v1/error-catalog.json)
- Handoff pack schema: [`schemas/handoff/v1/pack.schema.json`](../../schemas/handoff/v1/pack.schema.json)
- Recovery procedures for storage-level failures: [rebuild and migration runbook](../operations/rebuild-and-migration-runbook.md)
- Error-message index: [troubleshooting guide](../guide/troubleshooting.md)

## Invocation grammar

```
agent-session-grep [GLOBAL FLAGS] <COMMAND> [ARGS] [COMMAND FLAGS]
agent-session-grep doctor [--db <path>]
agent-session-grep --help | --version
```

Flag position is significant and is enforced by the parser:

- **Global flags go before the command name.** A token that looks like a global
  flag but appears after the command name is treated as a positional argument
  by the command, not as a global flag. This is deliberate: it lets you search
  for the literal string `--robot`.
- **Command flags go after the command name.** `--max-items`, `--cursor`,
  `--since`, and friends are extracted per command.
- `doctor` is the one exception: it accepts `--db` either before or after the
  command name.

The first bare (non-`-`) token is the command name. An unknown command name is
rejected before the store is opened, with a Levenshtein "did you mean"
suggestion and the full command list.

## Store resolution

The store (a SQLite file) is resolved in this order:

1. `--db <path>`
2. `$ASG_DB`
3. `<platform data dir>/asg.db` — run `agent-session-grep config paths` to see
   the platform data dir.

Read and write commands treat a missing store differently, on purpose:

- **Write commands** (`ingest`, `sync`, `index`) create the parent directory and
  the store if needed.
- **Read commands** (everything else) never create a store. A missing store is
  `not_found` / exit 4 with the path and the command that would build it, so a
  mistyped `--db` fails loudly instead of returning a clean empty result from a
  freshly created empty database.

Write commands take an exclusive `writer.lock` lease in the store's parent
directory; read commands do not, so any number of readers can run concurrently.

## Global flags

| Flag | Value | Effect |
| --- | --- | --- |
| `--db <path>` | filesystem path | Store path. Optional; see [Store resolution](#store-resolution). Empty string, a missing value, a value that is itself a known flag name, and a repeated `--db` are all usage errors. |
| `--output <mode>` | `human` \| `json` \| `jsonl` | Output mode. Default `human`. `json` emits one envelope; `jsonl` emits one complete frame per line and may emit `frame_type: "progress"` frames during long `sync` runs. |
| `--robot` | — | Equivalent to `--output json`. Cannot be combined with `--output` (conflict is a usage error). |
| `--request-id <id>` | `[A-Za-z0-9._:-]{1,128}` | Correlation id echoed verbatim in every frame. An invalid value is a usage error rather than being silently replaced. |
| `--offline` | — | Fail-closed refusal of any operation that would need the network. No current command needs the network, so this changes no behaviour today; `doctor` and `hook` report it honestly in their `offline` field. |
| `--no-color` | — | **Accepted and ignored.** The parser recognizes the token so it does not become a command name, but no code path reads it. Human output emits no ANSI sequences in the first place. |
| `-h`, `--help` | — | Top-level help when it appears before any command name; per-command help when it directly follows a known command name. |
| `-V`, `--version` | — | Version line. If both `--help` and `--version` are present, help wins. |

`<command> --help` never opens the store, never creates a file, and never takes
the writer lease.

## Commands

Twenty-one commands, as listed by `KNOWN_SUBCOMMANDS`:
`ingest`, `sync`, `index`, `search`, `handoff`, `get-message`,
`get-session-resume`, `resume`, `hook`, `get`, `show`, `list`, `context`,
`status`, `mcp`, `tui`, `serve`, `doctor`, `providers`, `config`, `model`.

### `search <query>`

Full-text search, hits in descending relevance order. Returns message-level
entities (`msg_v1_…`), each carrying the owning `session_id`, a `score`, a text
summary, and `suggested_next_commands`.

| Flag | Value | Default | Notes |
| --- | --- | --- | --- |
| `--max-items <n>` | integer | 20 | Page size and response item budget. |
| `--max-bytes <n>` | integer | — | Response byte budget. Floor 4096; below it is a usage error. |
| `--cursor <token>` | token | — | Continuation token from the previous page's `page.next_cursor`. |
| `--provider <id>` | provider id | all | Repeatable; multiple values are OR-ed. |
| `--since <time>` | RFC3339/ISO-8601 or `1h`/`1d`/`1w` | — | Inclusive lower bound. |
| `--until <time>` | same as `--since` | — | Exclusive upper bound; interval is half-open `[since, until)`. |
| `--mode <mode>` | `lexical` \| `semantic` \| `hybrid` | `lexical` | `semantic`/`hybrid` require `index embeddings`. If the vector index is not ready the response reports `retrieval_mode=lexical_fallback` with a warning; it never degrades silently. |
| `--include-system` | — | off | Include `system`/`developer` role messages, which are excluded by default. |
| `--group-by-session` | — | off | Collapse to the best-scoring hit per session, with an `occurrences` count. |
| `--main-only` | — | off | Mainline messages only (exclude sidechain). Mutually exclusive with `--subagent-only`. |
| `--subagent-only` | — | off | Sidechain (subagent) messages only. |
| `--include-sidechain` | — | on | Explicitly request the default. Conflicts with `--main-only` and with `--subagent-only`. **Not mentioned in `search --help`.** |
| `--tool-kind <kind>` | `file` \| `command` \| `web` \| `query` \| `unknown` | — | Keep only messages carrying a tool activity of this kind. |
| `--tool-name <name>` | string | — | Keep only messages carrying a tool activity with this exact name (byte equality). |

Accepted `--provider` values are derived from the capability matrix, not
hard-coded. As of this build:

```
claude aider antigravity claude-code cline codex cursor grok-build
hermes kimi-code openclaw opencode pi qoder tencent-codebuddy
```

`claude` is a historical alias for `claude-code`. An unrecognized value is a
usage error that prints the whole accepted list.

Non-default facets are echoed back in `data.facets`; when every facet is at its
default, nothing is added, so existing output bytes are unchanged.

### `handoff <query>`

Search, then assemble a deterministic handoff pack (`handoff-pack/v1`). Verbatim
evidence and inference stay in separate sections. No LLM call, and nothing is
injected into any agent — it prints the pack and stops.

Because no model is called, `inference` is **always an empty array**: the
separation is a structural guarantee that a future summarizer cannot write into
`evidence`, not a promise of summary content. `provenance` is present only when
exactly one session matched — a pack spanning several sessions has no single
origin, so the field is omitted rather than guessed. Each entry in
`matched_sessions` carries its own `provider_id` either way, omitted when the
store holds no resume claim for that session.

| Flag | Value | Default |
| --- | --- | --- |
| `--max-evidence <n>` | integer | 20 |
| `--max-tokens <n>` | integer | 8000 |
| `--max-bytes <n>` | integer | 2000000 |
| `--provider <id>` | provider id (repeatable, OR) | all |
| `--since <time>` / `--until <time>` | as in `search` | — |

Budget truncation is reported as `outcome: partial` and **exit 10**, never as
success.

### `get-message <message-id>`

One message plus a bounded window of its same-session mainline neighbours.

| Flag | Value | Default |
| --- | --- | --- |
| `--session <ses-id>` | session wire id | — (required when the message id is shared across sessions) |
| `--around <n>` | non-negative integer | 0 (anchor only) |
| `--max-items <n>` | integer | — |
| `--max-bytes <n>` | integer | floor 4096 |

### `get <wire-id>`

Return the raw stored payload for one entity id. No flags. A well-formed id that
matches nothing is `not_found` / exit 4 with the fixed message `entity not
found` — the wire id is deliberately not echoed.

### `show <wire-id>`

Return one entity normalized for display (role, text, timestamp). No flags. Same
`not_found` behaviour as `get`.

### `list [limit]`

Page catalog entities in stable wire-id order.

| Argument / flag | Value | Default |
| --- | --- | --- |
| `limit` (positional) | integer | 20, or `--max-items` when that is given |
| `--cursor <token>` | token | — |
| `--max-items <n>` | integer | 20 |
| `--max-bytes <n>` | integer | floor 4096 |

`--max-items` and `--max-bytes` are accepted here even though `list --help`
mentions only `--cursor`.

### `context <session-id>`

Assemble one session: the branch message chain plus evidence spans.

| Flag | Value | Default |
| --- | --- | --- |
| `--policy <policy>` | `mainline` \| `full` | `mainline` (walks the parent chain, excludes sidechains) |
| `--level <level>` | `raw` \| `talks` \| `sessions` | `raw` |
| `--max-messages <n>` | integer | — |
| `--max-bytes <n>` | integer | floor 4096 |

An empty derived view falls back toward more detail (`sessions` → `talks` →
`raw`) and reports `effective_level`.

### `get-session-resume <session-id>`

Read-only resume metadata for one session. No flags. Fixed field shape, with
`null` for anything unknown: `provider_session_id`,
`original_working_directory`. It never builds or runs a resume command and never
returns a transcript path.

### `resume <session-id>`

Preview (default) or perform in-place session resumption.

| Flag | Effect |
| --- | --- |
| `--yes` | Actually spawn the provider process in the original working directory. |

Behaviour worth knowing before you rely on it:

- Without `--yes` this is a dry run: it prints the full command
  (provider, cwd, session id) and exits.
- The **first ever** `resume` invocation is forced to preview only, even with
  `--yes`, and writes a persistent acknowledgement marker. `--yes` takes effect
  from the second invocation onward.
- If the marker cannot be written, the command stays in forced-preview mode
  (fail closed) and says so in a warning.
- Providers whose resume command has not been verified report
  `available: false` with an `unavailable_reason`. That is a success, not an
  error: history stays searchable even when it cannot be resumed. No command is
  ever invented.
- `permission_mode` is always `null` and `permission_mode_verified` is always
  `false`, for every provider. That is the policy, not a gap: resume never adds
  a permission flag of its own, and the read-only resume metadata does not carry
  the provider's real approval mode, so the tool will not claim to have checked
  it. Read `permission_mode: null` as "no permission flag was added", not as
  "confirmed to run in the default mode".

### `hook <event>`

Claude Code hook integration. Reads the hook payload from stdin and writes the
`hookSpecificOutput` contract to stdout. **Off by default**: without `--enable`
it emits an empty context and injects nothing.

`<event>` is `session-start` or `user-prompt-submit` (the CamelCase spellings
`SessionStart` and `UserPromptSubmit` are also accepted).

| Flag | Value | Default |
| --- | --- | --- |
| `--enable` | — | off |
| `--max-tokens <n>` | integer | 2000 |
| `--provider <id>` | provider id (repeatable, OR) | all |
| `--decay-days <n>` | integer, `0` = no time filter | 0 |

The query comes from `payload.prompt` for `user-prompt-submit` and from
`payload.cwd` for `session-start`. A blank or missing query injects nothing
rather than guessing. An empty stdin is accepted. Injected text is
cross-boundary redacted (ADR-0009).

### `sync <file>...`

Atomically scan one or more transcript files into the store. Nothing is written
when nothing changed. Sources are opened read-only.

| Form / flag | Meaning |
| --- | --- |
| `sync <file>...` | Explicit paths. Directories are rejected. |
| `sync --from-file <list>` | Read paths from a manifest, one per line; blank lines and `#` comments are ignored. Use this when several thousand paths would blow past the command-line length limit. Mutually exclusive with `--discover`; extra positional paths are rejected; an empty manifest is a usage error. |
| `sync --discover` | Walk each provider's data root and sync what it finds. Accepts no paths and no other flags. Mutually exclusive with `--from-file` and with `--provider`. |
| `sync --provider <id> <file>...` | Name the provider explicitly to break a tie. Needed when two adapters' on-disk formats are the same shape (for example `pi` and `openclaw`), where content probing can only report a tie and refuses to guess. |

`--provider` here accepts adapter ids only — the 14 ingestible providers, with
no `claude` alias:

```
aider antigravity claude-code cline codex cursor grok-build hermes
kimi-code openclaw opencode pi qoder tencent-codebuddy
```

Only **6** of those register a discovery root, so `sync --discover` can only
find sources for these:

| Provider | Data root |
| --- | --- |
| `claude-code` | `~/.claude/projects` |
| `codex` | `~/.codex/sessions` |
| `openclaw` | `~/.openclaw/agents` |
| `tencent-codebuddy` | `~/.codebuddy/projects` |
| `antigravity` | `~/.gemini/antigravity-cli/brain` |
| `opencode` | `~/.local/share/opencode` |

The other eight (`aider`, `cline`, `cursor`, `grok-build`, `hermes`,
`kimi-code`, `pi`, `qoder`) have no root registered and report
`root_state: "unsupported"` in the discover report — they must be synced by
explicit path. Per-provider `root_state` is one of `unsupported`,
`home_unresolved`, `missing`, or `scanned`.

Discovery decides what is a source by asking each adapter's probe, not by file
extension, so SQLite, whole-file JSON, and Markdown transcripts are all
discoverable. Files that cannot be attributed are skipped and counted, never
guessed.

A single transcript file should contain a single session. When several session
ids are detected the first session still wins, and the rest is reported in
`warnings`.

### `ingest <file>`

Parse one file into the store. No flags. Same single-session constraint as
`sync`, reported as a diagnostic warning.

`sync` supersedes `ingest` for normal use; `ingest` remains as the
single-file path.

### `index <subcommand>`

| Subcommand | Effect |
| --- | --- |
| `index rebuild` | Reproject the whole FTS index from the authoritative catalog. |
| `index embeddings` | Build the semantic vector index from the catalog. Required before `--mode semantic` / `--mode hybrid` leave `lexical_fallback`. |
| `index compact` | `VACUUM` the store to reclaim freelist pages. Reports `bytes_before`, `bytes_after`, `bytes_reclaimed`. Deliberately not folded into `sync`: it takes an exclusive lock and needs temporary space roughly the size of the store. |
| `index purge-activities` | Prune orphaned tool-activity rows and dangling claims. Never touches the catalog or FTS. |
| `index <id-fact> <text> --force-dev` | Developer-only direct write that bypasses provider parsing. Requires `--force-dev`; refuses without it. Do not point this at a real store — what it writes has no source and no provenance. Not listed in `--help`. |

None of these subcommands takes additional arguments; extra tokens are usage
errors.

To build an index, use `sync --discover`, not `index`.

### `status`

Entity count and active generation for the current store. No flags, no
positional arguments.

### `doctor [--db <path>]`

Environment self-check. Without `--db` it reports build facts only and says so
in a `hint` field, with `db: "not-checked"` — that is not a failure. With `--db`
it opens the store and additionally reports `schema`, `generation`,
`interrupted_batches`, `orphaned_tool_activities`, and
`orphaned_activity_memberships`.

`--db` may appear before or after the command name. Any extra positional
argument is a usage error, including an unknown `--`-prefixed token, so
`doctor --bogus` fails rather than silently succeeding.

### `providers`

Report the provider capability matrix: maturity facts, roadmap target, and
per-field capabilities. No flags. Deferred providers have
`maturity_target: null` and `ingestible: false`.

This build reports 16 rows: 14 ingestible, plus `deepseek-harness` and `zcode`
as deferred/unsupported.

### `config paths`

Report the platform `config`, `data`, `cache`, and `logs` paths. `paths` is the
only supported `config` subcommand; anything else is a usage error. Directories
that have never been used simply do not exist yet, which is normal.

### `model <subcommand>`

Local embedding-model cache management. Never opens a network connection.

| Subcommand | Notes |
| --- | --- |
| `model status` | Works in every build. Reports `feature`, `present`, `verified`, and a `detail` note. |
| `model import --dir <bundle>` | Verifies SHA-256 for every declared file, then atomically publishes under `<cache>/models/<model_id>/`. Requires a binary built with `--features semantic-candle`; otherwise it refuses with `capability_not_supported` rather than staging anything. |

`--dir` is required for `import` and may appear before or after the subcommand
name. It is not listed in `--help`.

### `mcp`

Start the stdio MCP server (JSON-RPC 2.0). Takes no positional arguments.

stdout belongs entirely to MCP framing, so `--output`, `--robot`, and
`--request-id` have no meaning here. The store must already exist: `mcp` is a
read path, so a missing `--db` fails with `not_found` / exit 4 before the server
starts.

See [MCP client setup](../guide/mcp-clients.md) for per-client configuration and
the tool list.

### `tui`

Interactive read-only browser (preview). Requires an interactive terminal;
without one it fails with `tui requires an interactive terminal`.

| Flag | Effect |
| --- | --- |
| `--snapshot-json <query>` | Headless structural projection of a search, printed as a single JSON line. Used by the release consistency harness to compare entry points. Accepts no other arguments. Not listed in `--help`. |

### `serve`

Start a loopback HTTP server with the embedded web UI. A fresh random bearer
token is generated per start; open the URL the terminal prints (it contains the
token).

| Flag | Value | Default |
| --- | --- | --- |
| `--port <n>` | `0`–`65535` | `0` (random free port) |
| `--lan` | — | Always refused in this release. |

This release is loopback-only (`127.0.0.1`). `serve --lan` is rejected with the
message `serve --lan: capability_not_supported; this release is loopback-only`.
Note the envelope code for that refusal is actually `invalid_request` / **exit
2**, not `capability_not_supported` / exit 7 — see
[Help-text divergences](#help-text-divergences).

## Exit codes

Exit 0 and 10 are outcomes; everything else comes from the canonical error code.
This table is the complete mapping and matches
[`schemas/robot/v1/error-catalog.json`](../../schemas/robot/v1/error-catalog.json)
row for row.

| Exit | Canonical code(s) | Retryable | Meaning |
| --- | --- | --- | --- |
| 0 | — | — | Success. |
| 2 | `invalid_request` | no | Argument or request validation failed. |
| 2 | `cursor_invalid` | no | Cursor token could not be parsed or verified. Discard it and re-run from page 1. |
| 2 | `cursor_expired` | no | Cursor is past its 15-minute TTL. Re-run the query for a fresh one. |
| 4 | `not_found` | no | The requested store or entity does not exist. |
| 5 | `source_io` | no | Source file I/O error. |
| 5 | `source_changed` | **yes** | The source was rewritten while being read. |
| 5 | `snapshot_failed` | no | Snapshot verification could not complete. |
| 6 | `catalog_error` | no | Catalog or search-index error. |
| 6 | `writer_busy` | **yes** | The data-root writer lease is held by another process. |
| 7 | `provider_error` | no | Provider format or adapter error. |
| 7 | `capability_not_supported` | no | The capability is not declared, or `--offline` refused it. |
| 9 | `schema_incompatible` | no | Store or protocol version does not match this binary. |
| 9 | `generation_mismatch` | no | The cursor's generation is not the active generation. |
| 10 | — | — | Partial success: budget truncation. Results are usable but incomplete. |
| 70 | `internal` | no | Unclassified internal error — a defect signal. |

In human mode an error prints as `error [<code>]: <message>` on stderr,
followed by a one-line next-step hint unless the message already contains an
executable command. In `json`/`jsonl` mode it is a single error envelope on
stdout with a stable `error.code` and an `error.retryable` boolean.

## Machine output

Use `--robot`, `--output json`, `--output jsonl`, or MCP for anything a script
reads. Human-mode wording is not a contract and can change at any time.

- Envelope shape: `schema_version` `1.1`, with `ok`, `outcome`, `command`,
  `data`, `page`, `meta`, `warnings`, `redaction`, `retrieval_mode`, and
  `request_id`.
- Machine and cross-boundary output is redacted by default (ADR-0009). Human CLI
  output is not (ADR-0004).
- Cursors are stateless signed tokens. They survive a restart but die on
  generation change and after their TTL, and they are result-set bound: a
  `search` cursor works only for `search`, a `list` cursor only for `list`.

## Help-text divergences

Found while verifying this page against the binary. Each is a mismatch between
`--help`, the code, or observed behaviour, and should be treated as a release
blocker rather than smoothed over in docs.

| # | Divergence | Location |
| --- | --- | --- |
| 1 | `--no-color` is parsed but never read. No code path consumes it, so it is silently a no-op. It is also undocumented in `--help`. | `crates/agent-session-grep-cli/src/main.rs:362`, `:1456`, `:1692` |
| 2 | `search --help` omits `--include-sidechain`, which the dispatch arm really accepts (top-level `--help` does list it). | `crates/agent-session-grep-cli/src/main.rs:1978` vs help text at `:1230`-`:1239` |
| 3 | `serve --help` says `--lan` is `capability_not_supported`, but the refusal is constructed as a usage error, so the envelope code is `invalid_request` and the exit code is 2, not 7. | help text at `crates/agent-session-grep-cli/src/main.rs:1345` vs `:511`-`:515` |
| 4 | `search --help` and `hook --help` state `--provider claude\|claude-code\|codex`, but the binary accepts 15 values derived from the capability matrix. | help text at `crates/agent-session-grep-cli/src/main.rs:1233`, `:1278` vs `:2703`-`:2707` |
| 5 | `list --help` documents only `--cursor`; `--max-items` and `--max-bytes` are also accepted. | help text at `crates/agent-session-grep-cli/src/main.rs:1289` vs `:2561`-`:2563` |
| 6 | The `--db` value guard `is_known_flag_name` omits `--mode`, `--yes`, `--port`, `--lan`, `--enable`, `--from-file`, `--force-dev`, `--decay-days`, and `--dir`. `--db --mode ingest <file>` therefore creates a store file literally named `--mode`, which is exactly what the guard exists to prevent. | `crates/agent-session-grep-cli/src/main.rs:1449`-`:1482` |
| 7 | The `EXIT CODES` block in `--help` lists only 0 and 10 and then defers to an "error catalog" it never names or locates, so exit 6 and exit 9 have no discoverable explanation from the help surface. The complete table is above. | help text at `crates/agent-session-grep-cli/src/main.rs:1067`-`:1068` |
| 8 | `--version` prints `agent-session-grep-cli 0.1.0`: the crate name rather than the binary name (`agent-session-grep`), and 0.1.0 while the release under preparation is 0.2.0. The same values appear in `doctor` (`tool`, `version`) and in the MCP `serverInfo`. | `crates/agent-session-grep-cli/src/main.rs:403` |
| 9 | Machine output redacts numeric budget fields. `is_secret_key` matches any key containing `token`, so the handoff pack's `budget.max_tokens` and `budget.used_tokens` come back as `"[redacted]"` in `--robot` output and over MCP. A caller cannot read back the budget it just set. | `crates/agent-session-grep-cli/src/redaction.rs:105` |
