# Changelog

All notable changes to agent-session-grep are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

> Planned first public version: `0.1.0`. No tag or release has been published.

### Added

- `kimi-code` now indexes the user's own prompts. Kimi writes them as
  `turn.prompt` (and `turn.steer` for a mid-turn correction) with the text in a
  top-level `input` block array, not as `context.append_message`; the adapter
  parsed only the latter, so the highest-value text in a Kimi session — what the
  user actually asked — never entered the index, and the golden fixture carried
  no such record for a test to catch it. Both record types now emit as user
  messages in file order, with the record shape taken from the upstream ctx
  adapter's real-shape fixture. `context.append_loop_event` step/tool events
  stay unparsed on purpose: they are tool activity rather than messages, and
  wire.jsonl carries no per-message native id to anchor them to.

- Pi session-tree lineage is now reported instead of silently flattened. Pi
  format v2/v3 files are a parent-linked tree (`version` on the `session`
  header, per-record `id` + `parentId`; two records sharing one `parentId` are
  a retried/abandoned branch). The adapter still indexes **every** branch's
  text in file order — search completeness comes first — but the parse report
  now carries an explicit diagnostic naming the declared version and the number
  of lineage-bearing records, and `probe` reports both as matched evidence. No
  parent edges are emitted: canonical message identity adopts a provider native
  id verbatim and without a provider namespace, while real Pi record ids are
  8 hex characters scoped to one file, so promoting them would merge messages
  from different sessions onto one entity. `context` therefore stays
  Unsupported with the reason recorded per row — the blocker is
  document-scoped native message identity in the composition root, not a
  missing format fact. Pinned by a new synthetic branch fixture
  (`crates/agent-session-grep-provider-pi/tests/golden/v3-branched.jsonl`,
  BLAKE3-pinned, both branches indexed) plus positive/negative diagnostic
  guards in the golden and property suites.
- Repo filter parity across the machine surfaces: the MCP `search_sessions`
  tool takes `repo` (declared in its tool schema with the same length bound
  the derivation uses, enforced at runtime), the Web/loopback search routes
  forward a `repo` query parameter, and `hook <event>` takes `--repo` — all
  with the CLI `--repo` semantics (verbatim three-segment `host/owner/name`
  slug; blank values are `invalid_request`, never a silent "no filter").
  `generate_handoff` keeps no repo dimension on either surface.
