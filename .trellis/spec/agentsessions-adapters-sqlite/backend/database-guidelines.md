# Database Guidelines — agentsessions-adapters-sqlite

> SQLite + FTS5 patterns for the single-store backend.

---

## Overview

- Library: `rusqlite` against SQLite in WAL mode. There is no ORM; SQL is
  written directly and reviewed as such.
- The store is the single source of truth for two derived-but-authoritative
  roles: `catalog` (canonical entity payloads) and `fts` (a rebuildable search
  projection). `fts` can always be reconstructed from `catalog`.
- Schema version is gated by `PRAGMA user_version`; the current version is 5.
  Opening a newer schema than the binary understands is an error, not a silent
  downgrade.

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
  `source_scans`. Snake_case, singular-or-collective as already established.
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
