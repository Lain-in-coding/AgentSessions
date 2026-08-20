# Troubleshooting

Indexed by the exact text the tool prints. Find the line you actually saw, in
the left column, and read across.

Every message below was produced by running a release build. Nothing here is a
plausible-sounding paraphrase: if you cannot find your message on this page,
that is a documentation gap worth reporting rather than a message you should
translate yourself.

- Full flag and exit-code reference: [CLI reference](../reference/cli.md)
- Storage-level recovery procedures (rebuild, migration, lease contention):
  [rebuild and migration runbook](../operations/rebuild-and-migration-runbook.md)
- MCP client setup: [MCP client setup](mcp-clients.md)

## How to read an error

Human mode prints two lines to stderr:

```
error [<canonical_code>]: <message>
Next step: <generic next step for that code>
```

The `Next step:` line is a generic per-code fallback and is **omitted** when the
message already contains a runnable `asg` or `agent-session-grep` command — in
that case the message itself is the next step. For `invalid_request`, by far the
most common code, the fallback is always the same line:

```
Next step: Check the command and flag spelling; run --help for full usage
```

Machine mode (`--robot`, `--output json`, `--output jsonl`) prints a single
error envelope on stdout with a stable `error.code` and `error.retryable`. Script
against those two fields and the process exit code, never against the human
wording.

## Nothing is wrong — these are exit 0

| What you saw | What it means | What to do |
| --- | --- | --- |
| `no hits` followed by `Hint: try a shorter query, or fewer words (a single word often works).` | The query ran; the index simply contains no match. Exit 0. | Try a shorter query or a single word. Search is plain-text and lexical by default — it does not stem, expand synonyms, or infer meaning. |
| `the catalog is empty (nothing has been indexed yet)` followed by `Next: asg …sync --discover` | The store exists but has never been populated, so *every* query would return nothing. Exit 0. | Run the command on the `Next:` line verbatim. |
| The same two lines from `list` | Same condition, seen from `list`. `list` prints the identical pair, not a shorter variant. Exit 0. | Same. |
| `The catalog is empty — run: asg …sync --discover` (from `status`, after the `entities: 0` / `generation: N` lines) | Same condition, seen from `status`. Exit 0. | Same. |
| `db: not-checked` with `hint: no --db given: the checks above cover the environment only. Run doctor --db <path> to verify the store and its schema.` (from `doctor`) | `doctor` was run without `--db`, so it only checked build facts. This is **not** a failed self-check. `schema` is reported as `null` in this mode. | Run `agent-session-grep doctor --db <path>` to actually validate a store and its schema. |
| `available: false` with an `unavailable_reason` (from `resume`) | This session's provider has no verified resume command. Exit 0 — history stays searchable even when it cannot be resumed. | Use `get-session-resume` to read `provider_session_id`, then run the provider's own resume flow yourself. The tool will not invent a command. |
| `retrieval_mode: lexical_fallback` plus a warning | You asked for `--mode semantic` or `--mode hybrid` but the vector index is not built, so the query ran lexically. It is reported rather than silently substituted. | Run `agent-session-grep --db <path> index embeddings`, then retry. |

## Exit 2 — `invalid_request` and the cursor codes

### Store and argument shape

