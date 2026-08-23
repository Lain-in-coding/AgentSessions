# Quickstart

Three commands take you from a fresh install to searching your own AI
coding-agent history: install, `sync --discover`, `search`.

`agent-session-grep` never writes to your provider transcripts. It reads them,
copies what it parses into its own SQLite store, and leaves the originals
untouched.

## 1. Install

There are no prebuilt binaries yet, so the installers build from a repository
checkout. You need a Rust toolchain (<https://rustup.rs>) on `PATH`.

Windows (PowerShell 7+):

```powershell
pwsh -File scripts/install/install.ps1
$env:PATH = "$env:LOCALAPPDATA\agent-session-grep\bin;$env:PATH"
```

Linux or macOS:

```bash
bash scripts/install/install.sh
export PATH="${XDG_BIN_HOME:-$HOME/.local/bin}:$PATH"
```

The installers place two executables in that directory: `agent-session-grep`
and the short alias `asg`. They print a PATH hint but do not edit `PATH`,
the registry, or your shell profile — the `export`/`$env:PATH` line above
covers the current shell only. See
[Install and upgrade](../operations/INSTALL-AND-UPGRADE.md) for custom
prefixes and permanent `PATH` setup.

Confirm the install:

```
$ asg --version
agent-session-grep-cli 0.1.0
```

The rest of this guide uses the short name `asg`. Every example works
identically with `agent-session-grep`.

## 2. Build the index

```
$ asg sync --discover
```

`sync --discover` walks the on-disk data root of every provider that has one
and hands each candidate file to the provider adapters for identification.
Files are claimed by content, not by extension, so SQLite, JSON, and Markdown
transcripts are discovered alongside JSONL.

Real output from a machine that has Claude Code installed and nothing else
(abridged — the per-provider report lists all 14 implemented adapters):

```
sources: 1（本次扫描的源文件数）
emitted: 3（本次新解析的消息条数）
committed: 3（本次实际入库的消息条数）
unchanged: 0（未变化的已有消息条数，不是文件数）
skipped: 0（因格式无法入库的消息条数）
generation: 1（当前入库代次）
discovery: 扫描了 14 个 provider（扫描不完整——本轮不推导 tombstone）
  claude-code: 1 source(s) found
  codex: data root not present — nothing to scan
  aider: no discovery root — pass transcripts explicitly: asg sync <file>...
  cursor: no discovery root — pass transcripts explicitly: asg sync <file>...
```

Read the per-provider report carefully, because two zero results mean
different things:

- **`data root not present — nothing to scan`** — this provider has a known
  data root, but that directory does not exist on this machine. Normally this
  just means the provider is not installed.
- **`no discovery root — pass transcripts explicitly`** — this provider has no
  registered data root at all, so `--discover` will *never* find it. It is
  still fully searchable, but you must name the files yourself (step 4).
  Which providers are in which group is listed in
  [Providers](providers.md).

`sync` is incremental and atomic. Re-running it on unchanged sources commits
nothing and does not advance `generation`; the counter increments only when a
batch actually lands.

### Where the index is stored

You do not have to choose a path. With no `--db` flag and no `$ASG_DB`,
the store defaults to `asg.db` inside the platform data directory:

| Platform | Default store |
|---|---|
| Windows | `%LOCALAPPDATA%\AgentSessions\data\asg.db` |
| macOS | `~/Library/Application Support/AgentSessions/data/asg.db` |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/agentsessions/asg.db` |

`asg config paths` prints the resolved directories for your platform:

```
$ asg config paths
cache: <local-app-data>\AgentSessions\cache
config: <roaming-app-data>\AgentSessions\config.toml
data: <local-app-data>\AgentSessions\data
logs: <local-app-data>\AgentSessions\logs
说明：以上是默认位置，未创建过的目录表示尚未使用，属正常。
```

Directories that do not exist yet are normal — they are created on first use.
To keep several independent indexes, override the store per invocation with
the global `--db <path>` flag (it goes *before* the subcommand) or set
`$ASG_DB`. Precedence is `--db` > `$ASG_DB` > platform default. Read commands
never create a store; write commands do.

## 3. Search, then read

```
$ asg search "migration"
日期       | Provider    | 会话标题     | 工作目录           | Session ID
2026-08-01 | claude-code | And what ab… | /…/demo-project    | ses_v1_0a80b66abbc6156bd38ea6b99e84a5b1

Next:
  agent-session-grep get-message msg_v1_11111111-0000-4000-8000-000000000003 --session ses_v1_0a80b66abbc6156bd38ea6b99e84a5b1 --around 2
  agent-session-grep context ses_v1_0a80b66abbc6156bd38ea6b99e84a5b1
```

Search matches individual *messages*, ranked by relevance, but the human table
shows **one row per session** — the title column is the highest-scoring hit in
that session. Two matching messages in one session therefore produce one row.
`--output json` returns every message hit separately if you need them all.

Every result prints the next commands to run with the real IDs already filled
in, so you can copy them straight from the terminal.

The data flow is three steps: `search` finds a message, `show` prints that one
message in full, `context` expands the session around it.

```
$ asg show msg_v1_11111111-0000-4000-8000-000000000003
role: user
timestamp: 2026-08-01T09:01:00.000Z
session: ses_v1_0a80b66abbc6156bd38ea6b99e84a5b1
text: And what about the migration rollback plan?
用 context <session> 展开这个会话的完整上下文。
```

```
$ asg context ses_v1_0a80b66abbc6156bd38ea6b99e84a5b1
session ses_v1_0a80b66abbc6156bd38ea6b99e84a5b1  branch leaf msg_v1_11111111-0000-4000-8000-000000000003  (generation 1)
  1. [user] How do I back up the sqlite database before a migration?
  2. [assistant] Copy the database file while no writer holds the lock, then verify the byte coun…(已截断,用 show msg_v1_11111111-0000-4000-8000-000000000002 看全文)
  3. [user] And what about the migration rollback plan?
evidence: 3 span(s)
```

IDs are content-addressed and prefixed by kind: `msg_v1_` for a message,
`ses_v1_` for a session, `doc_v1_` for a source document. They survive file
moves, renames, and incremental appends, so an ID you noted last week still
resolves after today's `sync`.

Retrieval is lexical (SQLite FTS5, with CJK bigram support) unless you ask for
something else. `--mode semantic` and `--mode hybrid` require a vector index
built by `asg index embeddings` first; if that index is missing, results are
labelled `retrieval_mode=lexical_fallback` and carry a warning rather than
silently degrading.

### Narrowing a search

All of these go *after* the `search` subcommand:

| Flag | Effect |
|---|---|
| `--provider <id>` | Restrict to a provider. Repeatable; multiple values are OR-ed. |
| `--role <role>` | Only `user`, `assistant`, `system`, `developer`, or `tool` messages. Repeatable; multiple values are OR-ed. |
| `--exclude <term>` | Drop hits containing the term. Repeatable; a hit matching any term is dropped. |
| `--since <t>` / `--until <t>` | Half-open time window `[since, until)`. Absolute RFC3339, or relative `1h` / `1d` / `1w`. |
| `--group-by-session` | Collapse to one best hit per session, with an `occurrences` count. |
| `--main-only` | Main-line messages only, excluding subagent sidechains. |
| `--subagent-only` | Subagent (sidechain) messages only. Mutually exclusive with `--main-only`. |
| `--tool-kind <kind>` | Only messages that invoked a `file`, `command`, `web`, `query`, or `unknown` tool. |
| `--tool-name <name>` | Only messages that used exactly this tool name. |
| `--include-system` | Include `system`/`developer` messages, which are excluded by default. |
| `--max-items <n>` / `--max-bytes <n>` / `--cursor <token>` | Page size, byte budget, and the continuation token from a previous page. |

Nothing is filtered unless you ask. Two useful combinations:

```bash
# What did the assistant actually say about the retry logic?
agent-session-grep search "retry" --role assistant

# Same question, minus the noisy thread you already read.
agent-session-grep search "retry" --role assistant --exclude backoff
```

`--role system` needs no `--include-system`: naming a role explicitly overrides
the default that hides system noise.

Exit code `10` means partial success: a budget truncated the response, so the
results are usable but incomplete.

## 4. Providers `--discover` cannot find

Several adapters have no registered data root — `--discover` skips them by
design. Point `sync` at the files instead:

```bash
# One or more explicit paths
asg sync path/to/transcript.jsonl path/to/another.json

# Long lists: one path per line; blank lines and `#` comments are ignored
asg sync --from-file sources.txt

# Disambiguate formats that are byte-shaped alike (pi and openclaw genuinely are)
asg sync --provider pi path/to/session.jsonl
```

Use `--from-file` once you have more than a few hundred paths; a shell glob
that expands to thousands of arguments will hit the command-line length limit.
A `sync` path may also be a directory: it is expanded recursively and each
candidate is accepted only when a provider probe claims it.

Two constraints worth knowing: a transcript file should contain exactly one
session (if several `sessionId`s are found, everything is attributed to the
first one and a warning is reported), and files no adapter claims are skipped
and counted rather than failing the run.

## 5. Check your setup

```
$ asg doctor --db <path>
db: ok
generation: 1
interrupted_batches: 0
offline: false
orphaned_activity_memberships: 0
orphaned_tool_activities: 0
schema: 13
semantic_feature: null
tool: agent-session-grep-cli
tool_activity_storage: true
version: 0.1.0
```

`doctor` resolves the store using `--db <path>` > `$ASG_DB` > the platform
default. If the resolved store exists, it validates that store and reports its
schema; if no store exists yet, it reports `db: not-checked` / `schema: null`
and gives the `sync --discover` command that will create it. It never creates a
store during a read-only check.

## Where to go next

- [Providers](providers.md) — where each provider keeps its transcripts, and
  what its adapter cannot parse.
- [serve, tui, and hook](serve-tui-hook.md) — the browser UI, the interactive
  terminal browser, and Claude Code hook integration.
- `asg --help` for the full command list, and `asg <command> --help` for one
  command's flags.

## Machine-readable output

Every command speaks a protocol envelope as well as human text. `--output
json` (or `--robot`, which also disables colour and progress) makes `stdout`
carry nothing but the envelope, which is what the MCP server, the Web UI, and
CI scripts consume:

```bash
asg --robot providers
asg --robot --request-id ci-1234 search "flaky test"
```

`--output jsonl` streams one JSON object per line instead. `--request-id <id>`
is echoed verbatim in every frame so a caller can correlate responses.
