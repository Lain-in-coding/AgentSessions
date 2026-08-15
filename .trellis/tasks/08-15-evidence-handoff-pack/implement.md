# Evidence Handoff Pack — Implementation (slice 1)

## Preconditions

- Work in the worktree on top of the verified baseline (Resume core stream
  merged; sibling tasks `08-15-unified-release-contract` and
  `08-15-offline-privacy-hooks` run in parallel — do not wait for them).
- Research: parent PRD (`prd.md`), ADR-0009 (`docs/adr/ADR-0009-*.md`,
  Proposed — field shapes only), design.md (D1-D7).
- Competitor scan: `C:/AgentHub/project/Github_src/` — verified no
  handoff-pack/context-pack contract exists in hstry / fast-resume / memex
  (parent PRD finding). fast-resume's truncation code is CLI resume-command
  specific, not applicable to the deterministic pack byte gate. Nothing to
  copy; all pack logic builds on this repo's own primitives (ResponseBudget,
  serde_json, blake3).

## Steps

### 1. Pure builder (`crates/agent-session-grep-application/src/handoff.rs`)

- `HANDOFF_PACK_VERSION = "handoff-pack/v1"`, `HANDOFF_GENERATION_MODE =
  "deterministic"`; redaction vocabulary constants (mode: noop/redact/reveal;
  status: not-applied/applied/clean/revealed).
- Structs (serde snake_case, declaration order == wire order): `RedactionResult`,
  `RedactionConfig` (boxed pure closure), `RedactionMetadata` (exactly
  mode/status/ruleset_version/redacted_count/audit_id per ADR-0009),
  `TimeWindow`, `HandoffPackOptions`, `SourceCursor`, `EvidenceEntry`,
  `MainlineSummary`, `Provenance`, `PackBudget`, `HandoffPack`.
- `build_handoff_pack(session_id, branch_leaf, branch_leaf_occurrence,
  messages, evidence, generation, provider_id, options, budget, redaction)`
  — pure, no I/O, no clock:
  1. budget.validate() + alignment check (messages.len() == evidence.len()
     and per-pair placement_id == occurrence_id, else InvariantViolation);
  2. candidates: snippet chars -> max_snippet_chars, redactor applied; time
     window filter (ISO-8601 lexicographic); entries without document cursor
     excluded (never fabricated);
  3. entry cap = min(max_messages, max_evidence_spans), report binding
     knob(s); byte gate = max_response_bytes - ENVELOPE_RESERVE_BYTES with
     exact serialization convergence + bytes_used fixpoint;
  4. redacted_count summed over kept entries; status derived from mode/count.
- `derive_pack_id`: blake3 over pack_version+session+generation+query+window+
  policy, prefixed `hp_v1_`.
- Unit tests (determinism byte-identity, field shape, no path keys / no
  `.jsonl` in serialization, noop redaction never claimed, redactor seam
  counts and applies, snippet truncation, knob reason reporting (single and
  both), byte gate greedy tail-drop + bytes_used self-consistency, window
  filtering incl. timestamp-less exclusion, mainline dedup preserving order,
  alignment failures, pack_id changes with inputs / stable across runs,
  cursor-less entries excluded).

### 2. Application exports (`application/src/lib.rs`)

- `pub mod handoff;` + re-exports; `ENVELOPE_RESERVE_BYTES` -> `pub(crate)`.

### 3. CLI (`cli/src/main.rs`)

- `handoff <ses-id>` dispatch arm: flags `--policy`, `--max-messages`,
  `--max-bytes`, `--max-evidence-spans`, `--query`, `--window-start` +
  `--window-end` (paired or usage error); fetch-all Context request
  (assembly source only); provider resolution via existing Get on the
  session's first document; `RedactionConfig::noop()`; outcome partial
  (exit 10) when pack budget truncated; unknown-precision warning mirrors
  context.
- `budget_from_flags` gains `max_evidence_spans` param (existing call sites
  pass None).
- Register the four new value flags in ALL prefix scanners: `command_name`,
  `extract_request_id`, `intercept_help_or_version`, `extract_db_flag_impl`,
  `bare_positionals`, `is_known_flag_name`; `protocol.rs::parse_output_mode`.
- `known_subcommand` + `help_text` + `subcommand_help_text` + `KNOWN_COMMANDS`.

### 4. Human renderer (`cli/src/human.rs`)

- `render_handoff`: pack id / generation, session + provider, evidence count
  + bytes, redaction status (mode), truncation from `budget` (authoritative —
  no top-level `truncation`).

### 5. Tests (`cli/tests/e2e.rs`)

- Deterministic pack across two runs (byte-identical `data`), pack_version,
  inference empty, redaction not-applied, provenance provider claude-code,
  evidence source cursors, no fixture path / `.jsonl` / path-bearing keys in
  stdout; evidence-cap truncation -> exit 10 partial with reason; time
  window + query carry-through; missing session -> not_found exit 4; human
  summary lines.

### 6. Smoke (`scripts/install/smoke.{ps1,sh}`)

- handoff assertions after context (pack_version, non-empty evidence,
  redaction not-applied); fix smoke.sh doctor double `--db` (align with
  smoke.ps1).

## Validation

- `CARGO_TARGET_DIR=C:/AgentSessions/target-08-15-handoff-pack`
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - smoke.sh against the built binary (all assertions incl. handoff).

## Review gates (trellis-check)

- No absolute transcript path / `.jsonl` / `path`-bearing key in pack,
  envelope, warnings, or human output; redaction fields shaped exactly per
  ADR-0009; default no-op never claims redaction; determinism test green;
  flags registered in every prefix scanner; no MCP surface added.

## Rollback

- Revert CLI arm + smoke lines + delete `handoff.rs` and the module export.
  Nothing else consumes the pack.
