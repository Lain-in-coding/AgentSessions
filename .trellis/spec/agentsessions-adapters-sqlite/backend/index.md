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
- Filtered FTS search — uses one prepared query with `fts MATCH` plus provider
  and normalized timestamp predicates before `LIMIT`; retains the pinned BM25
  score/StableId order. Empty filters keep the legacy SQL path. SQLite FTS5
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
- **Multi-Session Sources fail closed**: a Source declaring more than one
  native Session is not resumable (ambiguous), though still searchable.
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
  non-system Message as representative) and remains discoverable before
  Application-level system filtering.

### Known RFC-0001 §5.1 identity debt (follow-up, not blocking Resume)

`installation_namespace` derives from absolute path and lacks: a persisted
`installation_namespaces` registry, an `id_alias(old_id, new_id)` table with
TTL, Windows path-case normalization, and resume claims keyed by
`(namespace_registry_id, session_id)`. Relocation does not preserve Session
identity. The domain layer (`StableId::native_session_scoped`,
`SessionIdentityNamespace`) is correct; the gap is composition-root only.

---

## Quality Check

- `catalog` remains authoritative; `fts` fully rebuildable from it.
- All multi-write operations are transactional and go through the outbox.
- Migrations are additive/non-destructive and gated on `user_version`.
- `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`
  + `cargo test --workspace` (SQLite adapter tests included).

---

**Language**: All documentation in **English**.
