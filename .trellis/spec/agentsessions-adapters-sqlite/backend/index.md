# agentsessions-adapters-sqlite — Backend Guidelines

> SQLite + FTS5 storage adapter. Implements the catalog store and search index
> ports. This is the only crate that talks to `rusqlite`. The catalog is the
> source of truth; the FTS index is a rebuildable projection.

---

## Role in the architecture

`agentsessions-adapters-sqlite` is a **driven adapter**: it implements the
storage/search/context-graph ports declared in `agentsessions-ports`. It owns
the SQLite schema, migrations, writer lease, durable outbox, generation
tracking, source entity/placement claims, relation completeness, tombstones,
and the FTS5 projection.

Core invariant: **`catalog` is authoritative for payload; `fts` + `fts_ids`
are derived and must be fully rebuildable from `catalog` at any time.**

---

## Pre-Development Checklist

- [ ] Am I preserving the catalog-as-truth / FTS-as-projection split? Never
      make search the source of record.
- [ ] Schema change? Bump `PRAGMA user_version`, add a forward migration, and
      keep it non-destructive. Never drop/recreate to dodge a migration.
- [ ] Does the write go through the durable outbox two-phase path
      (`begin_index_batch` → `commit_index_batch`) so a crash leaves only a
      side-effect-free `building` row that `recover_interrupted` converts to
      `aborted`?
- [ ] Generation advance: is it guarded by CAS on `store_metadata`? A failed
      transaction must not pollute the active generation.
- [ ] Identity fidelity: does the code read `fts_ids.id_json` first and only
      fall back to `StableId::from_wire` (which degrades to Unstable) when the
      sidecar is absent? The catalog wire key does NOT encode stability.
- [ ] Source read-only: nothing here writes back to a provider source file.

---

## Key patterns (real code)

- `verify_pending_in_tx(...)` — pre-commit validation inside the transaction:
  checks active-generation CAS + durable intent state + digest/manifest.
- `rebuild_index()` — clears `fts`/`fts_ids` and re-projects the whole catalog
  in one transaction, advancing generation; rolls back cleanly on failure.
- `interrupted_batch_count()` — read-only doctor signal for pending `building`
  intents.
- `WriterLease::try_acquire(...)` — accepts only `fs4` `Ok(true)`, retains the
  authoritative locked handle, and maps contention to path-redacted WriterBusy.
- `searchable_text(payload)` — extracts the searchable body from the canonical
  payload for FTS.
- Filtered FTS search — uses prepared queries with `fts MATCH` plus provider,
  timestamp, facet and visibility predicates before `LIMIT`. Each corpus uses
  BM25 then canonical wire ID; message/session ranks combine through RRF k=60,
  never by comparing their raw BM25 scores. SQLite FTS5
  auxiliary functions and `MATCH` name the real virtual table (`fts`), not a
  table alias.
- `ContextGraphStore` reads — return typed Domain messages/documents/placements/
  edges, group reverse candidates by distinct Session, and expose aggregate
  placement/claim counts. They never return SQL rows or compatibility JSON.
- `merge_session_payloads(...)` / `merge_message_payloads(...)` — union the two
  projections of one entity that arrive from different sources. Real corpora
  need this: one logical session spans many transcript files, and resuming or
  forking a conversation copies its history into the new file, so the same
  entity legitimately arrives more than once per batch.

## Cross-source entity merging

`commit_source_batches_if_changed` folds an entity that several sources claim
instead of rejecting the batch. Which fields may differ is deliberately narrow:

| Entity | Unioned fields | Everything else |
|---|---|---|
| Session | `messages` (append-order), `documents` (sorted) | fields not in the union set (`document`, `documents`, `messages`) are not preserved by the merge |
| Message | contextual compatibility keys (`session(s)`, `span(s)`, parent provenance, sidechain, seq) | stable role/text and unknown intrinsic fields must match; timestamp has one deliberate exception (below) |

