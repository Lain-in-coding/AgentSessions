# Add explicit session relocation aliases

## Goal

Follow-up to comprehensive review P2-9: preserve ses_v1 and add explicit installation registry with conflict-checked relocation aliases

## Requirements

- Resolve comprehensive review P2-9 without rewriting existing `ses_v1` identities during ordinary sync.
- Persist installation namespaces independently of current source locations; accept an explicit old/new installation mapping through a maintenance operation under the writer lease.
- Add aliases with conflict detection and transactional updates of locators, claims, resume metadata, and generation. Do not infer that identical native session IDs from different installations represent the same Session.
- Preserve lookups by old canonical IDs; define alias retention, cycle rejection, backup and rollback behavior before migration implementation.
- Keep private paths and provider-native IDs out of diagnostics and shared evidence.

## Acceptance Criteria

- [ ] Synthetic relocation between drives and user roots keeps canonical Session/Message identities and valid old-ID lookups.
- [ ] Two independent installations with the same native Session ID remain distinct; ambiguous mappings fail without writes.
- [ ] Windows path casing, multi-source/multi-session catalogs, interrupted migration and repeated migration are covered.
- [ ] Ordinary sync never silently upgrades or rekeys historical `ses_v1` rows.
- [ ] Design and migration command are reviewed before implementing the additive schema change.

## Notes

- Registered follow-up from `09-25-comprehensive-review-repairs`, whose approved design preserves historical identity and isolates relocation migration from ordinary sync.
- Deferred because a safe cross-installation mapping cannot be derived from an absolute path or a native ID alone. The repair task preserves the compatibility boundary and documents the remaining limitation; it does not claim relocation is fixed.
- Authority: `docs/architecture/RFC-0001-canonical-model-and-stable-id.md` section 5.1 and comprehensive review P2-9.
