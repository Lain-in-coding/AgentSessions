# Relocation Implementation Plan

Status: complete. Planning approved on 2026-09-27; implementation and all required validation gates passed.

- [x] Map native/non-native identity derivation and all live source-locator tables.
- [x] Confirm current schema v17, explicit `index rebuild` upgrade path and write boundaries.
- [x] Write PRD/design, proposed command/alias lifetime and rollback contract.
- [x] Curate implementation/check contexts and validate their referenced files.
- [x] Obtain approval of the final planning summary and start this existing task.

## Phase 1: Contract and regression baseline

- [x] Freeze canonical IDs, source claims, resume observations and generation in
  synthetic relocation fixtures before changing implementation.
- [x] Add failure-first cases for a moved Claude/Codex root, a multi-session SQLite
  file, two independent installations with equal native IDs and Windows casing.
- [x] Add bounded port DTOs/policy using the existing provider registry and injected
  clock; pin plan/alias lifetime and privacy/error contracts.

## Phase 2: Persistent identity and additive schema

- [x] Add schema v18 registry/location/source-binding/receipt structures and durable
  manifest support. Bootstrap exact legacy namespace inputs without rekeying.
- [x] Allocate/persist new opaque namespaces inside authoritative source commits.
- [x] Cover old-schema/read-only opens, unresolved provenance, migration failure
  rollback and no-op validation before introducing the command.

## Phase 3: Relocation transaction and backup

- [x] Implement bounded preview/fingerprint mapping and occupied/overlap refusal.
- [x] Add consistent backup validation, fresh-plan/generation checks and snapshot
  revalidation under the writer lease.
- [x] Commit all seven existing live locator tables plus registry state through
  the durable-intent activation transaction. Preserve immutable historical
  manifests and intrinsic IDs/observations.
- [x] Test injected failure after each logical write group, recovery, source
  deletion after relocation, repeat no-op and explicit reverse moves.

## Phase 4: Composition and command

- [x] Inject persisted namespaces into staging; preserve content-based fallback.
- [x] Integrate active registered locations into ingest/sync/discovery and reject
  silent retired-location reuse.
- [x] Add top-level CLI `relocate` preview/apply parsing, help, Robot output and
  command scanners. Keep MCP/Web read-only and share protocol projection.
- [x] Update the installed-artifact smoke scripts and cross-entry read assertions.

## Phase 5: Verification and finish

- [x] Run the full-scope Trellis check, including claim/identity flow across layers,
  all live locator tables, backup ordering and privacy of opaque plan tokens.
- [x] Kill targeted mutations: new-root namespace derivation, omitted claim table,
  missing generation/snapshot validation and non-atomic activation.
- [x] Run workspace fmt/clippy/debug/release, semantic application/CLI feature gates,
  Node Web tests, Python suites, cargo-deny/audit, privacy and diff checks.
- [x] Update executable contracts/specs/changelog; record synthetic evidence and
  explicit limitations. Commit and archive only after acceptance passes.

## Ownership / Risk Hotspots

- Domain/Ports/Application: namespace input contract, plan policy, error/clock
  semantics. Do not move filesystem or SQL into these layers.
- SQLite adapter: v18 migration, registry, source claim mapping, backup and
  durable activation. Existing historical IDs/outbox data are rollback anchors.
- CLI: staging namespace injection, discovery, new command/help/Robot projection,
  e2e/install tests. No duplicated provider lists or automatic file moves.
- Formal authority: current task + RFC-0001, with concrete source anchors in
  `research/relocation-findings.md`. Pure implementation details may follow
  existing patterns; any material behavior/scope change returns to review.
