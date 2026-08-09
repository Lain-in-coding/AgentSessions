# Relational storage and selector audit

## Verified findings

1. The current schema is v6. `catalog` stores one payload per stable entity,
   while `source_membership(source_path, message_id, document_id)` records only
   entity claims. It cannot represent session-scoped occurrences or edges
   (`crates/agentsessions-adapters-sqlite/src/lib.rs:446-574`).
2. `SourceBatch` contains only entity entries. Commit first merges duplicate
   entity payloads, then derives tombstones from source membership
   (`crates/agentsessions-adapters-sqlite/src/lib.rs:356-379,667-812`).
3. Message merging excludes only `session(s)` and `span(s)` from conflict
   comparison. Divergent `parent` or `parent_native_id` therefore correctly
   fails today; adding `parents[]` would remove the error but not define a
   branch (`crates/agentsessions-adapters-sqlite/src/lib.rs:95-200`).
4. Session merging de-duplicates members by Message ID and preserves append
   order. It therefore loses which source/document occurrence supplied each
   member and cannot represent repeated placements
   (`crates/agentsessions-adapters-sqlite/src/lib.rs:202-275`).
5. Entity payloads, FTS, membership, generation activation, and outbox state are
   committed in one transaction. Any placement/edge tables must join that same
   transaction (`crates/agentsessions-adapters-sqlite/src/lib.rs:1047-1141`).
6. The durable intent and no-op checks currently hash/compare only entity
   upserts/deletes and source membership. A relation-only parent/span/ordinal
   change would be invisible unless relation manifests are added to both paths
   (`crates/agentsessions-adapters-sqlite/src/lib.rs:282-330,585-657,787-812,
   971-1045`).
7. Tombstone logic protects an entity while any unscanned source still claims
   it, but merged JSON aliases are not a subtractive source of truth. Dedicated
   source-to-placement claims are required so re-scanning or removing one
   source can delete only its unshared occurrences and regenerate aliases
   (`crates/agentsessions-adapters-sqlite/src/lib.rs:764-888`).
8. `rebuild_index` rebuilds only `fts`/`fts_ids` from catalog and leaves source
   membership intact. Relational context data must likewise remain untouched by
   an FTS rebuild (`crates/agentsessions-adapters-sqlite/src/lib.rs:1143-1245`).
9. Domain `Message` currently owns contextual `parent`, `seq`, `is_sidechain`,
   and `span`; `Session` owns one document and embedded messages
   (`crates/agentsessions-domain/src/lib.rs:37-131`). Stable Message should keep
   only ID, role, text, and provider timestamp.
10. `select_mainline` maps by Message ID. Multiple placements of one stable
    Message in a loaded context would overwrite in that map even if the parent
    field were moved elsewhere. Selection must walk placement identity, not
    Message identity (`crates/agentsessions-domain/src/thread.rs:43-72`).
11. Both providers reset `seq` to zero per parsed source document. A session
    spanning documents therefore needs a deterministic cross-document order;
    member-array append order is not an authoritative ordinal
    (`crates/agentsessions-provider-claude/src/lib.rs:196-270`;
    `crates/agentsessions-provider-codex/src/lib.rs:217-291`).
12. Provider `ParseReport.skipped` and diagnostics are discarded when staging
    returns only messages and session ID; CLI reports `skipped: 0`. The no-loss
    gate can therefore pass despite recoverable record loss
    (`crates/agentsessions-application/src/lib.rs:152-186`;
    `crates/agentsessions-cli/src/main.rs:883-981`).
13. The current real-data invariant compares emitted source occurrences to
    distinct stable Message entities. Legal cross-session de-duplication will
    make this unequal after the relational fix. It must compare emitted records
    to persisted source-placement claims and separately require zero skipped
    records (`scripts/evidence/real_data_regression.py:327-395`).

## Migration consequence

A v6 catalog cannot be losslessly backfilled: its session member list is
Message-ID de-duplicated, its parent is one compatibility alias, and it no
longer contains the source/session/ordinal/parent correspondence. The additive
migration must create empty relational tables and preserve old catalog rows.
`show`/`get`/`list` remain readable; context lookup for a legacy session with no
relations must fail explicitly with a re-ingest instruction rather than infer
from aliases. The first complete re-ingest populates relations and advances one
generation because the authoritative content has changed.