- **Timestamp exception (Codex)**: `timestamp` may differ as `string` vs `null`
  across projections of the same stable message — the old Codex adapter stored
  the occurrence-local outer envelope timestamp, the current one emits no
  stable timestamp, so re-ingesting an old catalog must not conflict. The
  merged value converges deterministically on `null` regardless of merge
  order (no stable timestamp exists for the entity).

- Each unioned field keeps a **singular alias** (`document`, `session`, `span`)
  holding the first entry, so readers written against the pre-union shape keep
  working. On a shared entity the alias names *one* contributor, not all of
  them — treat it as a compatibility shim, never as the complete answer.
- The merge starts from **the value already in the catalog**, not just from the
  other source in the current batch. Without that, syncing a corpus in several
  invocations would let each batch overwrite the previous batch's members.
- Identical stored bytes skip the merge entirely. That keeps an unchanged
  re-sync a content-level no-op and avoids forcing slice-era opaque payloads
  through a JSON parse.
- Once every known contributing source is relation-complete, compatibility
  aliases are regenerated subtractively from placements/edges/claims:
  divergent parents/sidechain values become `null`, spans name exact placement
  and document identities, and zero-message Session documents survive through
  source entity membership. Mixed legacy/incomplete state preserves existing
  aliases and never pretends they are complete.

## SQLite v7 relation commit

- Migration v6→v7 is one explicit transaction. The four relation tables,
  relation indexes, three default-empty outbox manifest columns, and
  `PRAGMA user_version = 7` commit or roll back together. No legacy aliases are
  backfilled into fabricated placements or completeness markers.
- Durable intent hashes entity changes plus relation upserts/deletes and the
  final source replacement state: entity memberships (including document
  attribution), placement claims, and relation marker state. An honestly empty
  replacement is still manifest data.
- A complete scan replaces claims and may delete only facts with no surviving
  source claim. An incomplete scan unions observed entities/placements/edges,
  derives no missing-record tombstones, and clears its completeness marker.
  Observed relation changes are allowed only when every final claimant observes
  the same placement/edge/root fact.
- Catalog, FTS, identity sidecar, source claims, relations, active generation,
  and outbox activation are one phase-2 transaction. Failure leaves the
  durable `building` intent but no data or generation side effect.
- Every ordinary and source-batch write path validates incoming StableId
  metadata against persisted `fts_ids.id_json` before taking a no-op shortcut
  or creating an outbox intent. A cross-batch metadata conflict fails without
  changing generation, claims, catalog, or index state.
- FTS rebuild touches only `fts`/`fts_ids`; relation rows, claims, completeness,
  context graphs, and aggregate context stats must remain byte-for-byte stable.
- Context reads fail `SchemaIncompatible` with a bounded re-ingest action when
  any known contributor lacks a v7 completeness marker. They never parse
  compatibility aliases as graph authority.

## Common mistakes

- Assuming `StableId::from_wire` restores Native/Reconstructed — it returns
  Unstable. Full identity lives only in `fts_ids.id_json`.
- Advancing generation outside the committing transaction.
- Adding an `ALTER`-free destructive "migration".
- Treating a per-source fact (session, span, parent) as intrinsic to a message.
  Message *identity* is shared across files; its *position* is not.
- Treating `skipped > 0` as a complete replacement or retaining an old
  relation-complete marker after an incomplete re-scan.

---

## Resume Metadata claims (ADR-0009, schema v8)

`source_session_resume_claims` holds source-scoped Resume Metadata written
atomically with source replacement and cleared on source removal (tombstone).
The canonical `ses_v1_*` is the catalog identity; the Provider-native ID and
Original Working Directory are isolated Resume Metadata, never entering FTS
text, opaque Session payload, diagnostics, progress, or errors.

