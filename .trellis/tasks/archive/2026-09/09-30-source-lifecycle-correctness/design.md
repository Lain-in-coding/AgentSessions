# Design: format-aware ingestion and empty identity

## Routing
Resolve known provider facts from the existing installation/registry path. Gate jsonl_health on the existing RecordStream declaration; bounded-whole inputs use their parser. No format-by-extension heuristics, additional provider registry, or cached-proof bypass. Unchanged fingerprint handling and source verification stay intact.

## Empty lifecycle
Separate the empty byte classification from provider identity. For an unknown first-empty source, verify capture then return no-op/warning without creating entries, source scans or pending/persisted fake installation ownership. For a known source, reuse its proven provider and express the empty replacement through existing SourceBatch semantics. Share the provider/empty decision between ingest and sync instead of patching one path only.

For persisted provider `empty`, admit only a specifically proven empty placeholder transition. Validate stored zero-byte scan/fingerprint, derived placeholder entities/claims and absence of real native/message/relation facts; reject missing/corrupt/contradictory proof and alias/relocation conflicts. Reserve the new normal assignment without destructive pre-write cleanup, then revalidate and replace only the affected source binding inside the existing durable transaction. Preserve other sources that share placeholders/old namespaces. Do not create a general reassignment mechanism or a schema migration; real identity conflict/CAS/manifest/lease checks stay strong.

SourceBatch.provider_id is reserved for discovery ownership, not canonical provider metadata for explicit files. Empty repair must not expand discovery deletion authority.

## Partial integration
Consume A's accurate skipped count, keep relation_complete false on omitted rows, emit a bounded no-path coexistence warning at the CLI boundary, and retain current incomplete-context errors. Complete recovery may remove only unclaimed stale data through the existing source replacement machinery.

## Existing parser semantic cache
The completeness fix changes parser semantics. Advance the existing PARSER_SEMANTIC_VERSION from 2 to 3 so unchanged old scans are reparsed once; preserve INDEX_PROJECTION_VERSION and SQLite user_version. Add a stale-version unchanged-bytes regression and verify the following repeat is a no-op. This is existing cache invalidation, not a schema/public-protocol change.
