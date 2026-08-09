# Database Guidelines — agentsessions-adapters-sqlite

> SQLite + FTS5 patterns for the single-store backend.

---

## Overview

- Library: `rusqlite` against SQLite in WAL mode. There is no ORM; SQL is
  written directly and reviewed as such.
- The store is the single source of truth for two derived-but-authoritative
  roles: `catalog` (canonical entity payloads) and `fts` (a rebuildable search
  projection). `fts` can always be reconstructed from `catalog`.
- Schema version is gated by `PRAGMA user_version`; the current version is 7
  (v6 added `source_membership.document_id`, nullable; NULL = pre-v6 row; v7
  added the relational tables `message_placements`, `message_edges`,
  `source_placement_membership`, `source_relation_scans`, plus the
  relation-manifest columns on `index_batches`, with a forward additive
  v6→v7 migration that keeps pre-v7 catalogs readable). Opening a newer schema
  than the binary understands is an error, not a silent downgrade.

---

## Query Patterns

- **All writes go through the durable outbox.** Never write `catalog` / `fts` /
  `fts_ids` / `store_metadata` directly from an ad-hoc statement. Use the
  two-phase path: `begin_index_batch` writes the durable intent, then
  `commit_index_batch` applies + activates inside a single transaction. Catalog,
  FTS, and generation move together or not at all.
- **Wrap multi-row work in one transaction.** A crash must leave either the old
  consistent state or a side-effect-free `building` row that `recover_interrupted`
  converts to `aborted` on the next `open_for_write`.
- **Generation advances via CAS**, not blind increment. `verify_pending_in_tx`
  re-checks the active generation and the durable intent's digest/manifest
  before applying. A stale batch fails closed.
- **Source replacement is transaction-wide.** Before deriving tombstones, collect
  the complete incoming ID set for every source in the batch. A prior entity is
  deleted only when it is absent from that complete set and no membership from
  an unscanned source remains. Canonicalize deletes by wire ID so source order
  cannot change the manifest or produce duplicate tombstones.
- **Search projection is derived.** `searchable_text` extracts the indexable
  body; `fts_ids` maps the FTS wire id back to the full `StableId` JSON so
  identity survives a rebuild. Rebuild prefers `fts_ids.id_json`; only when it is
  absent does it fall back to `StableId::from_wire` (which yields `Unstable`).
- **Only Message-kind entities enter `fts`.** Session (`ses_v1_`) and document
  (`doc_v1_`) rows are catalog-only containers — indexing their text would
  double-count search hits. The `fts_ids` identity sidecar IS still written for
  every kind (rebuild depends on it for stability-tier fidelity). `batch_is_current`
  therefore skips the fts-text comparison for non-message ids.
- **Container entities ride the same SourceBatch.** The composition root puts
  the derived session/document rows into the same `entries` as the messages, so
  they share the transaction, the membership derivation, and the tombstone rules:
  when a source's scan no longer contains them (e.g. fingerprint changed → new
  content-addressed document id), the old rows retire unless another source
  still references them.

---

## Scenario: Data-root writer lease

### 1. Scope / Trigger

Use this contract whenever code acquires or inspects the single-writer lease for a data root. The OS-locked file handle is authoritative; PID and timestamp fields are diagnostic only.

### 2. Signatures

```rust
pub fn WriterLease::try_acquire(data_root: &Path) -> PortResult<WriterLease>;
pub fn WriterLease::read_record(&mut self) -> PortResult<String>;
```

With `fs4` 0.13, `FileExt::try_lock_exclusive()` returns `Result<bool, io::Error>`: only `Ok(true)` means the lease was acquired.

### 3. Contracts

- `Ok(true)` -> retain that exact `File` handle for the lifetime of the lease.
- `Ok(false)` or a recognized lock-contention error -> return retryable `PortError::WriterBusy`.
- Write, seek, and read the lease record through the retained handle. Do not reopen the lock path while the lease is held.
- WriterBusy messages must not expose the absolute data-root or lock-file path.
- Activation CAS runs while the writer lease is held; the diagnostic record never grants ownership.

### 4. Validation & Error Matrix

| Condition | Result |
|---|---|
| Lock returns `Ok(true)` | Construct `WriterLease` |
| Lock returns `Ok(false)` | `WriterBusy`, retryable |
| OS sharing/lock violation | `WriterBusy`, retryable |
| Other open/read/write failure | `Backend` without source-path disclosure |
| Stale expected generation | Reject CAS; keep current generation |

### 5. Good / Base / Bad Cases

- Good: a competing process is denied, the holder is killed, and the next process immediately acquires the OS-released lock.
- Base: one holder writes and reads all diagnostic fields through the same locked handle.
- Bad: treating `Ok(false)` as success, reopening `writer.lock` on Windows, stealing by PID timeout, or including the absolute path in an error.

### 6. Tests Required

- Unit test that the lease record is readable through the retained handle.
- Cross-process contention test asserting exactly one holder and a retryable busy result.
- Process-kill test asserting reacquisition without deleting the lock file.
- Regression test covering the `Ok(false)` branch or an equivalent deterministic contention path.
- Protocol test asserting WriterBusy output contains no local absolute path.

### 7. Wrong vs Correct

