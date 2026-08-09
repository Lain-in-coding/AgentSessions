# Implement: Canonical Source/Session/Span foundation and storage migration

Ordered checklist. Each step ends with the quality gates
(`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`)
unless noted. Windows note: verify file survival after every write
(Defender hazard — check byte count non-zero after Write/Edit).

## Phase A — Domain

- [ ] A1. Add `EvidenceSpan { start, end }` (+ invariant `start <= end`) and
      `Message.span: Option<EvidenceSpan>` with `#[serde(default)]`;
      extend `Session::validate` to reject invalid spans. Unit tests:
      serde default, invariant rejection.
- [ ] A2. Add `SourceDocument { id, provider_id, variant_id, fingerprint, len }`
      with a `validate()` checking `IdKind::Document`. Unit tests.

## Phase B — Ports + providers

- [ ] B1. `MessageEvent` gains `span: Option<(u64, u64)>`; doc comment fixes
      the unit as "verified snapshot bytes" (row-source compatible, R4).
      `ParseReport` gains `session_native_id: Option<String>`.
- [ ] B2. provider-claude: report line byte offsets as span for each emitted
      message; surface `sessionId` as `session_native_id`. Round-trip test:
      span slice == source line.
- [ ] B3. provider-codex: same, using rollout `session_meta` id.
      Ignore-mirror behavior unchanged (regression test stays green).

## Phase C — Application

- [ ] C1. `StagedMessage` carries `span` + plumb through staging sink.
      `select_and_stage` signature unchanged; staging report exposes
      `session_native_id`. Tests: staged spans preserved.

## Phase D — Storage v6

- [ ] D1. Migration v5→v6: `ALTER TABLE source_membership ADD COLUMN
      document_id TEXT` (+ `user_version = 6`), single tx, idempotent guard.
      Populated-upgrade test per existing pattern.
- [ ] D2. Exclude non-message id kinds from FTS insertion in commit path and
      `rebuild_index` (kind from `fts_ids` id_json, wire-prefix fallback).
      Tests: session/doc rows in catalog but not searchable; mixed
      legacy+new rebuild preserves tiers + spans.
- [ ] D3. Tombstone reconciliation: when a source's scan drops all its
      messages, retire its session/document rows too (only when no other
      source references them — reuse membership rules). Tests.

## Phase E — CLI composition root

- [ ] E1. `staged_to_entries` → derive document id (content-addressed) and
      session id (native first, Reconstructed fallback); message payload
      gains `"session"` ref + `"span"`; emit session + document catalog
      entries in the same `SourceBatch`.
- [ ] E2. `show`: structured rendering for `ses_v1_` (document + member ids)
      and `doc_v1_` (provider/variant/fingerprint/len); message `show`
      exposes `session`/`span`, `null` when absent. Human + robot formats.
- [ ] E3. E2E: real-format Claude + Codex fixture ingest → show all three
      kinds with correct cross-references; legacy-row behavior (ingest with
      old payload shape simulated by v5 fixture store) shows explicit nulls.

## Phase F — Finish

- [ ] F1. Full-scope quality check (all packages), spec update pass
      (`.trellis/spec/` — record span-unit contract + FTS exclusion rule).
- [ ] F2. Update `docs/` runbook note: legacy rows need re-ingest for spans.
- [ ] F3. Commit batches per repo convention (Conventional Commits, no AI
      footer, no push without explicit user request).

## Validation commands

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Rollback points

- Each phase is an independent commit batch; storage migration (D) lands
  only after B/C tests are green so a revert of D leaves a working v5 tree.
- Migration tx rollback leaves store v5-readable (design §7).
