# Design: relational message placement and cross-source edges

## 1. Decision summary

A provider-native Message is one stable identity/content entity. Session,
document, source-local ordinal, sidechain state, byte span, and parentage belong
to a contextual `MessagePlacement` and `MessageEdge`. Application loads one
session-scoped graph through a typed port, Domain validates/selects it as pure
data, and SQLite persists all entity and relation changes in the existing
outbox/generation transaction.

This is the minimum model that represents all four verified real-data shapes
without selecting an arbitrary parent. It does not implement Thread/Branch
entities, new providers, or RFC governance promotion.

## 2. Domain model

`Message` retains only:

```text
Message { id, role, text, timestamp? }
```

`parent`, `seq`, `is_sidechain`, `span`, session, and document are removed from
its authoritative meaning. `Session` no longer owns one document or an embedded
message vector.

Add these pure domain types:

```text
PlacementId                 opaque `plc_v1_` BLAKE3 identifier
MessagePlacement {
  id, session_id, source_document_id, message_id,
  source_ordinal, is_sidechain, span?
}
MessageEdge {
  child_placement_id, parent_message_id, parent_native_id?,
  relation: reply|retry|fork|continuation|subagent|tool_result
}
SessionContextGraph { session_id, messages, placements, edges }
```

A placement ID is derived from session ID, document ID, Message ID, and
source-local ordinal. It is an occurrence identity, not a `StableId` entity and
never replaces the stable Message ID. The source path is not encoded. Identical
documents at several paths share a placement but retain separate
source-to-placement claims.

When a provider omits a native Message ID, the fallback Message ID is
`Unstable` and derived from provider ID, variant ID, content-addressed document
ID, and source-local ordinal. It never uses the source path. Re-ingesting a v6
path-derived fallback row therefore retires that legacy ID through normal
membership/tombstone rules and writes the new path-independent ID; no fake
`id_alias` is introduced.

Providers currently emit `reply` for explicit parent pointers. Other relation
variants are represented but never inferred without provider evidence. A root
has no edge row. `parent_native_id` preserves the provider fact for compatibility
projection; it is contextual and never participates in stable Message conflict
checking.

### Validation and selection

Domain validates ID kinds, placement/edge uniqueness, session scope, span
bounds, and edge child existence. Application then invokes pure
`select_mainline`/`select_full` over placements, not Message IDs.

For an edge's parent Message ID, resolution within the requested session is:

1. one matching parent placement: use it;
2. several matches but exactly one in the child's document: use that one;
3. no match: preserve current orphan semantics and stop the walk;
4. otherwise: return an explicit ambiguous-graph error; never choose an alias.

Cycles terminate through a visited placement set. `mainline` retains the honest
all-sidechain fallback. Ordering is deterministic across per-file ordinals:
provider timestamp, then document ID, source ordinal, and placement ID. Missing
timestamps sort before present timestamps; present timestamp strings compare by
their preserved UTF-8 bytes (both implemented providers emit ISO-8601 UTC).
The remaining keys provide a stable order without claiming unknown chronology.
`full` returns all placements in that order.

`mainline` first resolves contextual parents, then computes leaves inside the
candidate graph: non-sidechain placements are candidates, or all placements
when every placement is sidechain. A candidate is a leaf when no other
candidate resolves it as parent. The selected leaf is the maximum candidate
leaf under the deterministic order. A cycle has no leaf, so the selector falls
back to the maximum candidate and the visited set still guarantees termination.
It then walks contextual edges and returns root-to-leaf order.

## 3. Ports and Application flow

Add a backend-independent `ContextGraphStore` port, implemented by the same
SQLite store used for `CatalogStore`/`SearchIndex`:

```text
load_session_graph(session_id) -> typed SessionContextGraphRead
message_contexts(message_id)   -> typed placement/session candidates
context_stats()                -> aggregate placement/source-claim counts
```

`SessionContextGraphRead` contains Domain `Session`, `Message`,
`SourceDocument`, placement, and edge values rather than JSON payloads. No SQL,
table names, or storage JSON shape crosses the port. `App` binds
`CatalogStore + ContextGraphStore`; the composition root still injects one
`SqliteStore`. `Status` includes the aggregate context counts required by the
privacy-safe regression harness.

`handle_context` loads only the requested session graph, validates it, selects
placement IDs, applies budget/truncation, and assembles evidence from each
selected placement's exact document/span. It never reads `document`, `parent`,
`session`, or `span` aliases.

Context response items become
`{ id, placement_id, message_id, payload }`, where `id == message_id` is the
existing compatibility alias. `branch_leaf` remains a Message-ID compatibility
field and a new
`branch_leaf_placement_id` is authoritative. Evidence `occurrence_id` equals
the placement ID, so repeated stable messages do not collapse. TUI aligns
messages/evidence by placement ID.

Add an Application request/response for message-context candidates. TUI search
hits use it instead of `Show` + singular `session`.
`message_contexts(message_id)` returns candidates grouped by distinct session,
each carrying that session ID and its placement IDs. One distinct session opens
context even when it contains several placements; zero reports index-only;
several distinct sessions report an explicit ambiguity and do not pick one.
A migrated legacy Message with known source claims but missing relation
completeness returns `schema_incompatible`, not index-only. CLI context and MCP
`get_session_context` remain explicit session requests through the same ADT.

## 4. SQLite v7

Use an additive forward migration:

```sql
CREATE TABLE message_placements (
  placement_id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  document_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  source_ordinal INTEGER NOT NULL CHECK(source_ordinal >= 0),
  is_sidechain INTEGER NOT NULL CHECK(is_sidechain IN (0,1)),
  byte_start INTEGER,
  byte_end INTEGER,
  CHECK((byte_start IS NULL AND byte_end IS NULL) OR
        (byte_start >= 0 AND byte_end >= byte_start)),
  UNIQUE(session_id, document_id, source_ordinal)
);
CREATE TABLE message_edges (
  child_placement_id TEXT PRIMARY KEY,
  parent_message_id TEXT NOT NULL,
  parent_native_id TEXT,
  relation TEXT NOT NULL
);
CREATE TABLE source_placement_membership (
  source_path TEXT NOT NULL,
  placement_id TEXT NOT NULL,
  PRIMARY KEY(source_path, placement_id)
);
CREATE TABLE source_relation_scans (
  source_path TEXT PRIMARY KEY,
  relation_schema_version INTEGER NOT NULL CHECK(relation_schema_version >= 7)
);
```

Add indexes for session ordering, Message-to-placement lookup, document lookup,
and placement claim lookup. `source_relation_scans` distinguishes an honestly
empty v7 source from a legacy source that has not been re-ingested. Referential
integrity is enforced inside the same transaction as current entity membership;
no new SQLite behavior leaks upward.

Extend `index_batches` with default-empty relation upsert/delete manifests. The
operation digest hashes canonical entity changes and each source-scoped state
replacement: entity membership, placements, edges, placement claims, scan
completeness, and relation-completeness marker state. A source path plus an
empty replacement is still manifest data. Pending verification recomputes the
same manifest. A relation-only or completeness-only change therefore bypasses
no-op, is crash-safe, and advances the generation exactly once.

`SourceBatch` carries de-duplicated stable entity entries plus placements and
edges plus the complete `ParseReport` for one staged source. A zero-skipped
complete scan replaces entity/placement claims, deletes a placement/edge only
when no source still claims it, and writes its relation-complete marker. An
incomplete scan (`skipped > 0`) may upsert observed entities/placements/edges
but unions rather than replaces existing claims, derives no tombstones, and
atomically removes any prior relation-complete marker. Context therefore stays
disabled until a later complete scan. Stable entities claimed elsewhere remain.
One-batch and command-line-chunked sync have identical union semantics.

FTS rebuild continues to rebuild only `fts`/`fts_ids` from catalog; it neither
creates nor changes placements/edges. Relation counts and context results must
remain stable across rebuild.

## 5. Compatibility and migration

A v6 row lacks enough correspondence for safe backfill. Migration creates empty
relation tables, keeps every catalog/source-membership row, and never fabricates
placements or relation-complete markers. `get`/`list`/`show` remain readable.
Context is available only when every known source contributing to that session
has a v7 `source_relation_scans` marker. Otherwise it fails with canonical
`schema_incompatible` (exit 9) and a bounded "re-ingest required" action; it
must not return a partial graph or parse aliases. Re-ingesting a source writes
its placements, edges, claims, and completeness marker atomically. After all of
a session's contributing sources are complete, context becomes available.
The v6-to-v7 schema step itself runs in one explicit SQLite transaction; a
failure leaves both schema objects and `user_version` at v6.

Compatibility fields remain temporary projections, not authority:

- Session `messages` is regenerated from placements as a de-duplicated
  compatibility list. `documents`/`document` uses placement documents plus the
  existing source entity-membership `(source_path, session_id, document_id)`
  claim, so a zero-message source still attributes its document.
- Message `sessions`/`session` and `spans`/`span` are regenerated from placements;
  span entries include their exact document/placement.
- Message `parent` is emitted only when all current placements agree on one
  parent; roots emit null and divergent contexts emit null. No `parents[]` is
  introduced.
- Message `parent_native_id` follows the same all-placements-agree rule using
  edge provenance. Message `is_sidechain` is emitted as a boolean only when all
  current placements agree and as null when they diverge.
- Existing stored aliases remain readable until re-ingest. Application context
  never consumes them. Alias retirement is deferred to a separately governed
  contract change.

During mixed v6/v7 operation, affected payloads preserve existing legacy
compatibility facts and union newly observed v7 facts; they do not subtract
aliases while any known contributing source is relation-incomplete. Once every
known contributor is relation-complete, the store regenerates the exact
subtractive projection from placements, edges, and source entity membership.

Stable Message conflict checking compares only ID/content fields. Different
role, text, or timestamp under one stable Message ID still fails loudly and
atomically; contextual differences no longer participate in that conflict.

## 6. Parse-loss and regression evidence

`StagedBatch` retains the complete `ParseReport`; CLI sync/ingest reports emitted
placements and recoverably skipped records honestly. `context_stats` exposes
aggregate persisted placement and source-placement claim counts. A skipped
record is never hidden by a successful envelope and never leaves a source
relation-complete.

`INV-NO-PARSE-LOSS` is strengthened, not relaxed: provider-emitted source
records must equal persisted source-placement claims, and skipped records must
be zero. Stable Message count remains a separate de-duplicated census.
Synthetic regression fixtures must combine all four shapes, including one
stable child placed under different parents in two resumed/forked sessions.
They also assert exact evidence document/span, placement-distinct response
identity, relation-only generation changes, no-op, tombstone subtraction,
atomic conflict failure, rebuild stability, and report privacy.

## 7. Rollback and evidence discipline

The migration is forward-only and non-destructive. Before release, rollback is
code rollback plus disposal of test catalogs. A user catalog opened by v7 is not
downgraded; older binaries must reject the newer schema. Recovery continues to
abort side-effect-free `building` intents.

Real provider transcripts remain read-only and local. Only aggregate reports
under gitignored `evidence-output/` may be produced. Providers remain
Experimental and governance documents remain Draft until separate owner action.
A failed or partial real-data run is recorded as failed without hiding its exit
code or unevaluated invariants.
