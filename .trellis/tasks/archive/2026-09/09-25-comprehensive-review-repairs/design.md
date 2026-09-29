# Repair Design

## Execution Order

1. **Pure boundaries**: Unicode redaction, checked time/calendar arithmetic, checked native IDs, hook budget estimator, Grok integer conversion.
2. **SQLite lifecycle**: true read-only opens and explicit leased migration, then logical WAL snapshots and multi-session source staging.
3. **Retrieval correctness**: readiness `PortResult`, filtered semantic/hybrid retrieval, finite vector enforcement, stable rank fusion, cursor state binding, and shared MCP semantic wiring.
4. **Cross-entry registry**: all implemented provider IDs from the capability matrix, shared canonicalization, and Web parameter subset completion.
5. **Web experience correctness**: keep the embedded offline page as a protocol client; map visible filter controls to the existing Web contract, reset cursor state on input changes, and commit async responses only when their request generation/session still matches current UI state.
6. **Performance and dependencies**: bounded rebuild/SQL work, dependency upgrades, and final contract/spec/changelog synchronization.

## Key Design Decisions

- SQLite source snapshots use the SQLite backup API into a guarded temporary database. Snapshot bytes provide the fingerprint, while the original source path remains the durable source identity.
- `MessageEvent` and staged messages carry an optional provider session identity so a single OpenCode/Cursor source can emit multiple canonical Sessions. Singular report fields remain only as compatibility projections.
- `SemanticIndex::is_ready` returns `PortResult<bool>`; only `Ok(false)` may trigger explicit lexical fallback.
- Semantic retrieval receives the same normalized filters/facets/system-noise policy as lexical retrieval and applies them before the final limit.
- Message and session FTS rankings are merged by deterministic RRF rather than raw BM25 comparison.
- Cursor digests are versioned and bind requested mode, semantic readiness/model context, ranking version, result set, filters, facets, and repository signal.
- `SqliteStore::open` is genuinely read-only; schema migration occurs only through explicit maintenance or a writer-leased write path.
- Web filter controls serialize only parameters advertised by the backend capability contract; stale search/context responses cannot mutate current view state. Existing bilingual labels, light/dark themes, CSP, textContent rendering, and no-network deployment remain invariants.

## Safety and Rollback

Each milestone is a rollback point with focused tests before implementation. Failed migrations or snapshot/staging operations must leave the authoritative catalog and derived projections transactionally consistent. The relocation identity migration remains additive and is not part of ordinary sync.
