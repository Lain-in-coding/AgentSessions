# Empty-placeholder and parser-cache evidence

Reviewed against the unchanged CLI/SQLite baseline on 2026-09-30 using synthetic inputs only.

## Actual old first-empty state
A standalone first-empty sync persists exactly these derived catalog placeholders:
- Document `doc_v1_9d688f0b845e2b6f7adb7a9eda35c150`, Reconstructed sidecar, payload provider=`empty`, variant=`empty`, len=0, fingerprint=`af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262` (BLAKE3 of empty bytes).
- Session `ses_v1_bf6e9effacfd4419e7a57dd0d881f0c6`, Reconstructed sidecar, document/documents referencing that document, messages=[].
- Two source_membership rows, each attributed to that document; no message entity and no resume claims. Source relation marker is version 7.
- Source scan len_bytes=0, matching empty fingerprint, provider_id=NULL, parser_version=2.
- Allocated-v1 namespace with provider_id=`empty`, one current location and the source binding. Namespace identity/clock and the physical locator are runtime values, not proof constants.

The schema allows shared document/session placeholders across sources, so repairing one source must not delete other claims. Verify full identity from fts_ids.id_json, never reconstruct stability from the wire key. A real source emptied later retains its REAL installation provider; do not confuse it with an initially unidentified empty source.

Relevant transaction seams: installation_for_source_commit (relocation.rs:488), persist_installation_in_tx (:663), source replacement application (lib.rs:~7010). Existing conflict checks must not become unconditional reassignment. The source replacement manifest already includes installation assignment; preserve existing hashing/tamper/CAS protections.

## Parser cache follow-through
PARSER_SEMANTIC_VERSION is currently 2 (adapters-sqlite lib.rs:7737). The code explicitly defines it as the semantic cache version for emitted entities AND relation/completeness behavior; stale versions force reparse even when bytes are unchanged. Correcting Hermes skipped/completeness therefore needs invalidation through this existing mechanism, not a new schema, public field or projection version. B owns updating this semantic version and a regression proving an unchanged version-2 scan is reprocessed once and then converges. Do not bump INDEX_PROJECTION_VERSION or user_version. This can cause one ordinary rescan after upgrade and must be mentioned in the final change notes.
