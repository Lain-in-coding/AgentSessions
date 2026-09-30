# Hermes partial-source accounting

## Goal
Prevent unparsed Hermes SQLite message rows from authorizing deletion of previously indexed history.

## Requirements
- Count source message rows accurately within existing limits, including orphaned messages and messages excluded by invalid session rows; committed + skipped must account for actual source rows without inventing an exact count from a capped diagnostic census.
- Keep partial reception: emit valid messages, truthfully report skipped rows and diagnostics, and rely on existing source incompleteness to retain prior claims.
- Accept temporary old/new document-scoped copies; never invent native identity or dedup by content. Complete rescans must be able to restore complete state.
- Preserve JSON variant behavior, readonly snapshots, caps, synthetic provenance, error classification, and experimental maturity.

## Acceptance Criteria
- [x] Unit/golden cases cover valid + orphan rows, all-orphan data, NULL/blank/malformed session IDs with messages, ordinary bad rows, exact limits and cap overflow.
- [x] Every in-budget source has truthful committed/skipped accounting; diagnostics distinguish omitted rows without leaking content.
- [x] No schema/dependency/public-field change; Hermes tests and targeted lint pass offline.
- [x] Parent B integration proves partial scans retain old indexed history and full recovery removes only obsolete unshared claims.

## Scope
Only the Hermes provider source and existing provider tests/golden expected output where necessary. Coordinator owns shared specs/docs; B owns CLI integration and coexistence warnings. Approved 2026-09-30 as part of the parent final plan.