| What you saw | Cause | Fix |
| --- | --- | --- |
| `--db requires a path` | `--db` was last on the line with no value after it. | Supply the path. |
| `--db requires a path (got flag --robot); --db <path> must precede other flags` | The token after `--db` is a known flag name, so it would have been used as a filename. | Put `--db <path>` first, before other global flags. **Note:** this guard does not cover every flag name — `--db --mode`, `--db --yes`, `--db --port`, and several others are *not* caught, and on a write command they really do create a store file named after the flag. Always give `--db` a real path. |
| `--db requires a non-empty path (an empty path silently uses SQLite's private temporary database)` | `--db ""`. | Supply a real path. |
| `duplicate --db flag` | `--db` given twice. There is no last-wins. | Give it once. |
| `--request-id must match ^[A-Za-z0-9._:-]+$ (1..=128 chars), got "…"` | Invalid correlation id. It is rejected rather than silently replaced, because a substituted id would look like successful correlation. | Use only `A-Za-z0-9._:-`, 1 to 128 characters. |
| `unsupported output mode: xml` | `--output` got a value outside `human\|json\|jsonl`. | Use one of the three. |
| `duplicate output flags: --output cannot be repeated or combined with --robot` | Both were given, or `--output` was repeated. | Pick one; `--robot` is exactly `--output json`. Note this message arrives as a **JSON error envelope on stdout**, not as a human `error [...]` line, because `--robot` has already taken effect by the time the conflict is detected. |
| `missing subcommand.` then `Available commands: …` then `Run \`--help\` for full usage.` | No command name on the line. | Pick a command from the printed list. |
| `unknown subcommand: handof` then `Did you mean \`handoff\`?` then the same `Available commands:` list | Typo. The suggestion comes from edit distance and is omitted when nothing is close enough — no guessing. | Use the suggested name, or pick from the printed list. |
| `doctor takes no positional arguments` / `mcp takes no positional arguments` / `serve takes no positional arguments` / `tui takes no positional arguments` / `providers takes no positional arguments` | An extra token, including an unknown `--`-prefixed one. Unknown flags are counted as stray positionals on purpose, so a typo cannot be silently discarded and reported as success. | Remove the extra token. |
| `config paths is the only supported config command` / `config paths takes no additional arguments` | Any other `config` subcommand. | Use `config paths`. |

### Identifiers

| What you saw | Cause | Fix |
| --- | --- | --- |
| `not a valid session id: <value>` | The value is not a well-formed `ses_v1_…` wire id. Emitted by `context`, `resume`, and `get-message --session`. | Copy the `session_id` from a `search` hit or from `show` output. Do not use a provider-native session id here — those are separate, and are only available through `get-session-resume`. |
| `not a valid entity id: <value>` | The value is not a well-formed wire id at all. Emitted by `get`, `show`, and `get-session-resume`. | Use an id from `search` or `list` output: `msg_v1_…`, `ses_v1_…`, or `doc_v1_…`. |
| `not a valid message id: <value>` | The value is not a well-formed `msg_v1_…` id. Emitted by `get-message`. | Use the `id` field from a `search` hit. |
| `session_id is not a valid session id: <value>` (over MCP, as JSON-RPC `-32602`) | Same condition, from the MCP surface. | Same. |

### Provider selection

| What you saw | Cause | Fix |
| --- | --- | --- |
| `unknown provider: <value> (expected claude\|aider\|antigravity\|claude-code\|cline\|codex\|cursor\|grok-build\|hermes\|kimi-code\|openclaw\|opencode\|pi\|qoder\|tencent-codebuddy)` | An unrecognized `search --provider` / `hook --provider` value. The accepted list is derived from the capability matrix, so it is always current. | Copy a value from the printed list. `claude` is an alias for `claude-code`. Note that `search --help` still advertises only `claude\|claude-code\|codex` — the error message is the accurate list. |
| `sync --provider: unknown provider <value>; expected one of aider\|antigravity\|claude-code\|cline\|codex\|cursor\|grok-build\|hermes\|kimi-code\|openclaw\|opencode\|pi\|qoder\|tencent-codebuddy` | An unrecognized `sync --provider` value. This list is adapter ids only — no `claude` alias. | Use `claude-code`, not `claude`. |
| `invalid request: no provider recognized this source[: <detail>]. Supported transcript formats are listed by \`agent-session-grep providers\`; if this file is not an agent transcript, leave it out` | No adapter claimed the file. When a `<detail>` is present it comes from an adapter in the *same* format family as the file, so it usually names the offending line. | Run `agent-session-grep providers` to see supported formats. If the file is not an agent transcript, exclude it. If it should be recognized, the `<detail>` is the repair hint — a JSONL adapter will name the bad line numbers. **Note** the message body redundantly restates `invalid request:` after the `error [invalid_request]:` prefix, so the full line reads `error [invalid_request]: invalid request: no provider recognized…`. Cosmetic, but do not treat the doubling as a sign of a different error. |
| `ambiguous provider selection: N variants matched with equal confidence (<list>); this source's shape is not distinctive enough to attribute, so it is refused rather than guessed. Re-run with \`sync --provider <id> <file>\` to name the provider explicitly` | Several adapters matched equally well. Some providers genuinely share an on-disk shape (for example `pi` and `openclaw`), so no content probe can separate them. Refusal is deliberate. | Re-run with `sync --provider <id> <file>` naming the correct provider. If the message also says `<id> was named as the expected provider, but <id> is not among the tied candidates`, then the provider you named is not one of the tied ones — pick one from the listed variants. |