- **Pair association**: `original_working_directory` is returned only when
  `pair_observed = 1` AND `original_working_directory_state = 'resolved'`.
  A cwd seen without a co-observed Provider Session ID is suppressed even
  when the ID itself is resolved. Implemented in `resume_metadata_from_claim`.
- **Fail-closed conflicts**: when the same canonical Session has conflicting
  claims across Sources, `resume_of` returns `resume_available = false` with
  `unavailable_reason = "conflicting resume metadata claims"` and discloses
  none of the conflicting values. It never picks by source path or order.
- **No reverse derivation**: there is no path from `ses_v1_*` back to the
  Provider-native ID. The native ID lives only in the claim row.
- **Explicit multi-session observations**: one Source can carry independent
  claims keyed by canonical Session. An ambiguous report-level observation
  still fails closed; per-message session observations are not conflated.
- **Batched reads**: `resume_of` chunks via `BATCH_IN_CHUNK` (no N+1) and
  short-circuits empty input with zero SQL statements. The `session_id` lookup
  is backed by index `source_session_resume_claims_session`, not a full scan
  (asserted by an EXPLAIN QUERY PLAN test).
- **Latest activity**: `latest_activity_ymd_for_sessions` batches
  `MAX(json_extract(catalog.payload, '$.timestamp'))` per canonical Session,
  truncated to `YYYY-MM-DD`, for the Human table 日期 column. No port/DTO/schema
  change; Robot/MCP output is unaffected (Human-mode only projection).

## Session metadata search projection (schema v11)

`schema v11` (`migrate_v10_to_v11`) adds `session_fts` (FTS5,
`session_wire UNINDEXED, text`) and `session_fts_ids` (session wire ↔ FTS
rowid sidecar) as a rebuildable, privacy-safe search projection over Session
metadata. It is a derived projection like the message `fts`, populated from
`source_session_resume_claims` + catalog; never authoritative.

- **Indexed fields**: resolved Provider-native Session ID
  (`provider_session_id_state = 'resolved'`), Original Working Directory only
  when `pair_observed = 1` AND resolved, and the first chronological valid
  user request as the deterministic title-like field. Each field is bounded by
  `SESSION_SEARCH_FIELD_CHARS` (4096 chars). Provider custom title/summary are
  not yet part of the Canonical provider contract and remain explicitly
  deferred.
- **Fail-closed conflicts**: when the same canonical Session has conflicting
  claims across Sources (any field disagrees), `session_search_text` returns
  `None` for the claim block — none of that Session's claim values are
  indexed, matching the `resume_of` conflict rule. It never picks by source
  path or order.
- **Never indexed**: `source_path`, transcript path, native IDs from other
  providers, or anything outside the fixed privacy-filtered metadata shape.
- **Maintenance**: `commit_index_batch_with_relations` collects affected
  Sessions (upserts/deletes, placement moves, source replacements and claim
  rows) and rebuilds their `session_fts` rows in the same transaction;
  `rebuild_index` repopulates the projection entirely from catalog + claims.
  Delete is rowid-scoped via the `session_fts_ids` sidecar.
- **Query merge**: `query_filtered` runs the message-FTS query and a second
  `session_fts MATCH` with the same `safe_fts_query`, merges deterministically
  (score desc, wire id asc) and truncates to the limit. A Session already
  represented by a matching non-system Message hit is deduplicated; a
  metadata-only Session returns its canonical identity (or its first
  non-system Message as representative). Candidate exclusions use a single
  JSON parameter (`json_each`) rather than one SQL expression per message;
  final merging uses deterministic RRF and canonical wire IDs.

## Scenario: Explicit installation relocation (schema v18)

### 1. Scope / Trigger
Indexing a new installation, upgrading v17 provenance, or reconnecting an
unchanged installation at an explicitly selected new directory/drive.

