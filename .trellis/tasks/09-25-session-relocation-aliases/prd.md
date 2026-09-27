# Preserve Session Identity Across Explicit Relocation

## Goal

Let an owner move an indexed provider installation to another directory or
drive, explicitly reconnect that location to its persisted identity, and keep
all existing Session/Message IDs and old canonical lookups valid.

## Background

This is the registered P2-9 follow-up from the comprehensive review. The
previous repair task deliberately preserved historical `ses_v1` derivation.
Current native Session identity still includes a path-derived installation
namespace (`cli/src/lib.rs:3108`, used at `:3605`). Domain hashes that namespace
at `domain/src/ids.rs:235`; keeping its original input avoids a rekey.
The source evidence and live locator-table inventory are in
`research/relocation-findings.md`.

## Requirements

1. Persist installation namespaces and source bindings. Bootstrap existing
   native Sessions with their exact legacy namespace inputs; allocate opaque
   persistent namespaces for new installations. Registry loss must not be
   advertised as identity-preserving reconstruction.
2. Provide an explicit CLI relocation operation with old/new roots and a
   canonical provider. Its default action is a read-only preview. Applying
   requires a fresh matching plan and a verified catalog backup.
3. Reconnect unchanged, verifiable source files to the same namespace. Keep
   `ses_v1`, Message, Document and placement identities unchanged. Preserve
   no-native stability grades and multi-session source boundaries.
4. Move all live source claims, registry locations and resume-claim locators
   atomically under the writer lease and existing durable-intent/generation
   protocol. Preserve observed native IDs and original CWD values.
5. Keep independent installations separate even if native Session IDs match.
   Occupied targets, namespace collisions, overlapping/cyclic mappings,
   changed/missing files and unverifiable legacy provenance fail without a
   partial live-state change.
6. Use flat location aliases pointing directly to a namespace. Proposed default
   retired-location compatibility is 90 days, configurable from 1 to 365 days.
   Retired locations cannot silently ingest as the moved installation. This
   window never expires existing canonical Session IDs or their lookups.
7. Integrate persisted active locations with ingest/sync and explicit discovery;
   the next scan of the new location must reuse identity and source membership.
   No provider configuration or provider file is moved or rewritten.
8. Define Windows casing/separator/drive and Unicode behavior without changing
   native identifiers. Diagnostics, Robot results and shared evidence contain
   counts/digests, not private roots, native IDs or transcript content.

## Acceptance Criteria

- [x] Synthetic relocation between drive/user-root layouts preserves every
      canonical Session/Message ID, context and old-ID lookup after re-sync.
- [x] A multi-source/multi-session SQLite catalog preserves placements, usage,
      tool activity, resume claims and source completeness across relocation.
- [x] Two independent installations with matching native Session IDs remain
      distinct; an occupied or ambiguous target is refused atomically.
- [x] Preview does not create/migrate/write the catalog, source or backup.
      Apply requires a current plan, writer lease and successful backup.
- [x] Injected failures leave all live tables and generation unchanged;
      interrupted durable intents recover through the existing abort path.
- [x] Repeating an applied mapping is a no-op; a stale plan fails explicitly;
      reverse relocation requires its own fresh explicit mapping.
- [x] Windows casing/separators, Unicode roots, ancestor overlaps, retired
      locations and alias expiry are covered without canonical-ID expiry.
- [x] CLI/Robot contracts, help, capability metadata, installer smoke and
      applicable specs describe the same CLI-only mutation boundary.
- [x] Format/lint, workspace debug/release, semantic feature, Web/Python,
      supply-chain, privacy and diff gates pass with synthetic fixtures only.

## Out of Scope

- Automatic source-file moves, copying, provider configuration changes or
  heuristic relocation during ordinary sync.
- Automatic merging/rekeying of separately indexed destination Sessions,
  cross-catalog merges or generic Session-ID rewrite aliases.
- MCP/Web mutation tools, lossy Unicode/native-ID normalization, guessed
  resume metadata, new remote services or real-transcript evidence.

## Planning Status

The first-release scope and 90-day location-alias default were approved on
September 27, 2026. The implementation and required verification gates completed on September 27, 2026.
The branch is ready for review and merge; registry loss remains explicitly
unreconstructible without a fresh explicit source scan.
