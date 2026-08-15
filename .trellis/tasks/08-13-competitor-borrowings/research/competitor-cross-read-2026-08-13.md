# Competitor cross-read — 2026-08-13

## Scope and evidence boundary

This review uses pinned local checkouts and the repository reuse audit. No source was fetched or updated. The fixed identities are recorded in `agentsessions-competitor-research` memory and include `ctx@06bc5ed`, `agent-sessions@af4793d`, `claude-historian-mcp@b627cdf`, and the remaining projects named in the parent PRD.

`cass` remains a strict clean-room exclusion because its license contains a Restricted-Party Rider. Its source must not be read again. Only the already-recorded abstract ideas (fail-open reads, staged publish, robot contract freeze) may inform an independent Rust implementation.

The repository's `docs/operations/REUSE-LICENSE-AUDIT.md` is authoritative when it is stricter than an upstream license. TypeScript, Swift, and Go implementations are idea-only by default; fixtures are synthetic and never copied from real transcripts.

## Strongest cross-project conclusions

### AgentSessions already has the stronger core contracts

- Typed stable IDs and placement IDs instead of path-derived identity.
- Verified source snapshots and source-scoped replacement/tombstone semantics.
- Catalog authority with rebuildable FTS projection and generation activation.
- Placement-aware mainline/full context and exact source evidence.
- Cursor binding to query, sort, generation, contract version, and expiry.
- Explicit response budgets, truncation, pagination, canonical errors, and clean robot/MCP stdout.
- Provider golden/property tests plus CLI and MCP process-level E2E tests.

These are independent capabilities, not claims of copied implementation.

### The four current P0 borrowings are justified

1. Provider/time search predicates must be typed, normalized, pushed down before SQL `LIMIT`, and bound into cursor identity.
2. Message retrieval must use typed placement authority, expose explicit multi-session ambiguity, and never infer transcript adjacency from filtered text records.
3. Context disclosure benefits from deterministic `raw | talks | sessions` levels, but summaries must remain structural and must not invent accomplishments, decisions, or solutions.
4. Search guidance should expose literal match evidence and executable next calls using real IDs; it must not expose FTS syntax or use regex/LLM classification.

The integrated implementation follows these boundaries rather than copying competitor code or strings.

## High-value later backlog

### P0 — identity and correctness prerequisites

- Namespace provider-native message/session identity before expanding discovery. Identical native IDs from different providers currently have a theoretical path to one global wire identity; migration and compatibility require a separate design.
- Make `list_sessions` return sessions only rather than generic catalog entities.
- Finish default system-noise exclusion with an explicit `include_system` opt-in.
- Finish optional group-by-session with best hit, stable ordering, occurrence counts, filter-before-group behavior, and cursor/budget tests.

### P1 — source lifecycle and operator truth

- Persist a provider-source catalog: discovery origin, availability, importability, trust/fidelity, bounded probe outcome, and last successful scan.
- Add durable source-scoped cursor/checkpoint APIs for append-tail ingestion, persisted only after successful activation.
- Add a genuinely read-only deep doctor: integrity, foreign-key, FTS/catalog, WAL, generation, and interrupted-batch checks.
- Add crash-recoverable FTS bulk mode with a durable recovery marker and bounded merge/optimize work.
- Replace Han-only bigrams with tested scriptgram behavior if broader unsegmented-script support becomes a requirement.
- Add typed file/plan/config/session facets from persisted provenance, not heterogeneous filesystem fan-out at query time.

### P2 — optional distribution and rich evidence

- Hash-aware, atomic Skill installation with separate plugin, binary, and data lifecycles.
- Bounded tool/reasoning/failure/file-touch event types and evidence-backed locate/file-history views.
- Trusted, signed, disabled-by-default provider plugin manifests only after a separate execution security review.
- Attachment descriptors and byte-range retrieval with strict type/size limits; never index image bytes.

## Rejected patterns

- Raw SQL or arbitrary path access over MCP.
- Regex interpolation of user queries/tool names, raw FTS syntax, or regex-derived accomplishments and solutions.
- Treating later assistant text as proof that an earlier error was solved.
- Treating filtered-record adjacency as transcript adjacency.
- Path decoding by replacing punctuation or path-derived canonical identities.
- Silent parse/backend failures converted into clean empty results.
- Search-triggered refresh writes, destructive reindex markers, or ordinary-open migrations advertised as read-only.
- Unbounded all-project/all-file scans, unbounded child-process output, or whole-corpus lowercase copies.
- Automatic shell resume or arbitrary local plugin execution through a read API.
- Destructive Skill replacement, unsigned dependency fetching, or copying upstream fixtures/prose.

## Project-specific notes

### `ctx`

The strongest idea is its explicit source lifecycle: bounded discovery, source-scoped identity/checkpoints, classified import outcomes, and recoverable FTS maintenance. Do not borrow its weaker FNV-style identity, plain offset cursors, raw SQL surface, search-time refresh writes, or plugin trust assumptions.

### `claude-historian-mcp`

It performs live query-time JSONL scans with short caches; it is not an incremental indexer. The pinned version exposes `search -> inspect`, not `search -> at -> get_session`, and ordinary search hits do not reliably expose identifiers needed for progressive disclosure. Useful presentation ideas are typed facets and compact projections; reject raw regex, false adjacency, inferred solutions/accomplishments, silent failures, weak timeframe parsing, and schema drift.

### `agent-sessions`

Useful product ideas are filtered session retrieval, metadata-first discovery, local transcript windows, attachment descriptors, and dense table/detail workflows. Do not borrow its global native-ID assumption, path-derived IDs, destructive migration patterns, credential/cookie probing, or monolithic desktop ownership structure.

## Licensing and release gate

The local reuse audit remains Draft. Any direct-copy or adaptation classification still requires owner/approver sign-off, attribution coordinates, and license obligations before release. Until then, the safe default is clean-room behavior-level reimplementation and independently generated tests/fixtures.
