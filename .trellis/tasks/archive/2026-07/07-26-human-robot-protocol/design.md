# Design: Human/Robot protocol completion

Normative: CONTRACT-cli-robot-mcp-draft §4 §6. Grounded in code as of
2026-07-26 (children 1-3 landed). All work is inside `agentsessions-cli`;
no ADT/domain/storage changes.

## 0. Documented decisions / deviations

1. **Human renderer is color-free** in v1: no TTY detection, no ANSI. This
   keeps `--no-color` honest (there is no color to disable) and adds no deps.
2. **Warnings source v1** is computed in the CLI projection layer (`render()`
   in main.rs) from `AppResponse::Context.evidence` (count of
   `precision: unknown`), NOT in the Application layer — PRD forbids ADT
   changes. E2E cannot hand-craft a legacy (pre-v6) store without adding a
   sqlite dev-dep to the CLI, so the warning-present path is unit-tested at
   the projection level; e2e asserts `warnings: []` on a modern store and the
   plumbing end-to-end.
3. **Broken pipe** maps to silent exit 0 (POSIX pager convention); any other
   stdout write error prints one stderr line and exits 5 (source_io class).
4. **`diagnostic` frame kind** is defined in the schema + builder for contract
   completeness but v1 emits none (progress covers the current need). Emitting
   none is compliant; the vocabulary is what the contract freezes.

## 1. File ownership (parallel agents)

```
crates/agentsessions-cli/src/human.rs      [agent A]  NEW: human renderer
crates/agentsessions-cli/src/protocol.rs   [agent B]  envelope/frames/details/request-id/pipe-safe writes
schemas/robot/v1/envelope.schema.json      [agent B]  progress+diagnostic frame defs
crates/agentsessions-cli/src/main.rs       [main after agents; agent B only for mechanical emit call-site fixes]
crates/agentsessions-cli/tests/e2e.rs      [main]     truth-table e2e
```

Main pre-adds `mod human;` + a stub `human.rs` before dispatch so agent A never
edits shared files. Because §3 REMOVES `protocol::emit`, agent B must also make
the MINIMAL mechanical call-site replacements in main.rs (`emit(...)` →
`write_stdout_line(&success_envelope(..., &[], None))`, behavior-identical, no
new features) so the crate keeps compiling; B touches nothing else in main.rs.
Main session does all real wiring (§4) only AFTER both agents report done.

## 2. human.rs (agent A) — frozen API

```rust
use crate::protocol::{Outcome, Page};

/// Render a SUCCESS result as human-readable lines (no envelope, no color).
/// `data` is exactly the JSON `render()` in main.rs builds per command
/// (shapes frozen by child 3; see below). Unknown commands fall back to
/// pretty-printed data (never panic).
pub fn render_success(
    command: &str,
    outcome: Outcome,
    data: &serde_json::Value,
    page: &Page,
) -> Vec<String>
```

Per-command output rules (unit-test each, including zero-result):

- `search`: header `N hit(s) (generation G)`; per hit `  <rank>. <id>  score <s>`
  (score 2 decimals); zero hits → `no hits`. If `data.truncation.truncated` →
  line `truncated: <reason>`. If `page.has_more` → `more: pass --cursor <token>`.
- `list`: header `N entrie(s) (generation G)`; per entry `  <id>  <payload
  preview ≤60 chars, control chars replaced>`; zero → `catalog is empty`;
  truncation/cursor lines like search.
- `get`: payload string as-is, or `not found`.
- `show`: `entity is null` → `not found`; else one `key: value` line per
  top-level field (sorted keys, nested values compact JSON).
- `context`: header `session <id>  branch leaf <leaf|none>  (generation G)`;
  numbered messages `  <n>. [<role>] <text first 80 chars>` (role/text read
  from each message payload; missing → `?`); `evidence: N span(s)`;
  truncation line when truncated.
- `status`: `entities: N` + `generation: G`.
- `ingest`/`sync`/`index`/`index.rebuild`/`doctor`/`config.paths` and any
  other command: sorted `key: value` lines of the data object (nested values
  compact JSON). This is the generic fallback and MUST be the same code path.

