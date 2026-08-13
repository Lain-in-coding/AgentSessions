# Design — UX review fixes

## Context

The 2026-08-13 newbie-UX diff changed five files (`cli/src/main.rs`,
`cli/src/human.rs`, `cli/src/protocol.rs`, `adapters-sqlite/src/lib.rs`,
`cli/tests/e2e.rs`). The multi-agent review reproduced nine blockers plus
medium issues. This design fixes them without re-litigating the settled
product decisions (ADR-0003..ADR-0006, CONTEXT.md 2026-08-13 decision
log).

## 1. Snippet pipeline (R1)

Current (broken): `attach_search_snippets()` in `main.rs` runs after the
Application clamp and render; per-hit `catalog.get()` (N+1), full payload
parse, no budget accounting.

Target:

- Snippet generation moves into the Application search assembly:
  `SearchResult` items carry an optional snippet (no redaction — owner
  decision, ADR-0004).
- `ResponseBudget.max_snippet_chars` becomes live (per-item cap; currently
  unused). Total snippet bytes count toward `max_response_bytes` during
  the same clamp that trims items.
- Payload fetch: new `CatalogStore::get_many(ids)` port method. SQLite
  implementation uses the existing variable-limit chunking (batched IN,
  ≤500 params); testkit `InMemoryStore` implements it trivially.
- Rendering: only the human renderer prints the snippet line; robot/MCP
  serializers omit the field — envelope shape unchanged (the review's
  compatibility requirement).

## 2. Help/version envelope (R3, ADR-0006)

New early interception stage in `main()`: after the global flag scan
(mode, request_id) but **before** `parse_db_flag` / store open /
special-command dispatch:

- top-level `--help` / `--version`;
- `<cmd> --help` / `-h` for every known subcommand, including
  `index` and `index rebuild`.

Behavior:

- human: print text to stdout, exit 0.
- json/jsonl/robot: success envelope, `command` = the subcommand (or
  `"help"` / `"version"`), `data = { help_text }` / `{ version }`;
  jsonl = exactly one frame; `request_id` echoed.
- No db creation, no writer lease, no file side effects on help paths.

## 3. Privacy-safe errors (R2)

- not_found: fixed generic message; `error.code` stays `not_found`
  (ADR-0005).
- sync directory: generic path-free message; platform-neutral wording for
  the expansion hint (no `Get-ChildItem`-only example).

## 4. Search hardening (R4)

- Application search entry rejects U+0000 and C0/C1 control characters →
  `invalid_request`. Never sanitize by deletion (token splicing risk).
- Protocol boundary: `PortError::Backend` details never appear in the
  user-visible message; generic text with bounded internal detail only.

## 5. operator_action (R5)

- `protocol.rs`: per-code match arms whose semantics are copied from the
  `operator_action` column of `schemas/robot/v1/error-catalog.json`.
- Unit test asserts every code has an action and that cursor/snapshot/
  generation actions match the catalog (drift guard).

## 6. Renderer (R6)

- `human.rs` show: branch on entity kind — `msg_v1_` curated projection;
  `ses_v1_`/`doc_v1_` generic key/value passthrough, no fabricated context
  hint.
- `human.rs`: dedicated `render_ingest()` (`variant`, `source_fp`,
  `committed`, `diagnostics`, `sources: 1`).
- context truncation hint prints the real wire id.

## 7. Parser robustness (R8)

- Value flags: if the next token is missing or is a known flag name →
  usage error (`invalid_request`). Applies to `--db`, `--request-id`,
  `--output`.
- Duplicate/conflicting global flags rejected (duplicate `--db`, duplicate
  `--request-id`, `--output`/`--robot` conflict or repetition).
- `doctor`/`config`: unknown positional tokens rejected.
- `command_name` derived from the command that actually failed.

## 8. Contract migration (R7)

- `smoke.ps1` / `smoke.sh` get-missing assertions → exit 4, `ok:false`,
  `error.code == "not_found"`; stale comments removed.

## 9. Docs (R9)

Mechanical sync as listed in the PRD.

## Rollback

- Each cluster lands as one logical change (commit only on owner
  authorization); reverting any cluster is independent.
- Envelope additions are additive (`data.help_text` / `data.version`);
  no existing field is removed anywhere.

## Open questions

None at planning time — Q1-Q7 settled by the owner via grilling.
