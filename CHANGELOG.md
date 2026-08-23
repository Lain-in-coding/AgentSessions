# Changelog

All notable changes to agent-session-grep are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The repository is still private and no GitHub tag or Release has been created
for this worktree. The `0.1.0` version in `Cargo.toml` is the current package
version, not evidence of a published release. This section records the current
candidate changes; it will become a dated version section only after the owner
creates the corresponding tag and Release.

### Added

- `search --role <role>` and `search --exclude <term>` — the two structured
  search dimensions the query language deliberately does not carry. `--role`
  accepts `user`, `assistant`, `system`, `developer`, or `tool`; both flags are
  repeatable and values within a dimension are OR-ed. An explicit `--role` list
  supersedes the `--include-system` default (so `--role system` needs no second
  flag) and narrows the response to messages, because a session carries no role.
  `--exclude` is a whole-term filter tokenized exactly like the query, not a
  query operator — the query language stays operator-free (ADR-0003). Both
  predicates are pushed into SQL before `LIMIT`, and both are bound into the
  continuation token, so a cursor issued under one filter set is rejected rather
  than replayed under another. Reachable from every entry point: `roles` and
  `exclude_terms` on the existing `search_sessions` MCP tool (still nine tools),
  repeatable `role`/`exclude` query parameters on `/api/search`, and a role
  selector plus an exclude field in the embedded web UI.
- 16-provider capability matrix (`agent-session-grep-ports`) with deferred
  provider rows (`deepseek-harness`, `zcode`) and per-provider maturity grading.
- Provider adapters for the 14 implemented, Experimental providers:
  `claude-code`, `codex`, `grok-build`, `antigravity`, `opencode`, `pi`,
  `hermes`, `cursor`, `kimi-code`, `openclaw`, `qoder`, `tencent-codebuddy`,
  `cline`, and `aider`. Each adapter has evidence-backed synthetic fixtures;
  DeepSeek Harness and ZCode remain deferred because no transcript evidence is
  available.
- Source installers install both `agent-session-grep` and `asg`: Windows uses
  two executable copies; Unix uses a managed symlink or wrapper. Upgrade and
  uninstall are idempotent and refuse unrelated aliases.
- `handoff <query>` CLI subcommand — deterministic handoff-pack/v1 generation
  with evidence/inference separation and budget truncation.
