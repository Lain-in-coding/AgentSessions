# Evidence Handoff Pack — Design (slice 1: deterministic pack builder + CLI)

> Parent: `08-15-open-source-product-roadmap`. Contract dependency:
> `08-15-unified-release-contract` (handoff-pack/v1 formalization) and
> `08-15-offline-privacy-hooks` (real redaction engine) run in parallel as
> sibling subtasks. This slice defines the pack shape, the pure builder, the
> budget enforcement, the redaction seam, and the CLI surface.

## Context

No handoff code exists. The Application layer already assembles session
context (`AppRequest::Context` -> `AppResponse::Context`: ordered messages with
canonical payloads + aligned `EvidenceSpanDto` byte-precision evidence), and
`ResponseBudget` (`application/src/budget.rs`) already owns the shared budget
semantics (byte hard gate + structured truncation). `ADR-0009` (Proposed)
defines the cross-boundary redaction rule and the redaction metadata field
shape (`mode` / `status` / `ruleset_version` / `redacted_count` / `audit_id`).

Slice scope (from dispatch): a pure `HandoffPack` builder in
domain/application, budget enforcement via `ResponseBudget` with JSON-escaped
byte accounting, a redaction seam defaulting to no-op, an additive `handoff`
CLI subcommand (robot JSON + human summary), and a determinism test. No MCP
surface, no store/schema/ports changes.

## Decisions

### D1 — Builder is a pure function in the application crate

`application/src/handoff.rs` — `build_handoff_pack` takes the projections of
`AppResponse::Context` (`session_id`, `branch_leaf`, `branch_leaf_occurrence`,
`messages`, `evidence`, `generation`), options (`query`, `time_window`,
`policy`), the user `ResponseBudget`, an optional `provider_id` (resolved by
the composition root), and a `RedactionConfig`. It performs no I/O, reads no
clock, and uses only `Vec` + `BTreeSet` — same inputs always produce
byte-identical output.

### D2 — Pack shape (handoff-pack/v1, this slice)

```json
{
  "pack_version": "handoff-pack/v1",
  "pack_id": "hp_v1_<blake3 of pack_version+session+generation+query+window+policy>",
  "catalog_generation": 7,
  "generation_mode": "deterministic",
  "policy": "mainline",
  "query": null,
  "session_id": "ses_v1_...",
  "time_window": null,
  "provenance": { "provider_id": "claude-code", "session_id": "ses_v1_..." },
  "mainline": { "messages": [...], "branch_leaf": "...", "branch_leaf_occurrence": "..." },
  "evidence": [
    {
      "message_id": "msg_v1_...",
      "occurrence_id": "plc_v1_...",
      "role": "user",
      "snippet": "...",
      "timestamp": "...",
      "source": { "document_id": "doc_v1_...", "fingerprint": "...",
                  "byte_start": 0, "byte_end": 42, "record_ordinal": 0, "precision": "byte" }
    }
  ],
  "inference": [],
  "budget": { "max_response_bytes": 4194304, "bytes_used": 1715,
              "truncated": false, "reason": null },
  "redaction": { "mode": "noop", "status": "not-applied",
                 "ruleset_version": null, "redacted_count": 0, "audit_id": null }
}
```

- **Evidence vs inference separated by construction**: `evidence` holds only
  verbatim catalog data; `inference` is always empty in this deterministic
  slice (`generation_mode: "deterministic"`, Q44 default). Future LLM content
  may only enter `inference`.
- **Source cursor references, never paths**: `source.document_id` is the
  content-addressed `doc_v1_*` identity, plus fingerprint and exact byte
  range. No source/transcript filesystem path ever enters the pack (privacy
  contract; asserted by tests that walk the JSON for `path`-bearing keys and
  `.jsonl` strings).
- **Deterministic ordering**: evidence entries keep the context occurrence
  order (itself deterministic); mainline dedups message ids preserving order;
  `pack_id` derives from pack content inputs via blake3 (no wall-clock time).
- **Parent-PRD fields deferred** (contract task decides): matched sessions
  (multi-session packs), tool activity, confidence, session_native_id (resume
  metadata belongs to the resume sibling), Markdown projection, hash/offline
  validation. `pack_id` enables the hash/re-render story without implementing
  it.

### D3 — Budget: single occurrence gate + exact serialization convergence

The pack treats (message, evidence) as one atomic occurrence. Caps:
- entry cap = `min(max_messages, max_evidence_spans)`, reporting the binding
  knob name(s) (`max_messages` / `max_evidence_spans`, comma-joined when both
  bind) in `budget.reason`;
