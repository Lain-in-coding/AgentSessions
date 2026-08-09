# Correctness and Security Fix Design

## Boundary

This child fixes confirmed defects in SQLite source reconciliation, writer-lease acquisition, and retryable contention classification. It does not change the public robot schema, generation model, provider parsing, or source identity format.

## Source-batch reconciliation

`commit_source_batches_if_changed` must derive one transaction-wide final membership view before deriving tombstones:

1. Reject duplicate source paths and duplicate IDs within a source.
2. Merge all incoming upserts by wire ID, rejecting conflicting payload/text projections.
3. Collect the complete incoming ID set across every source before inspecting prior memberships.
4. For every scanned source, compare prior membership with that source's new membership.
5. Delete an entity only when it is absent from the complete incoming set and no prior membership outside the scanned-source replacement set will remain.
6. Canonicalize/deduplicate deletes before building the outbox manifest.

This makes equivalent permutations produce the same manifest and permits an entity to move atomically from an existing source to a newly introduced source.

## Writer lease and privacy

With `fs4 0.13`, only `Ok(true)` from `try_lock_exclusive()` acquires the lease. `Ok(false)` and recognized sharing/lock contention are retryable `PortError::WriterBusy`. The message is a stable generic diagnostic and must not include `data_root`, `writer.lock`, or another absolute path.

All lease-record I/O continues through the retained locked handle on Windows.

## SQLite contention mapping

Translate `rusqlite::Error::SqliteFailure` with primary code `DatabaseBusy` or `DatabaseLocked` to retryable `PortError::WriterBusy`. Other SQLite failures remain `Backend`. The translation belongs at the adapter boundary and must be used by SQLite operations rather than leaking raw backend text upward.

No implicit retry loop or new busy timeout is introduced in this child: callers receive the existing retryable category and retain policy control.

## Compatibility and invariants

- No schema migration.
- No protocol-envelope shape or exit-code change.
- Catalog, FTS, membership, generation, and activated outbox row remain one transactional commit.
- A content-level no-op does not advance generation.
- Conflicting projections and ambiguous batches continue to fail closed.

## Validation

Targeted tests cover source permutations, cross-source moves to a new destination, shared memberships, `Ok(false)` contention behavior, path-redacted errors, and SQLite BUSY/LOCKED classification. Then run the full workspace quality gate and `cargo deny check`.

## Rollback

The source reconciliation, lease handling, and SQLite error mapping are separable edits. If a regression is found, revert only the affected logical edit while retaining its failing regression test for diagnosis.
