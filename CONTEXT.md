# agent-session-grep Context

Local-first search engine over AI coding-agent conversation history (Claude
Code, Codex, and future providers). Normalizes heterogeneous JSONL transcripts
into a canonical domain model with stable identity and nonlinear message
graphs, then serves full-text retrieval through CLI, Robot, MCP, and TUI
surfaces sharing one application ADT.

## Language

**Session**:
One continuous conversation between a user and a coding agent, as recorded by
a provider. A session lives across source documents (a transcript file and its
subagent files) and may be split or merged across providers.
_Avoid_: conversation, chat, thread, history

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
The full-corpus real-data regression — six invariants (sync ok, no parse
loss, sessions present, context nonempty, span coverage, rebuild stable) over
the operator's own transcripts. The gate that determines whether a release
claim holds on real input.
_Avoid_: regression, e2e, integration test

**Provider Maturity**:
The honest public rating of a provider adapter: certified / GA / beta /
experimental / unsupported. Never inflated; promotion requires golden tests
and source spans.
_Avoid_: support level, compatibility

---

## Decision log (2026-08-12)

- **Ultimate goal (Q1)**: finish 0.3, then decide on release based on real
  self-test feedback — not a fixed v1.0 gate.
- **Problem being solved (Q2)**: both (a) retrieving "why did we do that"
  decision context from old sessions, and (b) finding code/errors across
  providers (Claude Code + Codex).
- **Audience (Q3)**: open-source to the community; keep the architecture
  release-shaped without committing to the external release gates (signing /
  notarization / certification) yet.
- **0.3 exit (Q4)**: the 7 task acceptance criteria **plus** minimal
  open-source readiness (README, LICENSE, no leaks) together form the exit.
- **Naming (Q5)**: rename the project to `agent-session-grep` (already
  present in docs as the search tool); deep rename including crates and
  binary name.
- **Perf acceptance (Q6)**: full-corpus (1,124+ files) Gate D re-run is the
  objective completion line; first-ingest < 30 min was already validated at
  scale on the small corpus.
- **Rename scope (Q7)**: deep — workspace crates, internal identifiers, CLI
  surface, binary name all align to `agent-session-grep`.
- **Rename timing (Q8)**: after the performance task closes, as an
  independent task.
- **Execution sequence (owner-confirmed 2026-08-12)**: 0.3 closes →
  rename executes immediately → owner self-tests → only after the owner is
  satisfied do open-source release preparations begin → public release is
  the owner's final call. The repository stays PRIVATE throughout.
- **Regression strategy (Q9)**: run the full-corpus re-run now; add first-run
  progress frames as part of the run.
- **Docs (Q10)**: this CONTEXT.md is the canonical glossary; decisions are
  logged here (and, when irreversible, in ADRs under docs/adr/).

## Decision log (2026-08-13 — UX review fix round)

- **Fix scope (Q1)**: this round fixes all findings introduced by the UX diff
  plus the parser-robustness batch; perf P1s, MCP pre-existing contract gaps,
  and the evidence/install-script cluster move to a separate task.
- **Task structure (Q2)**: new Trellis task `08-13-ux-review-fixes` owns the
  production fixes; `e2e-hardening-followup` keeps its tests-only scope.
- **Snippets (Q3, owner revised)**: keep human search snippets with no
  redaction — the tool is local-first with zero network egress; on-screen
  secret display is accepted risk (ADR-0004). Budget enforcement is still
  required.
- **Search semantics (Q4)**: search is officially plain-text-only;
  literalization is a feature, not a regression (ADR-0003).
- **Missing-entity contract (Q5)**: get and show both return exit 4 +
  not_found; smoke scripts and CONTRACT updated in the same change (ADR-0005).
- **show projection (Q6)**: messages get the curated projection;
  session/document entities fall back to generic key/value rendering.
- **Machine-mode help/version (Q7)**: help and version always exit 0 in every
  mode; robot/json/jsonl wrap the text in a success envelope `data`
  (ADR-0006). No mode-dependent exit codes.

## Decision log (2026-08-13 — next round planning)

- **Next-round scope (Q1)**: three parallel tasks — A core usability (CJK
  tokenization, probe tolerance, merged-file session diagnostics, search-hit
  session context), B competitor borrowings (time/provider filters,
  system-noise filtering, group-by-session, around/get_message, summary
  levels), C context/perf Top 5 (edge batching, mainline index, timestamp
  pre-parse, rebuild join, merged IN). Each task owns a worktree; A lands
  first because B's contract depends on A's schema decisions.
- **CJK scheme (Q2)**: CJK bigram pre-processing at index write time
  ("配置备份" → "配置 置备 备份"); the FTS5 projection stays rebuildable via
  index rebuild. Trigram rejected (≤2-char queries die — the core Chinese
  query unit); dual-column is a later enhancement (ADR-0007).
- **Dependencies (Q3)**: A first (index-layer + contract), B depends on A's
  search-hit schema, C fully independent and parallel.
