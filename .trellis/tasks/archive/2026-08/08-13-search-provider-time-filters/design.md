# Design — Provider and time search filters

## Types

Introduce backend-independent normalized provider/time filter types. Application requests carry UTC instants and a canonical sorted provider set. Relative CLI syntax is resolved before the port call through the injected clock.

## Storage pushdown

Extend the search port query input rather than post-filtering paginated hits. SQLite combines `fts MATCH` with authoritative catalog/identity metadata predicates in one prepared query and keeps the existing score-desc/ID-tiebreak order. No denormalized filter field becomes a new authority.

## Cursors

Hash the canonical raw query plus sorted providers and normalized UTC bounds into the existing query digest. Pagination then remains stable and rejects filter mutation.

## Validation

CLI handles repeatable provider flags and compact/absolute times. MCP schema exposes provider array and absolute since/until strings only. Application validates the normalized half-open interval defensively.

## Rollback

Remove optional filters from request/query types and CLI/MCP parsing; omitted-filter SQL remains the existing path.