### 2. Signatures
`resolve_or_allocate_installation_namespace(provider, source, legacy_seed)`
returns a staging namespace. `relocation_preview(provider, from, to, ttl)` is
read-only. `open_for_relocation(path)` acquires a writer lease without schema
migration or projection rebuild. `apply_relocation(provider, from, to, ttl,
plan, backup)` returns the bounded `RelocationResult`; `backup_to(path)` never
overwrites a destination or its SQLite sidecars.

### 3. Contracts
`installation_namespaces` freezes the exact legacy seed or a new opaque seed;
`installation_locations` stores flat current/retired root ownership;
`source_installations` binds source locators; `installation_relocations` stores
private committed receipts. Migration skips unverifiable/ambiguous provenance
without rekeying historical entities. New namespace reservations live only in
memory until their source replacement activates. The durable source manifest
includes the assignment; a failed source commit leaves no registry pollution.
Before selecting or reserving a namespace for an unregistered input,
`validate_unbound_source_locator` checks existing unbound scan locators using
`normalize_absolute_path`. Include NULL provider metadata. A different stored
spelling with the same lexical key is `InvalidRequest`, even if an exact-text
row also exists; preserve the original locator, all tables and generation.
An exact unique legacy locator still requires complete native-session proof.
Registered current bindings keep their indexed path. The unbound cold path
streams rows with bounded memory, but may scan legacy candidates per input;
it is not an indexed or batched time-complexity optimization.


Relocation extends the existing outbox manifest and activation transaction.
Move every live source locator in `source_scans`, `source_membership`,
`source_placement_membership`, `source_relation_scans`,
`source_session_resume_claims`, `tool_activity_membership`,
`usage_event_membership` and `source_installations`, plus registry ownership,
receipt, generation and intent state together. Keep canonical payloads,
identity sidecars, intrinsic relations, native observations, original CWD and
historical outbox manifests unchanged. Validate destination logical snapshots
(including committed SQLite WAL) before and immediately before activation.

Preview pins a read transaction, streams source enumeration in 128-row pages
and retains bounded mapping metadata (up to 65,536 sources), never transcript
payloads or embeddings. A verified Backup-API snapshot is published to a new
file before creating the relocation intent. Only the private backup switches
out of WAL; its temporary sidecars are cleaned after connections close.
Read-only and stale-schema opens cannot create or migrate catalogs. Ordinary
write-open auto-reprojection must not run before a relocation backup.

The compatibility alias defaults to 90 days (1..365). It never authorizes an
ordinary scan of the retired installation. Expiry permits a new independent
installation, never namespace reuse from a stale staging reservation. It does
not expire canonical IDs. Repeated mappings are unchanged only while the
complete selected ownership/source set still matches the committed receipt.

### 4. Validation & Error Matrix
Malformed/occupied/ambiguous roots, unsupported provider or invalid policy ->
`InvalidRequest`; stale generation -> `GenerationMismatch`; unreadable source
-> `SourceIo`; changed content -> `SnapshotChanged`; missing lease ->
`WriterBusy`; stale schema -> `SchemaIncompatible`; backup/SQL failure ->
bounded backend error. All failures preserve live tables and generation;
a building intent may remain for ordinary abort recovery. Public diagnostics
and result fields contain counts/digests, never paths/native IDs/content.

### 5. Good/Base/Bad Cases
Good: several legacy namespace groups under one moved root retain distinct
native Sessions. Base: re-syncing the new root is a no-op. Bad: using the new
path as namespace input, accepting a stale receipt after a sibling installation
was added, or allowing one provider's retired root to block another provider.

### 6. Tests Required
Use only synthetic sources. Assert full table/identity/context/resume/backup
snapshots, not just counts; inject failure at every locator table, registry,
receipt and activation. Cover legacy migration rollback, opaque allocation,
independent same-native-ID installations, source deletion, WAL-only change,
plan lifetime/mapping/generation, target conflicts, reverse moves, retired
expiry, casing/separators/Unicode, read-only preview and no-clobber backup.
Cover provider-less legacy Windows locators with raw and normalized inputs,
ambiguous equivalent rows, exact legacy proof, and preservation on refusal.
The namespace guard is shared by ingest, explicit sync and discovery; tests
must include those routes rather than relying on CLI input normalization.