- byte gate = `max_response_bytes - ENVELOPE_RESERVE_BYTES (1024)`, converged
  against the **actual final serialization** (JSON-escaped, serde_json): the
  loop rebuilds the pack, measures `to_string().len()`, and drops the tail
  entry until it fits; `bytes_used` is made self-consistent (the field's digit
  count contributes to the length) by a bounded fixpoint iteration. Byte gate
  reason is `max_response_bytes` when it binds (clamp_items convention).
- snippet = `text` truncated to `max_snippet_chars`, then passed through the
  redaction seam.

The CLI requests `AppRequest::Context` with a fetch-all budget
(`usize::MAX` caps and bytes) so Context acts purely as an assembly source:
its independent messages/evidence clamping would otherwise double-apply the
user budget and can misalign the two lists (evidence truncates after
messages). The pack builder is the single budget enforcement point for packs
and reports truncation honestly.

### D4 — Redaction seam (ADR-0009 field shapes)

`RedactionConfig { mode, ruleset_version, audit_id, redactor:
Box<dyn Fn(&str) -> RedactionResult> }`, default `RedactionConfig::noop()`
(no-op closure, `mode: "noop"`, `status: "not-applied"`). The closure returns
`RedactionResult { text, redacted_count }`; the builder sums counts over
**kept** entries only, derives `status` (`not-applied` for noop, `applied`
when a redact-mode engine replaced >=1, `clean` when it ran with 0, `revealed`
for reveal mode). Never invent a redactor: no-op is explicit and nothing
claims redaction that did not happen. The sibling `offline-privacy-hooks`
task plugs a real engine into `RedactionConfig` without shape changes.

### D5 — CLI surface is additive

`handoff <ses-id>` with optional flags `--policy`, `--max-messages`,
`--max-evidence-spans`, `--max-bytes`, `--query <text>`,
`--window-start <ISO>` + `--window-end <ISO>` (must be given together;
filtering rule: entry timestamp must fall in the window and exist — fixed,
deterministic). Robot JSON carries the pack verbatim as `data`
(outcome `partial` + exit 10 when `budget.truncated`); human mode prints a
summary (pack id, session, provider, evidence count, bytes, redaction state,
truncation). `provider_id` for provenance is resolved via the existing `Get`
use case on the session payload's first document (`doc_v1_*` payload's
`provider` field); unresolvable -> `None` (honest, never fabricated).
No MCP tool in this slice (tool set is frozen; MCP `handoff` is a follow-up).

### D6 — Flag registration in all prefix scanners

`--query`, `--window-start`, `--window-end`, `--max-evidence-spans` are
value-bearing flags and are registered in every scanner that skips
value-bearing flags: `command_name`, `extract_request_id`,
`intercept_help_or_version`, `extract_db_flag_impl`, `bare_positionals`,
`is_known_flag_name` (main.rs) and `parse_output_mode` (protocol.rs), so a
value can never hide a later `--robot`/`--output` flag.

### D7 — Existing Context budget semantics untouched

`context` and the other commands keep their exact behavior; `budget_from_flags`
gains one optional parameter (`max_evidence_spans`) with existing call sites
passing `None`.

## Boundaries

- **Read-only**: pack generation reads the catalog only; provider transcripts
  are never touched. No cache, no catalog writes, no revealed export (a
  `revealed` pack may only leave via explicit export — deferred with the
  redaction engine; audit events are the sibling task's contract).
- **No path leakage**: no absolute transcript path, no `.jsonl` string, no
  `path`-bearing JSON key in any output (asserted by unit + e2e + smoke).
- **Explicit**: `handoff` is opt-in; no other command discovers or generates
  packs implicitly.
- **Deterministic**: same store state + same inputs -> byte-identical pack.

## Compatibility

- Additive CLI subcommand and flags only; no schema change, no ports change,
  no envelope change, no new canonical error code (errors reuse
  `not_found`/`internal`/`invalid_request`).
- `scripts/install/smoke.{ps1,sh}` gain a handoff assertion (the smoke.sh
  `doctor` invocation was aligned with smoke.ps1 — it passed `--db` twice,
  which the current binary rejects as a duplicate flag).

## Rollout / rollback

- Rollback: revert the CLI arm, the smoke lines, and delete
  `application/src/handoff.rs` + the `pub mod handoff` export. Nothing else
  consumes the pack.
- Rollout sequence: builder + tests -> CLI arm + tests -> smoke -> e2e.
- Contract handoff: `RedactionMetadata` field names/order and
  `RedactionConfig` shape are the seam the sibling tasks build against;
  `HANDOFF_PACK_VERSION`/`pack_id` prefix (`hp_v1_`) are the version anchor.