### Facets, budgets, and modes

| What you saw | Cause | Fix |
| --- | --- | --- |
| `--main-only and --subagent-only are mutually exclusive` | Both given. | Keep one. |
| `--include-sidechain conflicts with --main-only` / `--include-sidechain conflicts with --subagent-only` | `--include-sidechain` is the default, so combining it with a narrowing facet is contradictory. | Drop `--include-sidechain`. |
| `--mode must be lexical\|semantic\|hybrid, got <value>` | Bad `--mode`. | Use one of the three. |
| `--tool-kind must be one of file\|command\|web\|query\|unknown, got <value>` | Bad `--tool-kind`. The set is closed. | Use one of the five. `unknown` means tools outside the known set. |
| `--policy must be mainline\|full, got <value>` | Bad `context --policy`. | Use `mainline` or `full`. |
| `--level must be raw\|talks\|sessions, got <value>` | Bad `context --level`. | Use one of the three. |
| `response budget too small: max_response_bytes = <n> is below the floor 4096` | `--max-bytes` under 4096. | Use 4096 or more. |
| `--max-items must be a non-negative integer` / `--max-bytes …` / `--max-messages …` | Non-numeric value. | Supply an integer. |
| `--max-evidence requires a positive integer` / `--max-tokens requires a positive integer` / `--max-bytes requires a positive integer` (from `handoff`) | Non-numeric or zero value on a `handoff` budget flag. | Supply a positive integer. |
| `--max-tokens requires a non-negative integer` / `--decay-days requires a non-negative integer` (from `hook`) | Non-numeric value on a `hook` flag. | Supply a non-negative integer. |
| `--port requires an integer in 0-65535` (from `serve`) | Bad port — non-numeric, or numeric but out of range (`--port 99999` gives the same message). | Use 0 (random free port) through 65535. |
| `--since must be an RFC3339/ISO-8601 timestamp or a compact duration (1h\|1d\|1w), got "…"` (also for `--until`) | Unparseable time bound. | Use an absolute timestamp with offset such as `2026-08-01T00:00:00Z`, or a compact duration `1h` / `1d` / `1w`. Over MCP, compact durations are rejected — the protocol carries no clock reference, so only absolute timestamps are accepted there. |
| `--around must be a non-negative integer` | Bad `get-message --around`. | Supply 0 or more. |
| `list [limit]: limit must be an integer` | Non-numeric positional limit. | Supply an integer. |

### Cursors

All three cursor failures are recoverable the same way: throw the token away and
re-run the query from page one. Never retry the same token.

| What you saw | Code / exit | Cause |
| --- | --- | --- |
| `invalid cursor (<reason>); discard it and re-run the query without a cursor` | `cursor_invalid` / 2 | Corrupt token, bad digest, or reuse across a different query, sort, or result set. A `search` cursor works only for `search`; a `list` cursor only for `list`. |
| `cursor expired (<detail>); re-run the query without a cursor to start fresh` | `cursor_expired` / 2 | Past the 15-minute TTL. |
| `cursor generation <n> is no longer active (current <m>); re-run the query without a cursor to page over the latest data` | `generation_mismatch` / **9** | New data was ingested while you were paging. |
| `cursor contract major <n> is not supported (supported: <m>); re-run the query without a cursor` | `schema_incompatible` / **9** | The token was issued by an incompatible binary version. |

### Sync argument combinations