- `--output markdown` — the handoff pack's Markdown projection, which the pack
  schema had always declared ("JSON is authoritative; the Markdown form is a
  deterministic projection of this structure"). Byte-reproducible for a given
  generation/query/budget. Defined only for `handoff`; every other command
  refuses the mode instead of falling back, and it cannot be combined with
  `--robot`.
- `--out <path>` — write a command's result payload to a file instead of stdout.
  Never overwrites an existing path (which also means it never follows a symlink
  at the target); a failed run leaves no file. Refused for `mcp`, `tui`, `serve`.
- `resume <session-id>` CLI subcommand — dry-run by default (prints the
  provider command, original working directory, and permission mode);
  `--yes` spawns the provider in that directory. Providers whose resume
  command is unverified report `available:false` rather than a fabricated
  command.
- `search --mode lexical|semantic|hybrid` — retrieval mode selection. The
  default vector mode uses bigram hashes for fuzzy lexical matching, not a
  semantic model; hybrid combines lexical and vector rankings with RRF.
  Semantic and hybrid metrics remain informational and carry no release
  threshold or quality claim. A real local semantic backend is available
  through the optional `semantic-candle` cargo feature (off by default).
- `hook <session-start|user-prompt-submit>` CLI subcommand — Claude Code hook
  integration, disabled by default. Reads the hook payload from stdin and emits
  the `hookSpecificOutput.additionalContext` contract; nothing is injected
  unless `--enable` is passed. Injected text is redacted (ADR-0009).
- `serve --port <n>` CLI subcommand — loopback HTTP server (random bearer
  token, Host loopback check, embedded Web UI, JSON API).
- MCP tools: `search_sessions`, `get_session_context`, `get_session_resume`, `get_message`, `list_sessions`, `generate_handoff`, `list_providers`, `get_status`, and `doctor` (9 total).
- Cross-boundary output redaction (ADR-0009): Robot JSON/JSONL, MCP, HTTP API,
  Handoff Pack, and Web UI redact standalone and prose-embedded secrets
  (AWS keys, GitHub PATs, OpenAI/Anthropic/xAI keys, Bearer tokens, PEM
  private keys, secret-named JSON fields). Human CLI/TUI output stays
  unredacted (ADR-0004).
- Bounded ingestion (RFC-0002 §7): `ingest`/`sync` read every source through
  `ReadOnlySource`/`BoundedLineReader` with per-format caps (JSONL 8 MiB per
  record, JSON-family 32 MiB, SQLite 128 MiB); an oversized source or record
  fails closed (`SourceTooLarge`/`RecordTooLarge`) instead of loading the whole
  file into memory.
- Global `--offline` flag: fails closed on any future network-dependent
  capability (`capability_not_supported`, exit 7); `doctor`/`hook` report the
  flag. The default build carries no HTTP client dependency and the only socket
  is `serve`'s loopback listener, enforced by `tests/network_egress.rs` and a
  `security-audit` CI step.
- `hook` gains `--provider` (repeatable) and `--decay-days` filters, wired into
  the same `SearchFilters` the CLI and MCP use.
- Session metadata search (schema v11 `session_fts`): search by resolved
  provider session id, pair-observed original working directory, or the first
  user request in a session, without indexing absolute source paths.
- `serve` hardening: bounded worker pool, request size limits, Host/Origin
  loopback checks, forwarded-header rejection, a POST CSRF gate, and an
  `/api/projection/search` alias shared with the consistency harness.
- `tui --snapshot-json <query>`: headless structural search projection over the
  same Application path, so the release harness needs no terminal automation.
- 12 additional provider golden fixtures (grok, antigravity, opencode, pi,
  hermes, cursor, kimi, openclaw, qoder, codebuddy, cline, aider): synthetic
  transcript plus `PROVENANCE.md`, pinned canonical output, span round-trip,
  and a read-only checksum regression, alongside the existing Claude/Codex
  golden evidence.
- Resume execution contract: the first run forces a preview acknowledgement
  before any real spawn, the provider binary is preflighted, and a drift test
  keeps the capability matrix and the resume command builder aligned.
- `scripts/verify-release.py` expanded to 10 checks (binary/version, sync,
  lexical search, get, context, resume metadata and dry-run, deterministic
  handoff, semantic/hybrid effective modes, hook default-off, provider matrix).
- `scripts/rehearsal/compare_entrypoints.py`: the five-entry-point consistency
  harness now directly compares all five surfaces (CLI, MCP, Robot, Web, TUI)
  for the canonical search operation — Web launches a real loopback `serve`
  process and TUI drives `--snapshot-json`. An unimplemented entry point fails
  the harness instead of being recorded as a skip.
- Optional local semantic backend (`semantic-candle` cargo feature, default
  off): Candle 0.10 + pinned
  `intfloat-multilingual-e5-small@614241f6-candle-f32-meanpool-l2-qpass-v1`
  (384 dims, mean pooling, L2, `query:`/`passage:` prefixes). `model import
  --dir <bundle>` verifies every declared SHA-256 and atomically publishes the
  bundle into the local model cache (never downloads); `model status` reports
  whether the default E5 bundle is imported. Default builds stay
  bigram-hash / lexical-only, and missing weights keep the explicit
  `lexical_fallback` behavior. See `docs/operations/SEMANTIC-MODEL-BUNDLE.md`.
- Handoff packs project authoritative per-message facts: each mainline entry
  now carries `role` and `is_sidechain` (missing facts render `unknown` /
  `false`, never fabricated), and the pack carries the catalog `tool_activity`
  list for its hit messages (`handoff-pack/v1` schema updated).
- TUI search facet controls: `m` cycles the sidechain facet
  (include → main-only → subagent-only) and `k` cycles the tool-kind facet
  (any → file → command → web → query → unknown) with an empty input,
  reissuing the search through the same `SearchFacets` contract as CLI/MCP.
- Context responses (`context` CLI, `get_session_context` MCP) project
  `tool_activities` for the assembled messages via the ContextGraphStore batch
  read; empty when nothing is stored, never fabricated.
- Provider Beta readiness ledger (`docs/product/PROVIDER-BETA-READINESS.md`):
  per-provider local vs external Beta blockers, so no provider is promoted
  from code existence alone.
- `stats` — what the history is made of, by provider, month, project and session
  size. Separate from `status`, which answers whether the index is healthy.
  Values that cannot be derived without guessing become an explicit unknown
  bucket rather than being dropped, so each dimension's buckets sum to the total.
- `forget` and `prune` — remove sessions, a project directory, a date range or a
  provider from the index. Dry-run by default; only `--yes` deletes. Source
  transcripts are never touched, and a forgotten source is suppressed so the next
  `sync --discover` does not resurrect it (`--readmit` undoes that).
- `sync <path>` accepts a directory and expands it recursively, with candidacy
  answered by each provider's probe rather than by file extension. Previously it
  refused a directory and asked the user to expand the file list in their shell —
  the same traversal the tool already performs for `--discover`.
- `doctor` is a guided diagnosis: each check reports a fact, and each failing one
  names an executable next step (provider roots with file counts, store location
  and writability, whether either binary name resolves on PATH, whether a
  semantic backend is compiled in).
- Help text and suggested commands echo the name the binary was actually invoked
  as, so a reader who typed `asg` is not told to run `agent-session-grep`.
- `scripts/evidence/growing_source_repro.py` — reproducible harness for the
  growing-transcript race, running several trials because a single trial passed
  against a known-broken build about half the time.

### Fixed

- Syncing a transcript an agent is actively writing now reports
  `source_changed` (exit 5, retryable) instead of `provider_error` (exit 7, not
  retryable). The published error catalog and the runbook always said the former;
  the classification was lost twice on the way up, and the enum variant meant for
  it existed but was never constructed. Callers were told not to retry a race
  that retrying wins.
- A source containing two same-named tool calls with no paired output no longer
  fails the entire `sync`. Their derived activity ids collide, and the batch was
  rejected outright, so one affected transcript stopped every other source in the
  same run from indexing — measured as 0 of 151 real Codex rollouts indexed.
- `pi` transcripts are reachable: its discovery root is registered, and the
  pi/openclaw probes sample past the leading metadata records that previously
  left both at equal confidence, which refused every real pi file as ambiguous.
- `doctor` resolves the store the same way every other read command does
  (`--db` > `$ASG_DB` > platform default). It previously honoured only `--db`, so
  the one command whose job is checking your store reported `not-checked` while
  `status` and `search` read that same store. It still never creates a store.
- Port invariant violations are classified `internal` rather than
  `catalog_error`: they are defect signals, unrelated to the database, and the
  old classification sent the operator to check their `--db` path.
- serve query-string routing: `/api/search?q=...` no longer 404s.
- Smoke scripts assert the actual 9 MCP tools (was 7 after the provider wave;
  `generate_handoff` brought it to 9).
- `verify-release.py` runs with `--db` and a committed gate fixture.
- Redaction now covers secrets embedded inside prose (previously only
  whole-string secrets were matched).
- Search snippets are centred on the match rather than truncated from the start,
  so a hit late in a long message is visible without opening it.
- Handoff packs no longer leak an absolute path through
  `tool_activity[].target`; cross-boundary output carries the final path
  component only (ADR-0009).
- Three tests went red at random under parallel load, which trains the habit of
  rerunning until green and would bury a real regression. Two asserted absolute
  timings or real machine state; the third failed on the server's own request
  timeout under loopback congestion.

### Release scope

- CLI, Robot JSON protocol, MCP server, TUI, and Web UI adapters sharing one
  Application ADT. The cross-entry consistency harness reports `consistent`
  across all five; the three-platform clean-environment rehearsal is not
  complete (macOS was not run).
- Session search over the implemented provider set (lexical FTS5 plus ranked
  fuzzy-lexical retrieval), resume metadata extraction, and handoff packs.
- Installer scripts for Windows (PowerShell) and Unix (Bash), release
  rehearsal automation, and an evidence-backed open-source gate manifest.
- Core Beta evidence harness: lexical recall at 10 = 1.00 and parse loss =
  0.00 on the committed synthetic gate fixture.

### Known limitations

Stated here rather than discovered after install.

- Four of the sixteen providers have been verified against real transcripts on a
  developer machine (`claude-code`, `codex`, `antigravity`, `pi`); the rest carry
  synthetic fixtures only. Every provider is graded Experimental for that reason,
  not as a formality.
- Two providers (`grok-build`, `cursor`) cannot be verified without paid
  third-party access, and two more (`deepseek-harness`, `zcode`) are deferred
  with no adapter. `providers` reports each provider's grade rather than implying
  uniform support.
- Performance thresholds are not all met on the same corpus: a gated synthetic
  corpus fails write throughput, while real-density input fails the search and
  MCP p95 latency targets. The numbers and the measurement that refuted three
  earlier hypotheses are recorded in the plan, not smoothed over.
- `antigravity` writes a second transcript file per session
  (`transcript_full.jsonl`) whose record types differ entirely. It is currently
  half-accepted by the probe and loses a line; whether to support it as a variant
  or exclude it is undecided.
- Two distinct tool calls that normalise identically are stored as one activity.
  Making them distinct would change every derived activity id, which requires a
  new id namespace rather than a silent change.
- The MCP surface is read-only with no refresh tool, so an MCP client's view goes
  stale until `sync` is run elsewhere.
- macOS is untested: the three-platform clean-environment rehearsal covered
  Windows and Linux only.
