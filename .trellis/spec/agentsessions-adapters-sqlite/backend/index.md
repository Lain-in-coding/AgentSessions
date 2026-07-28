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
| Session | `messages` (append-order), `documents` (sorted) | must match byte-for-byte |
| Message | `sessions`, `spans` (keyed by contributing document) | must match byte-for-byte |

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
- Anything outside those fields differing under one id is a real inconsistency
  and still fails with `conflicting projections across sources`.

**Known limit:** `parent` is per-source too (a fork can re-parent a copied
message), and it is *not* unioned — mainline selection walks a single parent
chain, so multiple parents make "the mainline" undefined. Fixing that needs
the RFC-0001 §3.2 shape (edges as their own relation), not another array here.
See `docs/evidence/integration-beta/real-data-regression.md`.

## Common mistakes

- Assuming `StableId::from_wire` restores Native/Reconstructed — it returns
  Unstable. Full identity lives only in `fts_ids.id_json`.
- Advancing generation outside the committing transaction.
- Adding an `ALTER`-free destructive "migration".
- Treating a per-source fact (session, span, parent) as intrinsic to a message.
  Message *identity* is shared across files; its *position* is not.

---

## Quality Check

- `catalog` remains authoritative; `fts` fully rebuildable from it.
- All multi-write operations are transactional and go through the outbox.
- Migrations are additive/non-destructive and gated on `user_version`.
- `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`
  + `cargo test --workspace` (SQLite adapter tests included).

---

**Language**: All documentation in **English**.