| What you saw | Fix |
| --- | --- |
| `sync --discover and --from-file are mutually exclusive: --discover finds paths under the provider data roots, --from-file reads them from a list` | Use one. |
| `sync --discover takes no paths and no extra flags; paths come from the provider data roots` | Drop the paths and the extra flags. The one flag `--discover` does accept is `--all`, which expands the folded per-provider report. |
| `sync --discover and --provider are mutually exclusive: discover infers ownership from each provider's own data root, so naming a single provider conflicts with it` | Drop one. To name a provider, pass paths explicitly: `sync --provider <id> <file>...`. |
| `sync --from-file <list> takes no extra paths: every path comes from the list file` | Put every path in the manifest. |
| `sync --from-file: <path> contains no paths (blank lines and # comments are ignored)` | The manifest is empty or entirely blank lines and comments. Add one path per line. |
| `index <id-fact> <text> is a development direct-write entry point: it bypasses provider parsing and puts content with no provenance into the authoritative catalog. Add --force-dev if you really mean it. To build an index, run: sync --discover` | You invoked the developer-only direct-write form of `index`. Do not use it on a real store — what it writes has no source and no provenance. Use `sync --discover`. |

### Entry-point availability

| What you saw | Code / exit | Notes |
| --- | --- | --- |
| `tui requires an interactive terminal` | `invalid_request` / 2 | The TUI needs a TTY. In CI or a pipe, use `agent-session-grep --db <path> tui --snapshot-json <query>` for a headless JSON projection, or use `--robot search`. |
| `serve --lan: capability_not_supported; this release is loopback-only` | `invalid_request` / **2** | The message names `capability_not_supported`, but the envelope code is `invalid_request` and the exit code is 2, not 7. Do not branch on 7 for this case. This release binds `127.0.0.1` only, by design. |
| `serve: bind failed: <error>` | `invalid_request` / 2 | The requested `--port` is taken or not permitted. Omit `--port` to get a random free port. |
| `model import requires --dir <bundle-directory>` | `invalid_request` / 2 | Supply the bundle directory. |
| `capability_not_supported` from `model import` | `capability_not_supported` / **7** | The binary was built without `--features semantic-candle`. It refuses rather than staging an unverified bundle. Use a `semantic-candle` build, or stay on `model status`. |
| `hook: payload is not valid JSON` | `invalid_request` / 2 | stdin was non-empty but not JSON. An entirely empty stdin is accepted and injects nothing. |
| `hook <event>: event must be session-start\|user-prompt-submit` | `invalid_request` / 2 | Use one of those two (the `SessionStart` / `UserPromptSubmit` spellings also work). |

## Exit 4 — `not_found`

| What you saw | Cause | Fix |
| --- | --- | --- |
| `no store at <path> (from --db).` / `(from $ASG_DB)` / `(default store path)`, then `Build one with: asg …sync --discover`, then `Reads never create a store, so a mistyped --db fails here instead of returning an empty result.` | The store file does not exist. Read commands deliberately never create one — otherwise a typo in `--db` would hand you a brand-new empty database and a clean zero-hit result, which is the easiest possible silent failure. | Check the path for typos, then run the `Build one with:` command verbatim. The parenthetical tells you where the path came from, which is how you find out whether `$ASG_DB` is set when you did not expect it. |
| `entity not found` | The id is well-formed but nothing in the catalog matches. The id is deliberately not echoed back. | Run `list` to browse real ids, or re-run the `search` that produced the id — the index may have been rebuilt since. |

This is the same code you get when `mcp` cannot start: the MCP server is a read
path, so the store must already exist before a client launches it.

## Exit 5 — source-side failures

| What you saw | Code | Retryable | Notes |
| --- | --- | --- | --- |
| `源文件无法读取` | `source_io` | no | Fixed wording. The underlying filesystem detail and the absolute transcript path are deliberately stripped, because that path is private data. Check that the path you passed exists and is readable. |
| `source snapshot changed: <detail>` | `source_changed` | **yes** | The transcript's length, mtime, or fingerprint moved between snapshot and commit, so staging was discarded rather than committing a torn read. |
| `snapshot_failed` | `snapshot_failed` | no | Snapshot verification could not complete at all, as distinct from a confirmed change. Inspect source metadata and filesystem health. |

A source that exceeds an adapter's tested ceiling is refused rather than risking
an out-of-memory failure. That refusal surfaces as the `<detail>` appended to
`no provider recognized this source` (exit 2), carrying the text
`source is too large for this adapter: <n> bytes exceeds supported limit <m>`.

