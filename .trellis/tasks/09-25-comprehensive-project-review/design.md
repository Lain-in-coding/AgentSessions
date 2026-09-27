# Review Design

## Boundaries

Audit the product from provider input through staging, catalog and projections, application services, and all exposed entry points. Inspect packaging, schemas, installation, and CI as supporting product surfaces. Exclude competitor clones and generated agent integrations from product bug counts.

## Workstreams

- Storage: SQLite transactions, migrations, projection recovery, identities, filtering, ranking, cursors, vectors, contention, and scaling.
- Providers: all provider crates, bounded parsing, malformed input, source snapshot consistency, identity/ordering, and loss accounting.
- Entry points: CLI, MCP, Web, TUI, hooks, resume, protocol/error/privacy contracts, and surface consistency.
- Coordinator: domain/ports/application, baseline execution, scripts/release/CI, cross-layer integration, deduplication, and final reproduction.

## Evidence Contract

Read indexed code through CodeGraph first; narrow to actual source when the index does not cover a detail. Read applicable specs and current formal contracts. Historical reports suggest hypotheses only. A finding requires a concrete reachable path; intentional limitations and accepted contracts are not bugs. Label evidence as runtime-reproduced, source-proven, or unconfirmed.

Use P0 for immediate critical damage, P1 for major correctness/security/data loss, P2 for ordinary functional defects, and P3 for lower-impact defects. Optimization items carry expected benefit, cost, and validation method rather than invented benchmark numbers.

## Safety and Compatibility

No product changes or behavioral migrations. Use synthetic temp data; never run ingestion against real user sources. Run test processes with finite waits and retain meaningful evidence in the task. Workers own disjoint report files and must not revert others' work.

## Deliverables

`research/storage.md`, `research/providers.md`, `research/entrypoints.md`, and a consolidated `review-report.md` with test and coverage evidence.
