# agentsessions-ports — Backend Guidelines

> Hexagonal port layer. Defines the trait boundaries between the application
> core and concrete adapters. No I/O, no concrete backend, no `std::fs`.

---

## Role in the architecture

`agentsessions-ports` is the **interface layer** of the hexagon. It declares the
traits (ports) that the application core depends on and that adapters
(`adapters-sqlite`, `provider-claude`, `provider-codex`) implement. It is the
narrowest crate on purpose: it names capabilities, it does not perform them.

Depends only on `agentsessions-domain`. Never depends on any adapter crate,
`rusqlite`, `serde_json`, or the filesystem.

---

## Pre-Development Checklist

- [ ] Am I adding a *capability the core needs*, or leaking an adapter detail?
      Ports describe what the core requires, not how SQLite/FS provides it.
- [ ] Does the new method belong on an existing port, or is it a genuinely new
      seam? Prefer extending an existing trait when there is one caller.
- [ ] Does the signature use domain types (`StableId`, `Session`, `Message`)
      and `PortResult<T>` — never `rusqlite::Error` or raw `io::Error`?
- [ ] If I add a `PortError` variant, does it map to a canonical protocol code
      in the CLI's `protocol.rs` `From<PortError>`? Adding a variant without
      updating that mapping breaks the error contract.
- [ ] Structured event params (e.g. `MessageEvent`) — should this be a struct
      rather than a positional tuple, so fields can be added without breaking
      every call site?

---

## Key types

- `PortError` / `PortResult<T>` — the error currency crossing every port.
  Variants: `Backend`, `SourceIo`, `SchemaIncompatible`, `NotFound`,
  `SnapshotChanged`, `WriterBusy`. Each maps to a canonical code downstream.
- `CanonicalEventSink` + `MessageEvent<'a>` — structured sink for provider
  parse output. `MessageEvent` is a struct (native_id / parent_native_id /
  role / text / timestamp / is_sidechain / span) precisely so new fields don't
  break callers. `span` is `(start, end)` byte offsets **into the verified
  snapshot bytes**, end exclusive, newline excluded; the unit is deliberately
  "snapshot bytes" (not "file bytes") so a future row-level source (SQLite
  provider) can satisfy the same contract by making the extracted row payload
  the snapshot. `None` means the provider cannot attribute a contiguous span —
  never fabricate one. `MessageEvent.session` optionally carries a
  `ProviderSessionIdentity { source_key, observation }` per message. The
  source-local key distinguishes native-less sessions and is never exposed
  as a provider-native ID. Report-level session fields are compatibility
  observations for messages without an explicit session.
- `ProviderAdapter` — `probe(bytes) -> Probe` + `parse(bytes, sink)`. The
  probe/select contract lives here; the selection *policy* lives in the
  application core.
- `SearchQuery<'a>` — literal query text plus normalized `SearchFilters`.
  Providers are validated canonical values from the capability matrix; time bounds use a normalized
  `(unix_seconds, nanosecond)` instant. Adapters must apply filters before
  storage `LIMIT`, not post-filter paginated hits.
- `SearchHit` — ranked stable identity plus additive text/session/guidance
  projection fields. `occurrences` is 1 on the default (non-grouped) path and
  the per-session collapse count on the grouped path. Guidance is assembled by
  Application; storage leaves `why_matched` and `suggested_next_commands`
  empty.
- `ContextGraphStore` — typed contextual reads:
  `load_session_graph(&SessionId)`, `message_contexts(&MessageId)`, and
  `context_stats()`. It returns Domain graphs, distinct-session candidates, and
  aggregate counts; no SQL row or JSON payload shape crosses the port.

## Scenario: Context graph reads

### 1. Scope / Trigger

Use this port when Application needs Message placement, edge, or reverse
session-membership facts that cannot be represented by `CatalogStore::get`.

### 2. Signatures

