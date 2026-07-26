# Design: Canonical Source/Session/Span foundation and storage migration

Grounded in current code as of 2026-07-26 (branch
`chore/batches-1-3-governance-and-evidence`).

## 1. Boundaries

Layer flow unchanged: domain ← ports ← application ← adapters/cli.
Providers stay read-only. No new frontend surface; `show` gains two id kinds.

## 2. Domain changes (`agentsessions-domain`)

### 2.1 EvidenceSpan

```rust
/// Byte span into a verified source snapshot; end exclusive.
pub struct EvidenceSpan { pub start: u64, pub end: u64 }
```

Invariant: `start <= end`. Attached to `Message` as
`#[serde(default)] pub span: Option<EvidenceSpan>` — `None` means
"provenance not recorded" (legacy rows), never fabricated.

### 2.2 SourceDocument entity

```rust
pub struct SourceDocument {
    pub id: StableId,            // IdKind::Document
    pub provider_id: String,     // e.g. "claude-code"
    pub variant_id: String,      // e.g. "claude-code/jsonl-v1"
    pub fingerprint: String,     // BLAKE3 hex of the verified snapshot
    pub len: u64,                // snapshot length the spans index into
}
```

No path stored in the entity (identity ≠ location, RFC-0001). The
path→document association lives in storage (`source_membership` extension),
not in the canonical payload.

### 2.3 Session payload

`Session` struct already exists. New: it becomes a persisted entity. Payload
carries `document_id` and ordered member message wire ids (not full
messages — messages remain their own catalog rows).

`Session::validate` extended: member reference list must be Message-kind ids
(validated at construction site in CLI; domain validate keeps existing
checks and adds span invariant `start <= end` per message).

### 2.4 Message payload gains `session` reference

Canonical message JSON gains `"session": "<ses_v1_...>"` so
message → session → document walks need no heuristics.

## 3. Ports changes (`agentsessions-ports`)

### 3.1 MessageEvent gains span

```rust
pub struct MessageEvent<'a> {
    // ...existing fields...
    /// Byte offsets of the source record this message was normalized from,
    /// relative to the verified snapshot bytes. None if the provider cannot
    /// attribute a contiguous span.
    pub span: Option<(u64, u64)>,
}
```

Provider-reported at parse time (R2). Both current providers parse line-wise
over the full byte slice, so line start/end offsets are cheap and exact.

### 3.2 Source-unit compatibility (R4, design-only)

`SourceSnapshot` stays file-oriented. The *unit* a span indexes into is "the
verified snapshot bytes" — for a future row-level source, the snapshot bytes
are the extracted row payload and spans index into that. This is documented
on `MessageEvent::span` and `SourceDocument.len`; no trait change needed now.
The spike's row-identity scheme can implement `SourceDiscovery` by
materializing row-scoped snapshots without touching this contract.

### 3.3 Session/document staging

`StagedMessage` (application) gains `span: Option<(u64,u64)>` passthrough.
Application `select_and_stage` unchanged in selection logic. Session/document
entity *derivation* happens in the CLI composition root (same place message
ids are derived today, `staged_to_entries`), keeping application free of
provider-specific identity policy.

## 4. Identity derivation (composition root)

- Document id: `StableId::derive(IdKind::Document, Reconstructed,
  [provider_id, variant_id, fingerprint])` — content-addressed, relocation-
  invariant, idempotent across re-ingest of identical bytes.
- Session id: Claude Code — native `sessionId` field (uuid, per-file);
  Codex — native rollout session id from `session_meta`. Fallback when
  absent: `derive(Session, Reconstructed, [document_wire_id])`.
  Provider surfaces the native session id via a new `ParseReport` field
  (`session_native_id: Option<String>`) — report-level, not per-message.
- Message ids: unchanged (native uuid first, path+seq Reconstructed fallback).

## 5. Storage migration v5 → v6 (`agentsessions-adapters-sqlite`)

Following the existing gated-migration pattern (single tx, idempotent):

- New columns are NOT needed on `catalog` (payloads are opaque JSON).
- Session/document entities are ordinary catalog rows (`ses_v1_*`,
  `doc_v1_*` keys) flowing through the same durable outbox/commit path —
  no new table for entity storage.
- `source_membership` extension: today `(source_path, message_id)`. v6 adds
  `document_id TEXT` column (nullable for legacy rows) so tombstone
  reconciliation can also retire session/document rows when a source
  disappears. Migration: `ALTER TABLE ... ADD COLUMN document_id TEXT` +
  `PRAGMA user_version = 6`. Legacy rows keep NULL = "pre-v6, unknown".
- Spans ride inside message payload JSON — no schema impact.
- `index rebuild` unchanged mechanically; sessions/documents get indexed
  with empty FTS text (they are retrieval entities, not search hits) —
  simplest correct choice: exclude non-message ids from FTS insertion but
  keep them in catalog; rebuild derives FTS rows only for message-kind ids
  (kind known via `fts_ids` id_json or wire prefix).

Failure containment: commit of a source batch already goes through
`begin_index_batch`/`commit_index_batch`; session/document rows join the
same batch entries, so crash semantics are unchanged.

## 6. Retrieval

- `get <id>`: already payload-echo; works for new kinds for free.
- `show <ses_v1_...>`: structured session entity (document ref + member ids).
- `show <doc_v1_...>`: structured document entity (provider/variant/
  fingerprint/len).
- `show <msg_v1_...>`: gains `session` and `span` fields when present;
  legacy rows show them as `null` (explicit absence, R3).

## 7. Compatibility & rollback

- v5 store opened by new binary: migrated in one tx; on failure the tx rolls
  back and the store remains v5-readable by the old binary.
- Old binary opening v6 store: refused by existing `SchemaIncompatible` gate.
- Legacy message rows (no session/span): retrievable unchanged; absence is
  explicit `null`. Full fidelity requires re-ingest (documented in output of
  `status`? — no: keep scope minimal, document in migration test + runbook
  note only).

## 8. Test plan (essence)

- Domain: span invariant, session validate extensions, serde defaults.
- Ports/providers: each provider reports spans; round-trip test — slice
  snapshot bytes with reported span == original source record line.
- Sqlite: populated v5 → v6 migration test (existing pattern at
  lib.rs tests); mixed legacy+new rebuild preserves tiers and spans.
- CLI e2e: real-format Claude + Codex fixtures → ingest → show session/
  document/message with cross-references; search still hits messages only.

## 9. Tradeoffs

- Spans in payload JSON (not a column): loses SQL queryability, keeps
  migration trivial and payload authoritative. Search-by-span is not a
  requirement; acceptable.
- Sessions excluded from FTS: sessions are containers; indexing their
  concatenated text would double-count hits. Message-level hits + session
  walk-up covers search-to-session UX later (child 3).
- Session id per document (Claude sessionId is per-file): cross-file session
  stitching (continuation) is explicitly child-3 territory; here one
  document yields one session.
