# Fix correctness and security blockers

## Goal

Fix confirmed data-consistency and privacy defects before expanding the integration surface.

## Requirements

- Make multi-source tombstone derivation independent of source input order.
- Add regression coverage for cross-source moves, a new destination source, and source permutation.
- Remove absolute local paths from WriterBusy protocol messages.
- Treat `fs4 0.13` `try_lock_exclusive()` `Ok(false)` as lease contention, not successful acquisition.
- Map SQLite busy/locked errors to retryable WriterBusy behavior where applicable.
- Preserve durable outbox, generation, source-membership, and no-op semantics.
- Correct the existing formatting failure.

## Acceptance Criteria

- [x] Source batches produce identical results for every permutation of equivalent source input.
- [x] Moving an entity from source A to newly introduced source B succeeds atomically without upsert/delete overlap.
- [x] WriterBusy output contains no absolute data-root or database path.
- [x] Lease acquisition rejects both `Ok(false)` and lock errors from `fs4` contention paths.
- [x] SQLite busy/locked errors use the documented retryable category.
- [x] fmt, clippy, workspace tests, and targeted regression tests pass.