```rust
fn load_session_graph(&self, session_id: &StableId)
    -> PortResult<SessionContextGraph>;
fn message_contexts(&self, message_id: &StableId)
    -> PortResult<Vec<MessageContextCandidate>>;
fn context_stats(&self) -> PortResult<ContextStats>;
```

### 3. Contracts

- `message_contexts` groups by distinct Session. Several placements in one
  Session produce one candidate with several sorted `placement_ids`.
- `ContextStats` exposes only aggregate placement and source-claim counts.
- Implement `ContextGraphStore for &T` whenever a concrete store implements it,
  so one shared store reference can fill Application port slots.

### 4. Validation & Error Matrix

- Unknown Session → `PortError::NotFound`.
- Migrated legacy relation state that is not complete →
  `PortError::SchemaIncompatible`.
- Backend corruption/query failure → `PortError::Backend`.
- Ambiguous graph topology is Domain selection output, not a SQLite error.

### 5. Good/Base/Bad Cases

- Good: one Message has two placements in one Session → one candidate.
- Base: one complete Session graph → typed graph with no compatibility parsing.
- Bad: return raw SQL rows or JSON blobs → port-layer violation.

### 6. Tests Required

- `&T` blanket forwarding.
- Candidate grouping by Session.
- Not-found and schema-incompatible classification.
- Aggregate counts without source paths or payload content.

### 7. Wrong vs Correct

```rust
// Wrong: leaks persistence shape.
fn load_edges(&self, session: &str) -> Vec<rusqlite::Row<'_>>;

// Correct: backend-independent Domain values.
fn load_session_graph(&self, session: &StableId)
    -> PortResult<SessionContextGraph>;
```

---

## Scenario: Shared retrieval and source contracts

### 1. Scope / Trigger
Search ports and multi-session source events cross application/adapter boundaries.

### 2. Signatures
`SemanticIndex::is_ready() -> PortResult<bool>`;
`semantic_model_id() -> PortResult<Option<String>>`;
`query_semantic_filtered(&[f32], usize, &SearchFilters, &SearchFacets, bool)`
returns `PortResult<Vec<SearchHit>>`. `SearchIndex::query_with_policy` accepts
the same facets and `include_system` visibility policy for lexical search.

### 3. Contracts
Apply filters/facets/visibility before top-k. Only `Ok(false)` readiness permits
explicit lexical fallback; errors retain their port classification. Reject
non-finite vectors/scores. `SearchProvider` is a private canonical wrapper:
accepted IDs come from implemented searchable capability rows, with the
historical `claude` alias. Source fingerprints are opaque: file BLAKE3 hex or
`sqlite:<logical-backup-BLAKE3>`; source paths remain locators.

### 4. Validation & Error Matrix
Unknown/deferred provider -> boundary invalid request. Missing semantic index
-> explicit fallback. Backend/schema/busy failures -> errors, never absence.

### 5. Good/Base/Bad Cases
Good: two sessions in one source retain different observations. Base: absent
event session uses the report observation. Bad: treating a source-local key
as a native resume ID, or filtering an already truncated semantic result.

### 6. Tests Required
Assert registry/runtime/schema parity, trait forwarding, readiness error
propagation, metadata/facet visibility, and staged session preservation.

### 7. Wrong vs Correct
Wrong: `is_ready().unwrap_or(false)` or a second hard-coded provider list.
Correct: propagate readiness errors and derive accepted provider values from
the capability registry.

## Quality Check

- No `use rusqlite`, no `use std::fs`, no `serde_json` in this crate.
- Every port method returns `PortResult<_>`; concrete adapter errors are
  wrapped into `PortError` by the adapter, never surfaced raw.
- New `PortError` variants have a matching arm in the CLI protocol mapping.
- Traits stay object-safe where the composition root needs `&dyn Trait`
  (e.g. `ProviderAdapter` is used as `&dyn ProviderAdapter` in the registry).
- `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`.

---

**Language**: All documentation in **English**.