### 7. Wrong vs Correct
Wrong: persist a namespace during parsing or update only `source_scans`.
Correct: stage the assignment, then atomically activate all authoritative facts
through the same durable manifest and generation CAS used by source commits.

## Scenario: Read-only lifecycle and logical source snapshots

### 1. Scope / Trigger
Opening catalogs, ingesting live SQLite sources, and rebuilding embeddings.

### 2. Signatures
`SqliteStore::open` uses `SQLITE_OPEN_READ_ONLY`; `open_for_write` acquires the
writer lease before migrations. `SourceBatch.resume_claims` is a vector of
per-session claims. `rebuild_embeddings_from_catalog(model_id, dimension,
batch_size, encode)` returns indexed/skipped/cleared counts.

### 3. Contracts
Read opens never create/migrate a catalog and use a finite 1-second busy timeout.
SQLite source capture uses Backup under a pinned read transaction, with a
128 MiB logical-size ceiling and guarded temporary destination. Never checkpoint
or write the provider source. Logical fingerprints include committed WAL data
and remain current across an external checkpoint without logical changes.
Per-session resume claims share the existing `(source_path, session_id)` key;
no schema migration is required. Parser semantic version 2 forces old source
scans to reparse. Provider parser `TempDb` keeps `conn` before `_guard` so
SQLite closes before cleanup. The guard owns the private database plus its
`-wal`/`-shm` files; the last read-only connection does not guarantee their
removal. Register that guard before writing bytes, and keep the file handle
inside its lifetime so write/sync failure also cleans the copy. Never use
this cleanup on original provider sources or enumerate unrelated temp files. Validate duplicate facts/claims/StableId metadata before any
no-op shortcut. Old building intents are aborted on writer recovery.
Embedding rebuild uses bounded wire-ID keysets (batch 1..512), finite vectors,
and one transaction for replacement plus generation. Encoder failure rolls back.
Semantic query scans rows but retains only bounded top-k candidates; it is an
exact scan, not ANN and not a production latency guarantee.

### 4. Validation & Error Matrix
Absent read catalog -> backend error without creating a file. Wrong schema
-> `SchemaIncompatible`, with explicit writer-maintenance action. Source changes
-> `SnapshotChanged`; oversized snapshots -> `SourceIo`. Invalid batch facts,
vectors or encoding failures -> error without changing authoritative data.

### 5. Good/Base/Bad Cases
Good: two sessions in one live WAL DB retain separate placements/resume claims.
Base: repeated unchanged logical snapshot is a no-op. Bad: copying only the
main DB, silently merging sessions, or clearing old vectors before encoding.

### 6. Tests Required
Assert absent/stale/read-under-writer catalog behavior, unchanged source bytes,
WAL-only updates/checkpoints, per-session claims, invalid no-op batches,
encoder rollback, keyset query plans, finite scores and bounded top-k equivalence.
Both SQLite provider crates must create real temporary WAL/SHM files, then
assert all owned files disappear after successful reads and query errors.

### 7. Wrong vs Correct
Wrong: call migration from a read command or return early before batch validation.
Correct: lease writes explicitly and validate all incoming facts before no-op.

---

## Scenario: Journal retention compaction (schema v19)

### 1. Scope / Trigger
Explicit maintenance on a long-lived catalog whose terminal outbox batches keep
full detail manifests for every historical rewrite. Preview, staged plan, apply
and recovery are the only surfaces; ordinary open paths never compact.

