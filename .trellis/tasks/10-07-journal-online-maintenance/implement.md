# Implementation and validation

## Ordered delivery
1. Freeze shared maintenance Ports DTOs/interfaces; implement queue persistence and Application scheduling with mock-port tests. Storage adapter work can proceed independently behind these interfaces.
2. Implement typed maintenance opener/backup, fixed-subset atomic journal transaction, VACUUM/checkpoint, invariants and stage recovery; keep all B2 tests green.
3. Integrate CLI validation, safe worker process lifecycle and write-command wake; test actual subprocesses and measured physical reclamation.
4. Independent Trellis check: spec compliance, privacy/compatibility, concurrency and failure review; fix issues, rerun checks, update product docs/specs and record measured evidence.

## Checks
- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Targeted B2 `cargo test -p agent-session-grep-adapters-sqlite --test journal_retention`
- New Application/adapter/CLI maintenance tests, including real background processes on the current platform.
- Review release/CI platform matrix before declaring cross-platform evidence. Distinguish local evidence from CI not run.

## Safety
Use isolated temp data roots only. No maintenance on personal catalogs. Keep scoped changes; do not reset/revert others or touch generated Trellis integration files. Do not commit machine-local state. Before Windows recursive cleanup verify resolved ownership boundaries.

## Delivery records
User approved plan and implementation in the current conversation. Parent owns integration/docs and cross-child verification; children own independently testable code slices. Do not archive or claim completed until required acceptance evidence is present. No automatic push until commits and gates are checked; report incomplete gates honestly.

## Implementation result
All three code slices and independent safety review are complete. Final local gates and measured physical evidence passed; see research/verification.md. The user approved both work commits and push; archive only after committing the implementation and documentation.
