# Audit Design

## Scope partitions

1. Workspace architecture and domain/ports/application crates.
2. SQLite adapter and persistence behavior.
3. Claude and Codex providers.
4. CLI composition, commands, robot protocol, and end-to-end tests.
5. Product plans, RFCs, ADRs, contracts, schemas, spikes, and release gaps.
6. Trellis specifications, tasks, workspace journal, and local agent configuration.
7. Git history/status, CI, packaging, repository hygiene, and test verification.

## Evidence strategy

Each partition is reviewed by a read-only agent. Reports must separate implemented behavior from documentary claims and cite repository-relative paths. The coordinator reconciles overlaps and runs repository-level validation commands.

## Exclusions

Generated build output under `target/` and vendored/reference projects under `Github_src/` are not first-party implementation. They are inventoried only to establish their role and ensure they are not confused with deliverables.
