# Unified release contract — Implementation (slice)

> Executed 2026-08-15 by the implement agent in worktree
> `agent-aa094103dbc05af90` (baseline `main` @ f4175a0).
> Design authority: `design.md` in this task dir (decisions D1–D4 +
> deferred identity migration plan).

## Steps

### 1. `asg` CLI alias

- `crates/agent-session-grep-cli/Cargo.toml`: second `[[bin]]`
  (`name = "asg"`, `path = "src/main.rs"`). No `CARGO_BIN_NAME`
  references exist in the crate, so both names share main.rs unchanged.
- Verified: `cargo build` produces both `agent-session-grep` and `asg`
  binaries.

### 2. Robot protocol 1.0 → 1.1 (additive)

- `crates/agent-session-grep-cli/src/protocol.rs`:
  - `SCHEMA_VERSION` `"1.0"` → `"1.1"`.
  - Four new `CanonicalCode` variants (wire / exit / retryable):
    `capability_not_supported` (7, no), `model_download_failed` (6, yes),
    `model_corrupted` (6, no), `vector_index_not_ready` (6, yes); each with
    `as_str`, `exit_code`, `retryable`, `operator_action`.
  - Drift tests updated: runtime array 13 → 17 codes,
    envelope-schema code enum 13 → 17, new exit-code assertions,
    operator_action coverage for new codes, envelope test literal → 1.1.
- `schemas/robot/v1/envelope.schema.json`: four `schema_version` consts
  `1.0` → `1.1`; `errorBody.code` enum + 4 codes.
- `schemas/robot/v1/error-catalog.json`: `schema_version` → `1.1`; four
  new entries (layer/retryable/partial_allowed/cli_exit_code/robot_ok/
  redaction/operator_action). Layers: `application` (capability),
  `semantic` (model ×2, vector index).
- `crates/agent-session-grep-cli/tests/e2e.rs`: `assert_envelope_shape`
  schema_version literal → 1.1.
- No existing field/code renamed or removed; no SQLite schema touched.

### 3. Ports contract types (no engine)

- `crates/agent-session-grep-ports/src/lib.rs`:
  - `ProviderCapability` enum (discover/parse/search/context/resume/
    handoff/tool_activity) + `as_str` wire strings +
    `ProviderCapabilityManifest`.
  - `ProviderAdapter::capabilities()` trait method with default
    `vec![ProviderCapability::Parse]` — existing adapters compile
    unchanged.
  - `OutputBoundary` enum (HumanCli/Tui/Robot/Mcp/Web/Http/Handoff) +
    `is_cross_boundary()` (ADR-0009 split).
  - `RedactionMode` (default/reveal), `RedactionStatus`
    (applied/none/partial), `RedactionMetadata` with exactly
    `mode, status, ruleset_version, redacted_count, audit_id: Option<String>`.
  - Types only + doc comments; no serde derives (ports has no serde dep).

### 4. handoff-pack/v1 schema draft

- `schemas/handoff-pack/v1/handoff-pack.schema.json`: `status: "draft"`,
  `schema_version: "draft-2026-08-15"`. Contract-first registration of
  pack_id/generation/budget/truncation/redaction + evidence/inference
  split with per-item spans; Markdown is a projection, never authoritative.

### 5. Deferred identity migration (design only)

- See `design.md` "Deferred: provider-scoped session identity" —
  `ses_v2_*` coexistence, `installation_namespaces` registry,
  `id_alias` TTL table, outbox-transaction backfill, multi-ID fail-closed.
  No code in this slice.

### 6. Docs / logs (main repo task dir)

- `design.md`, `implement.md` created; `check.jsonl` and `implement.jsonl`
  entries appended.

## Validation

```
CARGO_TARGET_DIR=<repo>/target-08-15-unified-release-contract
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --bins
```

Results recorded in the final report; no commit/push (owner authorization
required).

## Review gates

- No absolute transcript path / personal identity in any new code, test,
  schema, or doc (all examples synthetic; schema `$id` is the product
  domain).
- Drift gate: error-catalog.json ⇄ envelope.schema.json ⇄ CanonicalCode
  stay in lockstep (test `published_error_catalog_matches_runtime_mapping`,
  `published_envelope_schema_contains_runtime_contract`).
- `asg` and `agent-session-grep` both build from the same main.rs.
