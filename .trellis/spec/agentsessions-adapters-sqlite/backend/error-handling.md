# Error Handling — agentsessions-adapters-sqlite

> How the SQLite adapter surfaces failures through the port boundary.

---

## Overview

The adapter implements port traits, so it must speak the ports layer's error
vocabulary: it returns `PortError`, never a raw `rusqlite::Error` and never a
`DomainError`. Backend specifics (SQLite error codes, busy timeouts, IO
failures) are translated at the crate boundary into the appropriate `PortError`
variant so that no layer above ever depends on SQLite.

---

## Error Types

Map raw failures onto `PortError` (see the ports crate) roughly as:

- Lock/lease not acquired → `PortError::WriterBusy` (retryable at the CLI).
- SQLite primary result code `DatabaseBusy` or `DatabaseLocked` →
  `PortError::WriterBusy` with a stable path-free message; do not add an implicit
  retry loop at this boundary.
- Source file changed under a snapshot → `PortError::SnapshotChanged`.
- Source read/IO failure → `PortError::SourceIo`.
- Schema newer than the binary understands → `PortError::SchemaIncompatible`.
- Requested row absent where absence is an error → `PortError::NotFound`.
- Everything else genuinely backend-side → `PortError::Backend`.

---

## Error Handling Patterns

- Translate at the boundary: convert `rusqlite::Error` into a `PortError` inside
  the adapter method, attaching a safe message. Do not let `rusqlite::Error`
  escape the crate.
- Prefer `map_err` at the call site over a blanket `From` when the same raw
  error means different things in different methods (e.g. a missing row is
  `NotFound` in a getter but a bug in a writer).
- Fail closed on consistency checks. If `verify_pending_in_tx` finds a
  generation/intent mismatch, return an error and let the transaction roll
  back; never patch the state to make it "work".
- Writer contention is expected, not exceptional — surface it as `WriterBusy`
  so the CLI can present it as retryable rather than a hard failure.

---

## API Error Responses

The adapter does not format wire output. It returns `PortError`; the CLI maps
`PortError` → canonical code → exit code. Keep messages free of absolute paths
and secrets — they travel up into the CLI error envelope.

---

## Right / Wrong

Translating a raw `rusqlite::Error` at the crate boundary:

```rust
// Wrong — leaks rusqlite upward; every caller now needs a SQLite dependency
// to interpret the failure, and the retryable signal is lost.
fn get(&self, id: &StableId) -> Result<Vec<u8>, rusqlite::Error> {
    let conn = self.conn.lock().unwrap();
    conn.query_row(/* ... */)
}

// Right — return PortError; classify busy/locked as WriterBusy so the CLI
// can surface it as retryable. A missing row is a hard error in this getter.
fn get(&self, id: &StableId) -> PortResult<Vec<u8>> {
    let conn = self.conn.lock().map_err(|e| PortError::Backend(e.to_string()))?;
    conn.query_row(/* ... */).map_err(|e| match e {
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::DatabaseBusy =>
        {
            PortError::WriterBusy("catalog is locked by another writer".into())
        }
        rusqlite::Error::QueryReturnedNoRows => {
            PortError::NotFound("entity not found".into())
        }
        other => PortError::Backend(other.to_string()),
    })
}
```

Failing closed on a consistency check:

```rust
// Wrong — patch the state so the batch "succeeds" and return Ok.
if pending.generation != active { set_generation(pending.generation)?; }

// Right — a generation/intent mismatch is an error; roll back, do not patch.
if pending.generation != active {
    return Err(PortError::Backend("generation CAS mismatch".into()));
}
```

---

## Common Mistakes

- **Leaking `rusqlite::Error` upward.** Callers above must not need a SQLite
  dependency to interpret a failure.
- **Swallowing a busy/locked error as `Backend`.** That loses the retryable
  signal; classify it as `WriterBusy`.
- **Returning `Ok` after a partial write.** If the batch did not fully apply +
  activate, it is an error and the transaction must roll back.
- **Embedding the source path or payload in the error message.** Reference the
  entity by wire id, not by its content.