- Repo identity (schema v16 `session_repo_slugs` projection): sessions are
  grouped by the `host/owner/name` slug derived from their pair-observed
  working directory via local git detection (`git rev-parse --show-toplevel`
  + `remote get-url origin`, cached per directory — Recall's repo identity
  and sessiongrep's `find_repo_root` patterns). Only the three-segment slug
  is stored — absolute paths never enter the projection (privacy contract).
  A stored row means detection succeeded; no row means unknown (honest
  degradation — directories that no longer exist, non-git directories, or
  remotes without `origin` are never guessed). The projection rebuilds in
  the same transaction as `session_fts` (affected-session commits and
  `index rebuild`), and is removed with its session. `search --repo <slug>`
  filters hits to sessions of that repo; `status` reports per-repo session
  counts (`repos` key, sessions desc + slug asc). Detection failures never
  fail sync or index.
- Token usage tracking (usage dimension, schema v15 `usage_events` +
  `usage_event_membership` projection): only numbers the provider format
  explicitly gives are recorded — never estimated from text length or any
  other proxy. `claude-code` extracts observed per-message `message.usage`
  (input/output/cache_read/cache_write, anchored to the assistant record's
  uuid); `codex` derives per-event increments from `event_msg/token_count`
  cumulative totals under monotonic validation (98% stale-regression guard,
  fork-child inherited baseline absorbed, per Recall's derivation rules).
  Session-level events (no message anchor) are stored with a NULL message id —
  anchors are never invented. A stored row means "the provider reported
  usage" (coverage marker: absence of rows means unknown, not zero).
- `status` reports usage totals (`usage.sessions` + five bucket sums +
  observed/derived event counts); zero-session usage is reported as "no
  usage facts", distinct from a missing projection. `doctor` reports
  `usage_storage: true` and orphaned usage projection counts, purged by
  `index purge-activities` in the same transaction.
- Per-provider `usage` capability column (capability matrix + robot
  `providers` envelope): `claude-code` Native, `codex` Derived; the other 12
  implemented providers honestly report Unsupported with per-format evidence
  comments (pi/kimi-code/tencent-codebuddy/cline/opencode/cursor/grok-build
  formats carry usage fields but their adapters lack per-message native ids
  or field parsing; aider/hermes/antigravity/openclaw/qoder formats carry no
  usage facts).

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
- `list_sessions` Peek Bundle (borrowed from hstry): every session entry
  carries a `peek` object with `first_user_text` / `last_user_text` (≤200
  chars each, char-boundary truncation; null without user messages), capped at
  1 KiB serialized per session with the cap and over-limit truncation guarded
  by tests. Peek bytes are charged to the `max_response_bytes` gate.
- `list_sessions` derived session titles (borrowing list #6): session entries
  carry a `title` string (≤80 chars, char-boundary truncation; omitted when no
  candidate exists) derived by the chain custom title (`title` field) > AI
  summary (`summary` field) > first valid user message. Injected noise is
  filtered at provider parse time, so the first committed user message is the
  first valid one. Titles live in a rebuildable schema v13 `session_titles`
  projection maintained in the same transaction as the session FTS rows, and
  title bytes are charged to the `max_response_bytes` gate like peek bytes.
  The human list renderer shows the title instead of the raw payload preview.
- Parser-semantic version in `sync` unchanged detection (borrowed from
  Recall's parser-version incremental sync): schema v14 adds
  `source_scans.parser_version` (migration default `0`), and the unchanged
  judgment now compares `(len_bytes, fingerprint, parser_version)` against
  the binary's `PARSER_SEMANTIC_VERSION` constant. When parsing semantics
  change (the constant is bumped), sources with unchanged bytes are
  re-parsed once on the next `sync` — targeted backfill, with the reparse
  reason reported through the sync warnings channel — instead of keeping
  stale parsed content until a manual `index rebuild` or a source-file
  change. No full rebuild is required.
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
- Seeded randomized property suites for six more provider adapters
  (`kimi-code`, `openclaw`, `qoder`, `tencent-codebuddy`, `cline`, `aider`),
  closing the coverage gap that previously left randomized property tests to
  Claude/Codex. Fixed-seed xorshift64* transcripts assert span/seq/count/
  metadata invariants against independent ground truth; deterministic
  golden-source mutations (truncate/insert/delete/byte-flip, split and
  shuffled lines) must never panic, never mutate the source bytes, never
  report a partial commit, and stay byte-identical across re-parses; and
  `probe` over arbitrary bytes must never panic and must report only the
  adapter's own variant with non-ambiguous confidence
  (`crates/agent-session-grep-provider-*/tests/properties.rs`).
- Resume execution contract: the first run forces a preview acknowledgement
  before any real spawn, the provider binary is preflighted, and a drift test
  keeps the capability matrix and the resume command builder aligned.
- `scripts/evidence/privacy_scan.py`: scans tracked text for personal or
  machine-specific absolute paths, alongside
  `docs/operations/PUBLIC-HISTORY-SCRUB.md` for the history decision.
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
- Seeded randomized property suites (`tests/properties.rs`) for `grok-build`,
  `antigravity`, `opencode`, `pi`, `hermes`, and `cursor`, mirroring the
  existing claude-code/codex coverage: deterministic xorshift64* corpora pin
  span round-trips, seq/committed self-consistency, parse determinism,
  read-only sources (RFC-0002 §7), probe never-panics on arbitrary bytes, and
  no-panic legal output under seeded mutations of each golden fixture
  (truncate / insert / delete / flip / split / shuffle). The readiness ledger
  gains a `property` column guarded in both directions against each suite's
  existence (`beta_readiness_property_column_matches_properties_test_existence`).
- Resume command matrix extended with authoritative upstream evidence:
  `antigravity` → `agy --conversation <id>` (fast-resume adapter +
  agent-sessions builder), `opencode` → `opencode <directory> --session <id>`
  with the directory positional omitted when unknown (fast-resume adapter;
  cc-switch/agf corroborate the `--session`/`-s` flag), `kimi-code` →
  `kimi --session <id>` (fast-resume adapter, same wire.jsonl source face),
  and `tencent-codebuddy` → `codebuddy --resume <id>` (AgentRecall, same
  `~/.codebuddy/projects` JSONL face). Capability matrix resume columns move
  to Derived for these four; `hermes` (conflicting `--resume`/`--session`/no
  CLI claims across reference projects), `qoder` (no resume evidence), and
  `cursor` (CLI `agent --resume` belongs to a different store.db surface)
  stay Unknown rather than fabricate commands.
- Honest `tool_activity` accounting for the seven providers whose formats
  were reviewed for structured tool-call records: `grok-build` (ACP
  `_meta.bashCommand` meta chunks), `antigravity` (`tool_calls`),
  `qoder` (`tool_use`/`tool_result` record types), and `kimi-code` (loop
  `step`/`tool` events) do carry structured tool records, but none of the
  seven formats carries a durable per-message native id — messages report
  empty native ids, so activities cannot anchor (staging fail-closed drop,
  R5.3); `pi`, `openclaw`, and `tencent-codebuddy` documented format
  knowledge carries no structured tool-call records. All seven stay
  `Unsupported` in the capability matrix, with the reason recorded in the
  Beta readiness ledger and pinned by new golden-corpus drift tests in each
  provider crate.
- Pseudo-user noise filtering in the provider parse layer: `claude-code`
  skips harness-injected user-role records by explicit envelope shape only
  (`<system-reminder>`, `<local-command-caveat>`/`Caveat:`, exact
  `[Request interrupted by user]` markers, empty-arg `<command-name>`
  envelopes, `<local-command-stdout>`/`<local-command-stderr>`,
  `<bash-input>`/`<bash-stdout>`/`<bash-stderr>`,
  `<user-prompt-submit-hook>`, `<task-notification>`, and whole-line
  `[Image: …]` isMeta image references — every rule cites its format
  evidence), and `codex` skips `# AGENTS.md` title and
  `<environment_context>` user injections (cc-switch rollout evidence).
  Filtered records count into `ParseReport.skipped` with a diagnostic and
  never enter the index; command envelopes carrying user arguments stay.
  The twelve formats without such injection shapes gain pinning tests
  asserting verbatim passthrough of noise-shaped user text, so a filter
  borrowed from another format cannot land silently.
- Rank signals for lexical search hits: displayed score is now
  `max(0, bm25 × recency_decay) − sidechain_penalty` with a 30-day
  exponential half-life (future timestamps clamp to full score) and a 0.3
  decay floor so old messages are never fully suppressed; sidechain hits pay
  a fixed 1.0 score penalty (same relevance ranks them after mainline hits).
  Constants live in one module (`application::ranking`) and every parameter
  is pinned by unit tests; ranking is recomputed from the injected
  application clock, so results stay deterministic per clock (tests inject a
  fixed clock; production tracks wall time as before). Applies to pure
  lexical retrieval (including lexical fallback) only — semantic hits and
  hybrid RRF fusion are unchanged.
- CJK single-character query recall: the FTS token stream now also carries a
  unigram for every Han character alongside the ADR-0007 bigrams
  (`application::cjk::fts_tokens_cjk`, shared by the index and query sides),
  so single-character queries such as `search 了` match sentences containing
  the character instead of returning nothing. Bigram recall for two-character
  and longer queries is unchanged; worst-case token growth over raw CJK text
  is about 4x (ADR-0007 §后果 updated). Existing databases pick up the new
  tokens via `index rebuild` (no schema change). `bigram_cjk` stays bigram-
  only and remains the guidance evidence source.

### Changed

- The public-facing security and readiness documents now state two limits they
  previously glossed over. `SECURITY.md` records that key-name redaction covers
  string values only (numbers/booleans under a secret-looking key pass through —
  `handoff-pack/v1`'s integer `max_tokens`/`used_tokens` were being emitted as
  the string `"[redacted]"`), and that absolute paths are not secrets to the
  ruleset: `original_working_directory` crosses machine boundaries verbatim
  because `resume` needs it, while `source_path`/`transcript_path` are omitted
  from that shape by design. `THREAT-MODEL.md` §7 no longer claims the
  data-root-locking spike "confirmed the lease does not support NFS" — the spike
  says network filesystems were *not verified* — and both remaining open
  decisions (path privacy mode, network data-root policy) now carry their fact
  basis and a recommended ruling for the owner to sign (§7.1/§7.2).
  `README.md` corrects three claims that did not survive checking: "every hit
  carries a source span" (four of fourteen adapters honestly report none), "CJK
  bigram" tokenization (unigrams + bigrams since the v17 projection), and the
  Kimi format cell now names `turn.prompt`/`turn.steer`. The owner release
  checklist was re-verified against the current tree: verify-release 10/10,
  privacy scan 0, `cargo deny` all ok, public-tree export clean, and the last
  three stale CLI `main.rs` references from the lib split repointed.

- Tool activity extraction now follows the record shapes real `claude-code` and
  `codex` transcripts write (structure census over local corpora: record types,
  tool names and argument key names only, never content). `codex` registers
  `function_call` (previously ignored — 88% of observed tool calls) alongside
  `custom_tool_call`, pairs both `function_call_output` and
  `custom_tool_call_output` by `call_id` (12805/12805 observed outputs pair, 0
  orphans; previously the two halves were crossed so nothing ever paired and
  status stayed `unknown`), reads the string `input` that `custom_tool_call`
  actually carries instead of an absent `arguments` object, and resolves an
  `apply_patch` envelope to the first `*** {Add,Update,Delete} File:` path
  instead of storing patch text. The kind closed set gained the names the
  census found (`PowerShell`, `shell_command`, `exec_command`, `apply_patch`,
  `Agent`, `spawn_agent`, `web_fetch`, `web_search`, `view_image`); everything
  outside it — MCP tools, orchestration/plan tools, user plugins — still fails
  closed to `kind = Unknown` with no target. Codex tool outputs carry no
  failure marker in any observed record, so status stays success-or-unknown and
  is never inferred from output text. `TOOL_ACTIVITY_TARGET_MAX_CHARS` moved to
  ports as the single owner of the 512-character target bound.
- Claude Code `tool_use` blocks now render as `Name(target)` summaries in the
  canonical message projection (idea from cc-switch's `extract_text_from_item`,
  MIT; implementation is our own). Claude Code stores tool calls inside
  assistant messages and those blocks have no `text` field, so 2610 of 4226
  observed assistant messages projected to an empty body — indexed, counted as
  committed, and matchable by no query. The summary makes "which tool touched
  which file" searchable. Only provider-recorded strings are used, under a
  fixed template, bounded by the same target cap as the store, so a body and
  its stored activity target agree verbatim; the catalog payload still holds
  the provider record in full and spans still point at source bytes
  (ADR-0004/THREAT-MODEL: the catalog never rewrites source text — this is the
  canonical projection, which already canonicalizes local-command envelopes).
  Those pinned canonicalizations keep priority over summaries.
- Message FTS body retention (borrowing list #3, ctx text-retention policy):
  a message's searchable projection is now bounded to 16 000 characters
  (`application::retention::MESSAGE_FTS_MAX_CHARS`, char-boundary truncation)
  at the index write paths, the payload-reprojection path
  (`searchable_text` — rebuild/merge/put), and the ingest entry construction.
  The catalog payload keeps the full provider text (ADR-0004/THREAT-MODEL:
  the catalog never rewrites source text), so `get-message` and context views
  are unchanged; only the FTS index volume is bounded. The same cap applies
  on every path so current-detection compares the same bounded text and
  idempotent re-sync still reports no change. Existing databases converge
  automatically: an oversized row is rewritten (bounded) on its next sync;
  `index rebuild` re-projects the whole index from the catalog.

### Fixed

- **`serve`'s slowloris-isolation test no longer asserts a wall-clock budget.**
  `integration_slowloris_does_not_block_a_fast_get` measured that a fast `GET`
  finished within 250 ms while a partial-header client was parked on the pool.
  That budget measures how loaded the host is, not whether the server
  serialises: the test passed 5/5 in isolation and failed inside a full
  `cargo test --workspace` run on a busy machine, which is exactly how it would
  flake on a shared CI runner. The property is now expressed structurally — the
  server's read timeout is set far beyond the request so the parked connection
  provably cannot be reaped first, meaning a `200` can only come from
  concurrent service, and a server that serialised would stall past the
  client's own read timeout and fail loudly. The reaping half moved to
  `integration_stalled_headers_are_answered_408`, which asserts the `408` with
  no timing assertion at all. Server behaviour is unchanged.

- **Silently wrong search results on stores built by an older binary**
  (index-projection versioning, schema v17). The FTS token transform is a
  contract between index time and query time; when it changed (pure CJK
  bigrams → unigram + bigram) nothing invalidated the tokens already on disk,
  and `PARSER_SEMANTIC_VERSION` does not cover this axis — it only triggers a
  re-parse when *parse* semantics change. Measured on a real 170,468-entity
  library (generation 9) built by the previous binary: MCP `search_sessions`
  returned **0 hits** for `配置备份` and `备份` while `clippy` matched and
  scored normally. An error would have been visible; a partially empty hit set
  was not. Now: `INDEX_PROJECTION_VERSION` is persisted as the store-level
  `store_metadata.index_projection_version`; write opens (`sync`/`ingest`/
  `index`) reproject from the authoritative catalog automatically (no
  re-parse — catalog payloads are authoritative for content) and advance the
  generation once; read-only paths fail closed with `schema_incompatible`
  (exit 9) naming `index rebuild` instead of returning a wrong hit set; and
  `doctor` reports `index_projection_version` /
  `index_projection_expected` / `index_projection_stale`. The version's
  contract covers every projection input — CJK tokenization,
  `MESSAGE_FTS_MAX_CHARS` retention truncation, `searchable_text`, the
  `session_fts` field set, and the `session_titles` / `session_repo_slugs`
  derivation rules (all reprojected by the same rebuild). The v17 migration
  stamps from an observable fact, so a fresh data root is never reported stale
  and never churns a rebuild on first open. `catalog`, `list`, and `get` are
  unaffected: the catalog is authoritative and the projection is derived.
  Measured on that library: v15 → v17 migration 0.19 s, one whole-store
  reprojection of 170,468 entities ≈ 140 s, after which `配置备份` and `备份`
  both recall again.
- Message edge relation classification (session-tree lineage, borrowed from
  hstry's `fork_type` three-way classification): a sidechain message's parent
  edge is now stored as `subagent` instead of `reply`. Only the claude-code
  format carries a provable edge type (`isSidechain` = subagent/branch flag),
  so this is the honest subset — fork/retry/continuation have no explicit
  format field and keep `reply` rather than being guessed. Edge relations are
  deterministic and re-derived on re-ingest, so existing databases converge
  on their next sync.
- serve query-string routing: `/api/search?q=...` no longer 404s.
- Smoke scripts assert the actual 9 MCP tools (was 7 after the provider wave;
  `generate_handoff` brought it to 9).
- `verify-release.py` runs with `--db` and a committed gate fixture.
- Redaction now covers secrets embedded inside prose (previously only
  whole-string secrets were matched).
- Error output at the Robot/JSON boundary now follows the same redaction
  discipline as success envelopes: error messages and details go through the
  shared redaction engine, and `ProviderError::Io` OS text (which can embed
  real absolute transcript paths on Windows) is masked at conversion, like
  `PortError::SourceIo`. MCP error frames share the same masking.
- The entry-point consistency e2e test skips (instead of failing the suite)
  when no Python interpreter is available.
- CI now runs the `semantic-candle` feature test suite, so that module is
  covered by the quality gate.

### Planned for 0.1.0

- First public release: CLI, Robot JSON protocol, MCP server, TUI, and Web UI
  adapters sharing one Application ADT. Full cross-entry release rehearsal is
  still required before publication.
- Session search over the implemented provider set (lexical FTS5 plus ranked
  fuzzy-lexical retrieval), resume metadata extraction, and handoff packs.
- Installer scripts for Windows (PowerShell) and Unix (Bash), release
  rehearsal automation, and an evidence-backed open-source gate manifest.
- Core Beta evidence harness: lexical recall at 10 = 1.00 and parse loss =
  0.00 on the committed synthetic gate fixture.
