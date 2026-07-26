# Canonical Source/Session/Span foundation and storage migration

Child 1 of `07-24-advance-integration-beta`. Owns the canonical entity
foundation that all later children (fixtures, shared Application ADT,
MCP/TUI) build on.

## Goal

Persist independent SourceDocument and Session entities and per-message
EvidenceSpan provenance for the existing file-backed Claude Code and Codex
providers, with a gated storage migration, so retrieval can answer "which
session, which document, which bytes" instead of only "which message".

## Confirmed Facts (from current code, 2026-07-26)

- `agentsessions-domain` already models `Session { id, document_id, messages }`
  and `Message { id, parent, role, text, seq, timestamp, is_sidechain }` with
  `Session::validate` invariants, but the CLI persists only per-message
  canonical JSON payloads into `catalog`; no `ses_v1_`/`doc_v1_` row exists.
- `IdKind::Source/Document/Session/Message` and the three stability tiers
  already exist in `ids.rs`; complete identity (kind+stability) is persisted
  only in the `fts_ids.id_json` sidecar, and `StableId::from_wire` degrades to
  `Unstable` (documented catalog-key constraint).
- Storage is at schema v5 with `PRAGMA user_version` gating, durable outbox
  (`index_batches`), `store_metadata.active_generation`, `fts_ids`,
  `source_membership` (composite key), and `source_scans`.
- `SourceSnapshot` in ports is file-oriented (`path/len/mtime_ms/fingerprint`).
  The sqlite-source-identity spike shows row-level sources need a different
  unit contract; per parent decision this child only keeps the contract
  compatible, it does NOT productize SQLite row sources.
- Providers emit `MessageEvent` (native_id/parent_native_id/role/text/
  timestamp/is_sidechain) but no byte-offset provenance; `ParseReport` counts
  committed/skipped without span information.

## Requirements

### R1: Canonical entity persistence

- Persist SourceDocument and Session as first-class catalog entities with
  their own `doc_v1_`/`ses_v1_` StableIds, derived per RFC-0001 rules:
  native id when the provider guarantees one (Claude Code `sessionId`,
  Codex rollout id), otherwise Reconstructed from intrinsic facts. Never
  derive from absolute paths or mtimes.
- A Session payload must reference its document and its member message ids in
  order; a SourceDocument payload must carry provider id, variant id, and the
  verification fingerprint of the snapshot it was ingested from.
- Message payloads must gain a session reference so `show` can walk
  message -> session -> document without heuristics.

### R2: EvidenceSpan provenance

- Every committed message records the byte span (start/end offsets into the
  verified source snapshot) of the source record it was normalized from.
- Spans are provider-reported at parse time (extend `MessageEvent`), not
  recomputed after the fact; sink/storage carries them losslessly.
- Span info survives ingestion, storage migration, retrieval (`show`), and
  `index rebuild`.

### R3: Storage migration

- Schema bump v5 -> v6 gated by `PRAGMA user_version`, following the existing
  migration pattern (idempotent, single transaction, no data invention).
- Existing v5 stores migrate without loss: existing message rows remain
  retrievable; newly required entities/spans for legacy rows may be absent
  until re-ingest, and their absence must be explicit (not fabricated).
- `index rebuild` remains correct: identity fidelity via `fts_ids` first,
  `from_wire` fallback, per the documented catalog-key constraint.

### R4: Contract compatibility (SQLite row sources, design-only)

- The source-unit contract introduced or touched by this child must not
  hard-code "one file = one document". Where the contract names a unit,
  it must be expressible for a future row-level source (spike evidence),
  without implementing any SQLite provider now.

### R5: Boundaries

- Providers stay strictly read-only over sources; local-first; no network.
- No new provider, no MCP/TUI/protocol surface changes beyond what `show`
  needs to expose the new entities.
- Quality gates green: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.

## Acceptance Criteria

- [ ] Ingesting a real-format Claude Code transcript and a Codex rollout
      produces SourceDocument, Session, and Message entities; `show` on each
      id kind returns a structured entity with correct cross-references.
- [ ] Every committed message carries an evidence span that, applied to the
      verified source bytes, yields exactly the source record it came from
      (round-trip test per provider).
- [ ] Opening a v5 store with the new binary migrates to v6; pre-existing
      messages remain listable/searchable/gettable; a migration test covers
      populated-store upgrade, and legacy rows expose "no span/session yet"
      explicitly rather than fabricated values.
- [ ] `index rebuild` on a migrated, mixed (legacy + new) store preserves
      identity stability tiers per the documented constraint and keeps spans
      intact.
- [ ] `Session::validate` (extended as needed) rejects cross-kind references
      introduced by the new payload shapes; property of seq monotonicity is
      preserved.
- [ ] No source file is opened for write at any point (existing read-only
      guarantees still hold; regression covered).
- [ ] All three quality gates pass on Windows locally; CI matrix unchanged.

## Out of Scope

- SQLite row-source discovery/verification (design compatibility only, R4).
- Golden fixtures and property/fuzz hardening (child 2).
- Thread/Branch selection semantics and response budget (child 3 ADT); this
  child persists the raw threading facts (parent pointers, sidechain flags)
  it already has, it does not add branch-selection logic.
- Any remote, publishing, signing, or release action.

## Dependencies / Ordering

- First implementation child of `07-24-advance-integration-beta`; no upstream
  child dependency. Children 2+ must consume this child's entity contracts
  and must not redefine them.