```rust
// Wrong: Ok(false) is not an error.
if file.try_lock_exclusive().is_err() {
    return Err(writer_busy());
}

// Correct: only Ok(true) acquires the lease.
if !matches!(file.try_lock_exclusive(), Ok(true)) {
    return Err(writer_busy());
}
```

---

## Scenario: Production process evidence

### 1. Scope / Trigger

Use this contract when proving writer lease, durable-intent recovery, or generation CAS behavior across process boundaries. Evidence must drive production `SqliteStore` APIs; a disposable spike cannot substitute for production-path evidence.

### 2. Signatures

```rust
pub fn SqliteStore::open_for_write(path: &str) -> PortResult<SqliteStore>;
pub fn SqliteStore::begin_index_batch(
    &self,
    upserts: &[(StableId, Vec<u8>, String)],
    deletes: &[StableId],
) -> PortResult<PendingIndexBatch>;
pub fn SqliteStore::commit_index_batch(
    &self,
    pending: &PendingIndexBatch,
    upserts: &[(StableId, Vec<u8>, String)],
    deletes: &[StableId],
) -> PortResult<()>;
```

The integration helper is non-user-facing and may expose only deterministic test operations such as hold, try-open, begin-intent, recover, and stale-commit.

### 3. Contracts

- Coordinate subprocesses with explicit readiness output, not sleep-only races.
- A killed holder must release the OS lock without deleting `writer.lock`.
- An exited process may leave a durable `building` intent, but no catalog/FTS/generation mutation.
- The next `open_for_write` converts interrupted `building` intents to `aborted`; repeating recovery is idempotent.
- A stale generation must fail the real transactional CAS before catalog/FTS apply.
- Process evidence records the exact OS/target; WSL2 evidence is not minimum-glibc or distribution certification.

### 4. Validation & Error Matrix

| Condition | Expected result |
|---|---|
| Holder owns lease; contender opens | `WriterBusy`, retryable, no absolute path |
| Holder is force-killed | Next process acquires without lock-file deletion |
| Process exits after durable intent | Reopen marks intent `aborted`; generation/catalog unchanged |
| Recovery runs twice | Same post-state; no new generation |
| Active generation differs from pending base | Commit fails closed; pending payload is not applied |

### 5. Good / Base / Bad Cases

- Good: child prints `READY`, parent confirms contention, kills child, then confirms immediate production-store reacquisition.
- Base: child creates a real durable intent and exits normally before apply; parent inspects pre-state and triggers reopen recovery.
- Bad: testing only `fs4` directly, deleting `writer.lock` to recover, racing via arbitrary sleeps, or claiming a WSL2 pass certifies glibc 2.31/macOS behavior.

### 6. Tests Required

- Cross-process production-store contention with path-free `WriterBusy`.
- Forced-holder termination and reacquisition without deleting the lock file.
- Durable-intent exit, automatic recovery, catalog/generation invariants, and idempotent re-recovery.
- Stale-generation rejection through `commit_index_batch`, asserting the catalog remains unchanged.
- Run the same integration tests on each platform that is claimed as locally or CI verified.

### 7. Wrong vs Correct

```rust
// Wrong: a disposable lock probe is presented as production-store evidence.
Command::new("lock-spike").status()?;

// Correct: the child opens the production adapter and emits a readiness marker.
let _store = SqliteStore::open_for_write(db)?;
println!("READY");
```

---

## Migrations

- Migrations are keyed on `PRAGMA user_version` and applied in order at open
  time. Each step is non-destructive where possible (`ALTER TABLE`, additive
  tables).
- Bumping the schema means: add the migration step, raise the target version,
  and keep older-version open paths working through the migration chain. Do not
  rewrite history or drop the user's catalog to "reset" a schema.
- The catalog is a persistent archive from the user's point of view even though
  it is technically derived — treat destructive schema changes as prohibited
  and rebuild the FTS projection instead.

---

## Naming Conventions

- Tables: `catalog`, `fts`, `fts_ids`, `index_batches`, `store_metadata`,
  `source_scans`, and (v7+) the relational tables `message_placements`,
  `message_edges`, `source_placement_membership`, `source_relation_scans`.
  Snake_case, singular-or-collective as already established.
- `store_metadata` is a single-row singleton holding `active_generation`.
- Source membership is a many-to-many via `source_scans` + composite key; an
  entity is only tombstoned when no remaining source references it.

---

## Common Mistakes

- **Writing FTS without the catalog write.** They must move in the same batch;
  an FTS row with no catalog backing is an orphan the rebuild will delete.
- **Incrementing generation outside CAS.** Always go through the verified
  commit path; a raw `UPDATE` on `store_metadata` breaks interrupted-recovery
  guarantees.
- **Deriving tombstones while iterating sources.** That makes the result depend
  on input order and can place a message moved to a later/new source in both the
  upsert and delete sets. Build the complete incoming set first.
- **Treating `catalog` as identity-authoritative.** The catalog primary key is
  the wire string and does **not** encode stability. Full identity lives only in
  `fts_ids.id_json`. Restoring Native/Reconstructed identity requires the
  provider, not the catalog alone.
- **Leaving a transaction open across fallible work** that can early-return via
  `?` before commit — scope the transaction so the Drop rolls back cleanly.
