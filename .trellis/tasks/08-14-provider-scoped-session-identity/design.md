# Provider-scoped Session identity — Design

## Boundaries

- Touches `domain/src/ids.rs` (canonical identity derivation), the composition point where provider `session_native_id` becomes `ses_v1_*` (application/store sync path), the SQLite schema migration (`adapters-sqlite`), and session grouping/relation/context reads.
- Does NOT implement Resume Metadata persistence (core task) or provider extraction (provider task); it consumes their typed observation.

## Identity derivation

Current: `StableId::native(IdKind::Session, sid)` — no provider/installation namespace.

Target: canonical Session identity = `hash(provider_id, installation_namespace, native_session_id)`.

- Add a `SessionIdentityNamespace` concept in `domain` (provider id + installation namespace string) consumed by the Session-id derivation for native IDs.
- The `ses_v1_` wire string format may need a v2 marker OR keep the wire string stable while namespacing the underlying derivation — choose the option that avoids re-keying all `ses_v1_*` when possible and keeps migration reversible.

## Migration

- Bump `SCHEMA_VERSION` 7 → 8 with a documented, deterministic migration.
- The migration re-keys Session identities derived from provider-native IDs using the discovered provider/installation namespace.
- Migration must not reinterpret any existing `ses_v1_*` value as a native ID; where a Session has no provider-native identity (reconstructed), its canonical ID is preserved.
- Cursor, grouping, relations, and search results must survive; add round-trip tests on a migrated fixture.

## Multi-ID fail-closed

- Provider observation carries `multi_session`. At composition, a `multi_session` Source keeps its catalog Session (searchable) but must not claim a resumable provider Session ID — the Resume feature returns `resume_available:false` for it.

## Compatibility

- `session_of`/grouping/context continue to use the canonical Session ID.
- Provider metadata on a search hit describes the deterministic selected Session.

## Tests

- Collision: same native ID, different provider → distinct IDs; different installation → distinct.
- No `ses_v1_`→native reverse derivation path exists.
- Migrated legacy catalog stays readable; grouping/cursors/relations/search preserved.
- Multi-ID Source fails closed for Resume availability.