`source_changed` almost always means a coding agent is actively writing the
transcript you are trying to index — the current session's own file, most
commonly. Two ways out:

1. Stop the running agent, then re-run `sync`.
2. Accept a partial run: re-run `sync` and let the still-open source be skipped,
   then sync again once the session ends. Nothing already committed is lost, and
   `sync` writes nothing when nothing changed.

Retrying immediately without doing either usually just reproduces the error.

## Exit 6 — store-side failures

| What you saw | Code | Retryable |
| --- | --- | --- |
| `writer lease held: writer lease is already held` | `writer_busy` | **yes** |
| `数据库内部错误` | `catalog_error` | no |
| `cannot create the store directory <path>: <error>` | `catalog_error` | no |

**`writer_busy`** means another process holds the exclusive `writer.lock` lease
in the store's parent directory. Only one writer per data root; readers are
unaffected and any number can run concurrently. The lease is an OS file handle,
not a PID timeout, so a crashed process releases it automatically — there is no
stale lock to clean up by hand.

- If you are running `sync` on a schedule, the schedule is overlapping itself.
  Serialize it.
- If an MCP client, `serve`, or a hook is running, those are readers and are not
  the cause.
- Wait for the other writer, or end the leftover `agent-session-grep` process,
  then retry. Because it is retryable, a script may back off and retry rather
  than fail the run.

**`catalog_error`** is deliberately opaque: the backend's raw detail can carry
filesystem internals, so the user-visible message is the fixed string
`数据库内部错误`. The generic next step is the useful one here:

```
agent-session-grep doctor --db <path>
```

Check the `--db` path first — a trailing slash or a directory where a file was
expected produces this. If `doctor` also fails, follow
[rebuild and migration runbook](../operations/rebuild-and-migration-runbook.md),
procedure 3 (store reconstruction by re-ingest).

## Exit 7 — provider and capability failures

| What you saw | Code | Notes |
| --- | --- | --- |
| A provider adapter's parse failure | `provider_error` | The file is not a readable transcript of the claimed format, or it is corrupt. Adapter diagnostics usually name the offending line. |
| `capability_not_supported` | `capability_not_supported` | The capability is not declared by that provider, or `--offline` refused an operation that would need the network. Note that `serve --lan` reports this phrase in its *message* but exits 2, not 7. |

## Exit 9 — version incompatibility

