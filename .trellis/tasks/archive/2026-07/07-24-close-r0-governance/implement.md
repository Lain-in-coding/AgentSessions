# R0 Closure Implementation Plan

## Autonomous corrections

- [x] Repair `spikes/data-root-locking` to read/write through the same locked handle on Windows.
- [x] Add executable regression coverage for the repaired locking behavior where practical.
- [x] Rerun data-root, source-snapshot, SQLite-WAL, and search-backend spike scenarios.
- [x] Update spike evidence so commands, dependency versions, outcomes, and limitations match the checked-in programs.
- [x] Remove factual contradictions in target counts, protocol/schema fields, Provider maturity statements, and milestone wording.
- [x] Add missing Spike Card information for the four evidence-only spikes.
- [x] Produce an R0 architecture-review checklist separating facts from approval decisions.

## Owner decision packets

- [x] RFC-0001: ContentBlob layers, alias retention, normalization fingerprint, completeness enum.
- [x] RFC-0002: discovery roots/excludes, fingerprint minimum, mixed-version reporting, supported historical variants.
- [x] ADR-0001: formal corpus, relevance metric, performance thresholds, accepted backend decision.
- [x] ADR-0002: formal target list including Linux musl status and release certification requirements.
- [x] CLI/Robot/MCP: not-found semantics, cursor/error matrix, partial outcome, machine-mode help/version.
- [x] Threat Model: index-time redaction, privacy mode, network filesystem support.
- [x] Operations: license approver, external credential owners, final release dry-run responsibilities.

All packets are prepared in `docs/architecture/R0-ARCHITECTURE-REVIEW.md`; their governance decisions remain Pending until real owner/approver signatures exist.

## Validation

```text
cargo run --manifest-path spikes/data-root-locking/Cargo.toml
cargo run --manifest-path spikes/source-snapshot/Cargo.toml
cargo run --manifest-path spikes/sqlite-snapshot-wal/Cargo.toml
cargo test --manifest-path spikes/search-backend/Cargo.toml
cargo run --manifest-path spikes/search-backend/Cargo.toml
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

## Review gates

- No document moves to `Accepted` without explicit project-owner approval.
- No cross-platform checkbox closes without a real run result for that target.
- Evidence and executable output must agree before the task is considered decision-ready.