No trailing blank lines; every returned string is one printable line.

## 3. protocol.rs (agent B) — frozen API

Signature changes (main.rs compiles against these exactly):

```rust
pub struct Page { pub next_cursor: Option<String>, pub has_more: bool } // unchanged

pub fn success_envelope(command: &str, outcome: Outcome, data: Value,
    duration_ms: u64, page: &Page, warnings: &[String],
    request_id: Option<&str>) -> String
pub fn error_envelope(command: &str, err: &ProtocolError,
    request_id: Option<&str>) -> String
/// {schema_version, frame_type:"progress", command, request_id, message}
pub fn progress_frame(command: &str, message: &str,
    request_id: Option<&str>) -> String
/// Envelope pattern ^[A-Za-z0-9._:-]+$ and 1..=128 chars.
pub fn valid_request_id(s: &str) -> bool
/// Println replacement: EPIPE → silent exit 0; other errors → one stderr
/// line + exit 5. All protocol stdout goes through this.
pub fn write_stdout_line(line: &str)
```

- `emit()` is REMOVED — main.rs owns the mode branch (human vs envelope).
- `ProtocolError` gains `pub details: Value` (default `json!({})`), builder
  `with_details(self, Value) -> Self`. `From<AppError>` populates:
  `GenerationMismatch` → `{"cursor_generation": c, "active_generation": a}`;
  `ContractMismatch` → `{"cursor_contract_major": c, "supported": s}`.
  Error envelope emits `err.details` (still bounded ≤32 props by construction).
- `request_id: Option<&str>`: `Some` → echoed verbatim; `None` → existing
  generated `cli-<pid>-<millis>`.
- `warnings` serialized into the success envelope array (was hardcoded `[]`).
- envelope.schema.json: add `progress` and `diagnostic` `$defs` (fields:
  schema_version const, frame_type const, command, request_id, message
  maxLength 4096; additionalProperties false) and extend the top-level
  `oneOf` to 4 refs. Update `published_envelope_schema_contains_runtime_contract`
  to assert the frame kinds; keep all 13 error codes assertions.
- Unit tests: request-id validation matrix, details population, progress frame
  shape, envelope carries warnings, plus keep existing tests compiling with
  new signatures.

## 4. main.rs wiring (main session)

- `mod human;` (pre-added with stub).
- Parse `--request-id <v>` in `parse_db_flag`-adjacent global handling; invalid
  → usage error (exit 2) BEFORE opening the store; thread `Option<String>`
  through `run`/`dispatch` emission and the error path in `main()`.
- Mode branch replaces `protocol::emit`:
  - Human: `human::render_success(...)` lines → `write_stdout_line` each;
    warnings → stderr (`warning: ...` prefix, one line each).
  - Json/Jsonl: `success_envelope(..., &warnings, request_id)` → one line.
- `render()` returns `(Outcome, Value, Page, Vec<String>)`; Context arm counts
  `precision == "unknown"` evidence → warning string
  `"<n> of <m> evidence spans have unknown precision (legacy rows; re-ingest to restore byte spans)"`.
- `dispatch` gains `mode` param; `sync` emits `progress_frame` per source
  (after staging that source) ONLY when `mode == Jsonl`.
- `main()`/error path: `error_envelope(&command, &err, request_id)`;
  human diagnostics stay on stderr.

## 5. Test plan

- Unit (A): renderer per command incl. zero/truncated/cursor-hint.
- Unit (B): protocol as §3.
- Unit (main, in main.rs `#[cfg(test)]`): context warning computation from a
  synthetic `AppResponse::Context` with mixed precision.
- E2E (main, e2e.rs): human search/status/context are text (no
  `schema_version`), zero-result wording; jsonl sync over 2 files emits
  progress frames + one response frame (all lines valid JSON); `--robot` sync
  emits exactly one line; `--request-id` echo + invalid rejection; error
  details on generation_mismatch; warnings `[]` present on modern-store
  context envelope.
