# agentsessions-adapters-sqlite — Backend Guidelines

> SQLite + FTS5 storage adapter. Implements the catalog store and search index
> ports. This is the only crate that talks to `rusqlite`. The catalog is the
> source of truth; the FTS index is a rebuildable projection.

---

## Role in the architecture

`agentsessions-adapters-sqlite` is a **driven adapter**: it implements the
storage/search ports declared in `agentsessions-ports`. It owns the SQLite
schema, migrations, writer lease, durable outbox, generation tracking, source
membership/tombstones, and the FTS5 projection.

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

## Common mistakes

- Assuming `StableId::from_wire` restores Native/Reconstructed — it returns
  Unstable. Full identity lives only in `fts_ids.id_json`.
- Advancing generation outside the committing transaction.
- Adding an `ALTER`-free destructive "migration".

---

## Quality Check

- `catalog` remains authoritative; `fts` fully rebuildable from it.
- All multi-write operations are transactional and go through the outbox.
- Migrations are additive/non-destructive and gated on `user_version`.
- `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`
  + `cargo test --workspace` (SQLite adapter tests included).

---

**Language**: All documentation in **English**.
