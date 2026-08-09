# Correctness and Security Implementation Plan

## Source reconciliation

- [x] Add a failing regression for moving an entity from source A to a newly introduced source B in one call.
- [x] Add permutation coverage proving equivalent source orders yield identical catalog, membership, and generation outcomes.
- [x] Refactor tombstone derivation to use the complete transaction-wide incoming ID set and final membership view.
- [x] Preserve conflict rejection, durable outbox, generation CAS, atomic membership replacement, and no-op behavior.

## Writer lease and privacy

- [x] Change production lease acquisition so only `fs4` `Ok(true)` succeeds.
- [x] Return path-redacted WriterBusy diagnostics for in-process, cross-process, open, lock, and write contention.
- [x] Add focused tests for retryable contention and absence of local paths.

## SQLite contention

- [x] Add a centralized adapter-boundary mapping for SQLite BUSY and LOCKED to WriterBusy.
- [x] Apply the mapping to fallible SQLite operations without adding retries or changing transaction scope.
- [x] Add deterministic unit coverage for BUSY, LOCKED, and a non-contention SQLite failure.

## Validation

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p agentsessions-adapters-sqlite
cargo test -p agentsessions-cli
cargo test --workspace
cargo deny check
```

## Review gates

- Equivalent source permutations produce identical final state.
- A moved entity is never present in both upsert and delete sets.
- WriterBusy output contains no absolute local path.
- Only actual SQLite BUSY/LOCKED conditions are reclassified as retryable.
- The full quality gate is green before this child is considered complete.
