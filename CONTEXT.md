# agent-session-grep Context

Local-first, CLI-first search engine over AI coding-agent session history across
16 planned providers (with evidence-based maturity levels). It normalizes
heterogeneous provider transcripts into a canonical domain model with stable
identity and nonlinear message graphs, then serves retrieval, resume, and
handoff through CLI, Robot, MCP, TUI, and loopback Web UI surfaces sharing one
Application ADT. Native GUI is not a first-release surface, but remains a
future extension boundary.

## Language

**Session**:
One continuous conversation between a user and a coding agent, as recorded by
a provider. A session lives across source documents (a transcript file and its
subagent files) and may be split or merged across providers.
_Avoid_: conversation, chat, thread, history

**Provider Session ID**:
The provider-issued identifier used by that provider to resume a Session. It is
resume metadata, not the Canonical Session identity: it is interpreted together
with the provider and may be unavailable for reconstructed or partial Sources.
_Avoid_: session_id (ambiguous), StableId, Canonical Session ID

**Original Working Directory**:
The working directory recorded by the provider when a Session ran. It is a
resume hint and may later be missing or stale; it is not the Source locator,
Transcript storage directory, repository root, or part of Stable Identity.
_Avoid_: session folder, transcript folder, project path (unless equivalence is proven)

**Source (document)**:
One transcript file on disk as recorded by a provider (e.g. a Claude Code
`*.jsonl` or a Codex rollout). Sources are the unit of capture, fingerprinting,
and incremental sync; a SourceDocument entity is content-addressed and holds no
path.
_Avoid_: file, transcript, input

**Transcript**:
The raw provider-format record of a session or subagent. Used only when
talking about the on-disk format before normalization.
_Avoid_: (none — kept only because provider code uses it)

**Canonical Model**:
The normalized, provider-agnostic domain representation (Message, Session,
SourceDocument, MessagePlacement, MessageEdge) that all providers are reduced
to. Provider-native fields never leak past the adapter boundary.
_Avoid_: schema, normalized form, DTO

**Stable Identity (StableId)**:
The typed, content-derived identifier (`src_v1_`/`doc_v1_`/`ses_v1_`/`msg_v1_`)
that survives file moves, renames, incremental appends, and re-scans. Backed
by BLAKE3; stability is graded native / reconstructed / unstable, and the wire
string encodes no stability tier.
_Avoid_: id, key, UUID, hash

**Placement**:
The occurrence of a Message at a position in one Source. A message can have
many placements across sources; a PlacementId is derived from
(session, document, message, ordinal) and is path-independent.
_Avoid_: span, location, occurrence (occurrence is the general concept,
placement is this project's term)

**Mainline**:
The canonical linear view of a session chosen from its message graph — the
leaf most likely to represent the "real" conversation, with edged leaves
preferred over edgeless copies and internal messages excluded from leaf
competition. Selecting a mainline is the deterministic resolution of a
nonlinear graph.
_Avoid_: thread, active branch

**Evidence**:
The provenance attached to a retrieval result: the source span (byte or
unknown precision) where a message's text was observed. Evidence is what makes
a search hit inspectable back to its source.
_Avoid_: citation, source trace

**Catalog**:
The authoritative store of canonical entities (the source of truth).
FTS and fts_ids are projections that must be fully rebuildable from it.
_Avoid_: database, index

**Generation**:
A monotonically advancing, CAS-guarded version of the catalog that advances
only on committed batches. Cursors pin against a generation; rebuilds and
rollbacks are generation-visible.
_Avoid_: version, commit id

**Outbox (index batch)**:
The durable two-phase intent record (`begin_index_batch` →
`commit_index_batch`) that makes a batch commit atomic: a crash leaves a
harmless `building` row that recovery converges to `aborted`.
_Avoid_: journal, write-ahead log

**Snapshot (source snapshot)**:
The immutable `(len, mtime_ms, fingerprint)` identity of a Source captured at
open time and re-verified before commit; drift invalidates the staging batch.
_Avoid_: file state, checksum record

**Sync**:
The incremental reconciliation of one or more Sources against the catalog:
fingerprint-skip when unchanged, atomic per-batch commit when changed,
whole-source tombstone for emptied sources.
_Avoid_: ingest (ingest is the one-off path for a single file), import, update

**Provider Adapter**:
The per-provider implementation of the probe + parse contract that reduces a
provider's transcript format to canonical events. Confidence (confirmed /
high / low) selects the adapter; ambiguity refuses rather than guesses.
_Avoid_: parser, connector, driver

**Probe**:
The cheap pre-parse inspection of a source that decides which adapter
applies and at what confidence. Probing happens before any parsing.
_Avoid_: detection, sniffing

**Staging**:
The in-memory, atomically-discardable accumulation of parsed canonical events
before commit. A structural failure discards the whole batch; nothing is ever
half-committed.
_Avoid_: buffer, cache, pending writes

**Tombstone**:
The marker that a Source's content was deliberately removed (e.g. an emptied
source file), distinct from missing or never-seen. Absence is only confirmed
by a complete root scan; deletions are never inferred from silence.
_Avoid_: delete marker, gap

**Cursor**:
The self-contained, unsigned, keyless pagination token bound to
(contract major, generation, issued/expires, query digest, sort digest,
offset). Anti-tamper, not anti-forgery; expiry and generation mismatch are
explicit errors, never silent restarts.
_Avoid_: page token, offset pointer

**Response Budget**:
The byte/item caps (default 4 MiB / 1000 items / 2000 snippets / 500 messages /
1000 spans) that clamp retrieval responses. Truncation is explicit and
ordered, never silent.
_Avoid_: limits, pagination, max results

**Snippet**:
The bounded text preview attached to a search hit in human output only.
Not redacted (owner decision 2026-08-13, ADR-0004); snippets count against
the Response Budget.
_Avoid_: preview, excerpt, summary line

**Plain-text Query**:
The only supported search-query form: whitespace-split literal tokens joined
with implicit AND. FTS operators and phrase/prefix syntax are literal text,
not a query language (ADR-0003).
_Avoid_: FTS query, advanced query, raw query

**Data Root**:
The local directory (config / data / cache / logs) that holds the catalog and
its sidecars. A single writer lease (OS-level lock) serializes writers;
readers open without the lease.
_Avoid_: storage, store directory, working dir

**Gate D**:
The full-corpus real-data regression over the operator's own transcripts. Its
invariants are defined in `scripts/evidence/real_data_regression.py`
(`INVARIANT_IDS`) — sync ok, no parse loss, sessions present, context nonempty,
span coverage, rebuild stable, and sources unchanged. The gate that determines
whether a release claim holds on real input.
_Avoid_: regression, e2e, integration test

**Provider Maturity**:
The honest public rating of a provider adapter: certified / GA / beta /
experimental / unsupported. Never inflated; promotion requires golden tests
and source spans.
_Avoid_: support level, compatibility

---

## Decision records

The 2026-08-12 to 2026-08-15 decision rounds that shaped this glossary are
recorded in their durable artifacts: irreversible or contract-level decisions
live in the ADRs under `docs/adr/` (indexed in
`docs/adr/ADR-0009-cross-boundary-output-redaction.md` and
`docs/adr/ADR-0010-provider-maturity-rollback.md`), the release execution plan
and its locked decisions in `docs/product/OPEN-SOURCE-ROADMAP.md`, and the
canonical model / adapter contract in the RFCs under `docs/architecture/`
(`RFC-0001-canonical-model-and-stable-id.md` and
`RFC-0002-provider-adapter-contract.md`). This file is the glossary only and
does not duplicate the decision log.