| What you saw | Code | Fix |
| --- | --- | --- |
| `catalog schema version <n> is newer than supported <m>; upgrade agent-session-grep or rebuild the data root` | `schema_incompatible` | The store was written by a newer binary. Reading it under the old schema could misread new data, so it is refused instead. Upgrade the binary, or rebuild the store from sources with the binary you have. |
| `<subject> contextual relations are unavailable; re-ingest required` | `schema_incompatible` | Data was ingested before this relation existed. Re-run `sync` over the same sources. |
| `<subject> contextual relations are incomplete; re-ingest required` | `schema_incompatible` | A source was only partially scanned. Re-run `sync` over that source. |
| `cursor generation … is no longer active …` / `cursor contract major … is not supported …` | `generation_mismatch` / `schema_incompatible` | Cursor problems, not store problems. See [Cursors](#cursors). |

Migration procedures are in
[rebuild and migration runbook](../operations/rebuild-and-migration-runbook.md);
version-specific steps are in
[migration v6 to v7](../operations/migration-v6-to-v7.md) and
[migration v5 to v6](../operations/migration-v5-to-v6.md).

## Exit 10 — partial success

Not an error. The result is real but incomplete because a budget was hit. The
envelope carries `outcome: "partial"` and a `truncation` object naming the
reason.

Raise the relevant budget and re-run: `--max-bytes` (floor 4096), `--max-items`,
`--max-messages`, or, for `handoff`, `--max-evidence` and `--max-tokens`. A
script that treats any nonzero exit as failure will mis-handle this: check for
10 explicitly.

## Exit 70 — `internal`

There is no fixed message here: the message is whatever invariant was violated,
which is a defect signal rather than a usage problem. The generic next-step line
is the constant part:

```
Next step: Internal error: record the full output and report it
```

Capture the full output, including the `--robot` envelope, and open an issue. The
envelope is redaction-applied by default, but read it before attaching it.

## `sync --discover` found nothing

Exit 0 with no new messages is the most common first-run surprise, and there are
three distinct causes. The per-provider `root_state` in the discover report tells
you which one you hit:

| `root_state` | Meaning | What to do |
| --- | --- | --- |
| `unsupported` | That provider has **no discovery root registered at all**. `--discover` structurally cannot find its sources. | Sync it by explicit path: `sync <file>...` or `sync --from-file <list>`. |
| `missing` | The root is registered but does not exist on disk — the provider is not installed, or has never been run. | Nothing to do. Discovery deliberately does not treat a missing root as "the sources were deleted", so nothing already indexed is tombstoned. |
| `home_unresolved` | The root is registered but neither `$HOME` nor `%USERPROFILE%` could be resolved. | Set one of those environment variables. |
| `scanned` | The root was walked. Check `found` and `complete`. | If `found: 0`, the directory holds no file any adapter claims. |

Only **7** of the 14 ingestible providers register a discovery root:

| Provider | Root |
| --- | --- |
| `claude-code` | `~/.claude/projects` |
| `codex` | `~/.codex/sessions` |
| `openclaw` | `~/.openclaw/agents` |
| `pi` | `~/.pi/agent/sessions` |
| `tencent-codebuddy` | `~/.codebuddy/projects` |
| `antigravity` | `~/.gemini/antigravity-cli/brain` |
| `opencode` | `~/.local/share/opencode` |

`aider`, `cline`, `cursor`, `grok-build`, `hermes`, `kimi-code`, and `qoder`
report `unsupported` and must be synced by path. If you were expecting one of
those to appear automatically, that is the reason.

Files that no adapter can attribute are skipped and counted, never guessed at,
so a nonzero skip count alongside `found > 0` is normal in a mixed directory.

## Searching finds nothing that should be there

Work through these in order, cheapest first.

1. **Is the store the one you think it is?** Run
   `agent-session-grep --db <path> status`. Zero entities means nothing is
   indexed. `$ASG_DB` may be pointing somewhere unexpected —
   `agent-session-grep config paths` prints the default location.
2. **Is the index fresh?** Nothing watches your transcripts. There is no
   `watch` command, and the MCP server and `serve` are strictly read-only.
   Whatever `sync` last committed is what you are searching. Re-run
   `sync --discover`.
3. **Are you excluding the messages you want?** `system` and `developer` role
   messages are excluded by default — add `--include-system`. `--main-only`
   drops all subagent messages; `--subagent-only` drops all mainline ones.
4. **Is a time filter cutting them off?** `--until` is exclusive, so the
   interval is `[since, until)`. A boundary timestamp is excluded.
5. **Is the query too specific?** Search is plain-text and lexical by default.
   It does not stem, expand synonyms, or reason about meaning. Try one word.
6. **Did you expect semantic search?** `--mode semantic` needs
   `index embeddings` first, and the default build's vectorizer is
   `bigram-hash` — fuzzy lexical similarity, not semantic. `index embeddings`
   says so in a warning. Real semantic retrieval requires a `semantic-candle`
   build with an imported model bundle; check with `model status`.

## Machine-surface quirks to know about

Two behaviours that will look like bugs in your own code, because they are
bugs in this build rather than in yours:

- **`handoff` budget fields come back redacted.** In `--robot` output and over
  MCP, `data.budget.max_tokens` and `data.budget.used_tokens` are the string
  `"[redacted]"`, because the redaction ruleset matches any key containing
  `token`. You cannot read back the token budget you just set. `max_bytes`,
  `used_bytes`, and `max_evidence` are unaffected.
- **The MCP `search_sessions` provider list is stale and unenforced.** Its
  declared schema says `enum: ["claude","claude-code","codex"]` with
  `maxItems: 2`, but the server actually accepts all 15 CLI values and any
  number of them. A truly unknown value is rejected with the message
  `providers must contain only claude|claude-code|codex`, which understates
  what is accepted. Trust the CLI's own error message for the real list.
