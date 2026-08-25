# Provider and time search filters

## Goal

Add provider and time-range filtering to CLI and MCP search with storage pushdown and cursor-safe normalization.

## Requirements

- CLI `search` accepts repeatable `--provider claude|codex`, `--since`, and `--until`.
- Repeatable providers are ORed; provider and time dimensions are ANDed.
- `since` is inclusive and `until` is exclusive; `since >= until` and unknown providers are invalid requests.
- CLI accepts RFC3339/ISO-8601 absolute values and compact `1h`, `1d`, `1w` relative to the injected application clock. MCP accepts absolute ISO-8601 only.
- Values normalize to UTC instants before application execution and participate in the cursor query digest.
- Predicates push into the FTS/storage query before page limiting; zero matches return a clean empty page.
- Omitted filters preserve existing search results, ranking, and cursor behavior.

## Acceptance Criteria

- [x] Provider-only, since-only, until-only, and combined filters return correct subsets.
- [x] Multiple providers implement OR semantics and time combines with AND.
- [x] Boundary timestamps obey `[since, until)`.
- [x] Invalid provider/range/time syntax maps to `invalid_request`.
- [x] A cursor cannot be reused with different normalized filters.
- [x] SQLite tests demonstrate predicate pushdown and clean empty results.
- [x] CLI compact durations use an injected/test clock; MCP rejects compact durations.

## Constraints

- Integration stream one; later search guidance adapts to its final DTO.
- Catalog remains authoritative and FTS remains rebuildable.
- No commit/push without owner authorization.
