# Design: reliability integration

## Boundaries and ordering
A repairs Hermes accounting before B enables regular SQLite updates; otherwise fixing the routing bug exposes deletion from incomplete parsing. C can run alongside A. A owns the Hermes provider implementation/goldens, B owns CLI/source-installation changes and cross-provider integration tests, C owns Domain ordering and its tests. Shared specs, task lifecycle, integration, and commits belong to the coordinator.

## Confirmed behavior decisions
- Partial Hermes scans use the existing skipped/completeness protocol. Temporary old/new document-scoped message copies are accepted and explained through bounded warnings. Do not fabricate native IDs or relax incomplete-context failures. A later complete scan removes only unsupported, unshared old claims.
- Count all actual message rows under the existing source/message caps. The orphan census cap is not an exact skipped count. If a hard bound is exceeded or accounting cannot be made trustworthy, fail explicitly rather than label the source complete.
- Apply JSONL tail triage only to known record-stream sources using existing registry/manifest facts. Whole-source adapters own their own validation. Do not identify formats by extension or add a parallel hard-coded provider registry.
- An unknown first-empty source is a verified zero-commit no-op with a bounded warning, not a persisted empty provider. A known empty replacement retains its real source/installation boundary.
- Historical empty repair is limited to provable empty-only placeholders: no real message/placement/activity/usage, native identity, valid resume fact or conflicting relocation/alias evidence. Missing-state placeholder claims may be validated as placeholders, not mistaken for native proof. Verify again inside the write transaction; preserve shared placeholders and other source bindings. No generic identity reassignment or namespace garbage collection.
- SourceBatch.provider_id remains discovery ownership, not a general provider metadata backfill for explicit inputs.
- Context ordering is lexicographic on timestamp class/value, document, ordinal, placement ID. Raw timestamps remain unchanged.

## Interfaces and rollback
No public DTO/flag/schema/dependency changes. Existing skipped/diagnostic fields carry truthful partial state; an internal ordering key or source helper is permitted. Preserve manifest tamper checks, writer lease, CAS, source validation, and real identity conflicts. Use focused commits; rollback must never reset another worktree or rewrite unrelated source history.