### 2. Signatures
preview_journal_compaction() -> JournalCompactionPreview is read-only.
stage_journal_compaction(preview) -> JournalCompactionStage persists the plan with
a CAS plan digest. apply_journal_compaction(id) -> JournalCompactionOutcome runs
the single-transaction rewrite. recover_journal_compactions() converges staged
plans explicitly; journal_compaction_event(id) reads the audit row.

### 3. Contracts
Permanent bytes: operation_id, base/target_generation, state, operation_digest,
durable_point, created/committed_at_ms, error_code, relocation_json,
detail_format. Unresolved rows (building / search_built / cleanup_pending) keep
full detail forever and never enter a plan. Terminal rows (activated / aborted /
superseded) may aggregate the five detail columns into [] plus a
detail_summary_json carrying counts, byte sizes and a detail_digest; a
no-benefit row (detail smaller than the summary) stays full byte-identical.
Unknown states or detail formats fail closed with SchemaIncompatible in read,
preview and apply; validation happens before any skip logic. Apply is one
transaction with a four-column CAS (state, operation_digest,
target_generation, detail_digest); drift rolls back whole and recovery
abandons the plan with a reason. The v18 -> v19 migration is additive, rolls
back on interruption, keeps old rows readable as full, and older binaries
refuse the newer user_version instead of misreading summaries. The
journal_compactions audit table grows linearly with explicit maintenance and
that growth must be disclosed next to any compaction numbers.

### 4. Validation & Error Matrix
Unknown format on pending or aggregated rows -> SchemaIncompatible (read,
preview, apply). Plan drift -> whole-plan rollback; recover marks abandoned,
detail untouched. Re-apply -> idempotent, zero rewrites. Interrupted stage ->
recover converges without touching details. Concurrent readers -> WAL snapshot
isolation across the rewrite.

### 5. Good/Base/Bad Cases
Good: 21 single-message rewrites aggregate to 0.381 percent of detail bytes
(0.876 percent including the audit row) with operation digests unchanged.
Base: a tiny terminal row stays full because aggregation would not save bytes.
Bad: deleting unresolved detail, silently skipping an unknown format, or
compacting automatically inside open_for_write.

### 6. Tests Required
journal_retention.rs pins preview numbers, the soak, killed-stage recovery,
drift abandonment, concurrent reads, v18 migration and rollback, unknown
format/state fail-closed on all three surfaces, no-benefit rows staying full,
unresolved-row protection, CAS idempotence, and byte-identical relocation
manifests.

### 7. Wrong vs Correct
Wrong: explain-or-skip unknown values, aggregate pending rows, or report a
compaction ratio without stating whether the audit row is included.
Correct: validate first, aggregate only terminal rows that benefit, keep the
CAS transaction atomic, and publish both ratio scopes.

## Semantic search evidence gate (B4)

Contract: when assembling semantic or hybrid candidates, a candidate whose
cosine similarity is below the evidence floor is discarded in the same query
pass that applies provider/time/repo/facet predicates, and before the bounded
top-k heap insert; filter-before-top-k therefore holds for the evidence gate
as well. The shipped default floor is 0.20, selected from the holdout and
frozen regression scans (zero-recall-loss interval intersection, one step of
margin). The floor is overridable through ASG_SEMANTIC_SIMILARITY_FLOOR;
non-numeric or non-finite values must fail as invalid_request rather than
silently falling back. A floor of 0.0 still excludes negative similarity.
The lexical FTS path is untouched by the floor. Known limitation, disclosed in
README and the retrieval report: the fuzzy-lexical hash model has a background
similarity near 0.40 on unrelated short text, so the default gate rejects zero
and negative evidence but cannot remove non-zero, non-semantic false hits;
that requires real embedding weights plus recalibration.

## Quality Check

- `catalog` remains authoritative; `fts` fully rebuildable from it.
- All multi-write operations are transactional and go through the outbox.
- Migrations are additive/non-destructive and gated on `user_version`.
- `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`
  + `cargo test --workspace` (SQLite adapter tests included).

---

**Language**: All documentation in **English**.
