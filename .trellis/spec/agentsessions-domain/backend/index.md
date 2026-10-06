# agentsessions-domain — Backend Guidelines

> The pure core. Canonical model, stable identity, and domain errors. No I/O,
> no other-crate dependencies except `serde`/`thiserror`.

---

## What this crate is

`agentsessions-domain` is the innermost hexagon layer. It owns:

- **Canonical model** — stable `Message` / `Session`, `SourceDocument`,
  contextual `MessagePlacement` / `MessageEdge`, and `SessionContextGraph`
  (`src/lib.rs`).
- **Stable identity** — `StableId` with three stability tiers
  (`Native` / `Reconstructed` / `Unstable`), `IdKind`, and the independent
  path-free `PlacementId` occurrence identity (`src/ids.rs`).
- **Branch selection** — `src/thread.rs`: placement-aware
  `select_mainline(&SessionContextGraph)` / `select_full(&SessionContextGraph)`.
  Mainline resolves contextual parent Message IDs, selects a real graph leaf,
  then walks root→leaf. Leaf selection is **message-level**: any message
  referenced as a parent by a candidate's edge is internal to the mainline, so
  none of its occurrences (including stale cross-document copies) may be the
  leaf. Ambiguous repeated parents return `AmbiguousGraph`;
  orphans stop honestly; cycles terminate; all-sidechain graphs fall back to
  all placements. Parent resolution always searches the complete Session
  placement set, including sidechain placements; filtering sidechains affects
  leaf candidacy, not whether a real parent can be followed.
  Deterministic ordering is one total order over a per-message timestamp key:
  `Missing` first, then `Invalid` (raw bytes, byte order), then `Valid` parsed
  UTC instants (fractional digits normalized, zero-padded to nanoseconds),
  then document id, ordinal, and placement id. `select_mainline` and
  `select_full` share this single comparator, so both are independent of the
  input placement permutation.
- **Domain errors** — `DomainError` (`src/error.rs`).

It depends on **nothing in this workspace**. Ports, application, adapters, and
providers all depend on it; it depends on none of them. Keep it that way.

---

## Pre-Development Checklist

- [ ] Does this change belong in the domain at all? If it touches SQLite, files,
      CLI args, or a provider format, it belongs in another crate. The domain
      only models *what a session/message is*, never *how it is stored or read*.
- [ ] Are you adding a field to stable `Message`? Only intrinsic
      identity/content belongs there: `id`, `role`, `text`, `timestamp`.
      Session, document, parent, ordinal, sidechain, and span belong to
      placement/edge relations.
- [ ] Are you touching `StableId`? Re-read the stability contract below — the
      catalog wire string does **not** encode stability.
- [ ] Context-graph invariants belong in `SessionContextGraph::validate`, returning
      `DomainError::InvariantViolation`, not a `panic!` or `assert!`.

---

## Key patterns

- **No I/O, no side effects.** Everything here is data + pure functions. No
  `std::fs`, no `rusqlite`, no `println!`.
- **Identity stability contract (critical).** `StableId::as_str` (the wire
  string used as the catalog key) does **not** encode the stability tier.
  `StableId::from_wire` can therefore only reconstruct an `Unstable` id. Full
  identity (kind + stability) survives only in the `fts_ids.id_json` sidecar in
  the SQLite adapter. Any code that needs the real tier must go through that
  sidecar or re-derive from the provider — never assume `from_wire` round-trips.
- **Native identity is preferred.** When a provider supplies a native id (e.g.
  Claude Code `uuid`, Codex `payload.id`), use `StableId::native_checked` →
  `Native` tier. Reject invalid input before normalization. The legacy `native`
  constructor remains for persisted identity compatibility, not new input.
  The v7 no-native fallback is an `Unstable`, path-free
  provider/variant/document/ordinal derivation owned by ingest, not Domain.
- **Validation is explicit.** `SessionContextGraph::validate` checks ID kinds,
  unique messages/documents/placements/edge children, derived Placement IDs,
  one ordinal per session/document, referenced stable entities, and span bounds.
  Orphan parent Message IDs remain valid; parent-placement ambiguity is a
  selector error rather than fabricated validation.

---

## Scenario: Provider identity and calendar boundaries

### 1. Scope / Trigger
Provider-controlled IDs and timestamps enter ingest and context ordering.

### 2. Signatures
`StableId::native_checked(IdKind, &str) -> DomainResult<StableId>`;
`thread::parse_instant(&str) -> Option<Instant>` (internal).

### 3. Contracts
Checked native suffixes are nonempty, at most 256 Unicode characters, and
contain no whitespace or controls. Accepted values are preserved verbatim;
Unicode normalization is not identity normalization. Historical `ses_v1`
derivation is unchanged. Calendar parsing validates month length and leap years;
arithmetic overflow and invalid clock/offset/fraction text are rejected.

### 4. Validation & Error Matrix
Invalid native suffix -> `DomainError::InvalidRequest` without echoing the ID.
Invalid sorting timestamp -> `None` from `parse_instant`; callers keep the original bytes in the `Invalid` key class, so ordering stays total and permutation-independent.

### 5. Good/Base/Bad Cases
Good: composed and decomposed Unicode IDs remain distinct. Base: a UUID keeps
its existing wire ID. Bad: `a\0b`, `a b`, or a 257-character suffix is rejected;
February 30 is not a valid instant.

### 6. Tests Required
Assert valid wire compatibility, non-colliding Unicode, overlength/control
rejection, leap-century rules, extreme years, and invalid offsets.

### 7. Wrong vs Correct
Wrong: trim/remove controls/truncate before constructing a new native ID.
Correct: validate the original value, fail the write on error, and leave
historical identities unchanged.

## Quality Check

Run the workspace gate and confirm green before proposing a commit:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Domain-specific review points:

- No new dependency on another workspace crate. If you reached for one, the
  logic is in the wrong layer.
- No `unwrap`/`expect`/`panic!` on any path reachable from valid input. Invalid
  input returns `DomainError`; only genuine invariant violations become
  `DomainError::InvariantViolation`.
- New model fields have a documented reason and at least one downstream
  consumer.
- Unit tests cover validation plus true-leaf selection, same-document parent
  disambiguation, ambiguity, orphan, cycle, sidechain, and deterministic full
  ordering.

---

**Language**: English.
