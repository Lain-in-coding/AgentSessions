# Source lifecycle correctness

## Goal
Restore updates for non-JSONL formats and make empty-input transitions safe and consistent without changing real identity or discovery ownership.

## Requirements
- Depend on the checked Hermes accounting child before enabling its normal incremental updates.
- Apply tail-health Retain only to known record-stream sources using existing registry/manifest evidence, not a filename extension or a new hard-coded provider list.
- Preserve record-stream truncation retention, recoverable partial rows, source immutability and capture/verify behavior. Whole-source inputs must reach their own adapter validation.
- A first unknown empty input is a warned, zero-commit no-op with no fake provider or placeholder binding; later valid content may establish its normal provider.
- Emptying a known source retains the proven provider/installation and uses honest empty replacement in both ingest and sync. Repetition and refill are safe.
- Permit restricted atomic correction of an existing empty-only binding after proving it contains no real message/placement/activity/usage, native identity, valid resume fact or conflicting alias/relocation proof. Revalidate under the writer transaction. Missing placeholder claims alone are not native identity; actual native or ambiguous state remains rejected.
- Keep explicit input distinct from discovery ownership: do not backfill SourceBatch.provider_id as if every explicit file were discovery-owned.
- Explain partial source retention and possible temporary old/new coexistence with bounded, path-free warnings. Do not change protocol fields or relax context completeness checks.

## Acceptance Criteria
- [x] Hermes, Cursor ItemTable/diskKV, OpenCode and whole JSON/Markdown updates become visible; unchanged repeats remain no-ops.
- [x] JSONL truncated-tail and partial-row regressions, source mutation, and malformed/cap-failing whole-source safety remain correct.
- [x] First empty -> valid; known valid -> empty -> repeat -> restored work for ingest/sync, canonical and standalone paths.
- [x] Existing empty-only repair succeeds; real/ambiguous identity, source changes and transaction failure leave prior state intact; other/shared source bindings remain unchanged.
- [x] Hermes valid/orphan/malformed-session partial ingestion retains old hits, reports honest counts/coexistence, and a complete rescan converges without deleting shared history.
- [x] Relevant CLI/SQLite tests and lint pass offline with no schema/dependency/maturity change.

## Scope
CLI source staging/sync/ingest, SQLite source-installation resolution/commit validation, and their existing tests. Do not modify the Domain implementation or Hermes parser owned by siblings. Shared specs and git/task lifecycle are coordinator-owned. Owner approved 2026-09-30.
