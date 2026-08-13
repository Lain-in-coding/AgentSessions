# Core usability: CJK indexing, probe tolerance, search-hit context

## Goal

Fix the highest-severity findings from the 2026-08-13 UX test round: CJK
search is effectively broken (5 independent reproductions), broken transcript
lines reject the whole source with a misleading error (3 reproductions),
merged multi-session files silently collapse into the first session, and
search hits lack the session context an AI needs (2 reproductions). All four
block the product's core promise — fast, accurate recall over the owner's own
Chinese-first history.

## Requirements

### R1 CJK bigram indexing (ADR-0007)

- R1.1 Index-time transform: contiguous CJK runs in indexed text are split
  into adjacent-char bigrams joined by spaces ("配置备份" → "配置 置备
  备份"); the same transform applies to the query before FTS matching.
- R1.2 Migration: `index rebuild` re-projects the FTS table from the catalog
  (unchanged); generation advances — cursors expiring on rebuild is normal
  contract behavior.
- R1.3 Compatibility: plain-text-only semantics (ADR-0003) preserved; ASCII,
  paths, punctuation, flag-name queries behave exactly as today.
- R1.4 Tests: CJK recall fixtures (2-char queries like "配置"/"数据库" hit
  messages containing them in longer sentences); no regression on the
  existing literal/special-char table; index-size/word-count growth
  documented.

### R2 Probe broken-line tolerance + line diagnostics

- R2.1 A source whose sample window contains a small number of non-JSON
  lines must not be wholesale rejected as "no provider recognized"; degrade
  to a recoverable path matching the parser's existing skip-with-diagnostics
  behavior.
- R2.2 When a source IS rejected, the error message reports the offending
  line number(s) and a repair direction — never the bare misleading
  "no provider recognized this source".
- R2.3 Tests: golden basic.jsonl (which ships with an intentionally broken
  line) ingests successfully; a source with a fatal structural break still
  fails, with line-numbered diagnostics.

### R3 Merged-file session diagnostics

- R3.1 A single file containing multiple distinct sessionIds must not
  silently collapse into the first session: emit a diagnostic (sessions
  found + count) and document the single-file=single-session behavior.
- R3.2 Tests: multi-session file reports the diagnostic; single-session
  files are unaffected.

### R4 Search-hit session_id + text summary (ADR-0008)

- R4.1 robot/json/jsonl search hits gain `session_id` (the owning session's
  wire id) and `text` summary; MCP `search_sessions` hits the same.
- R4.2 Summary bytes count toward `max_response_bytes` (reuse the snippet
  byte-accounting path); truncation explicit.
- R4.3 Human output unchanged (already shows snippet); machine schemas get
  additive fields only, major unchanged.

## Acceptance Criteria

- [ ] CJK recall: 2-char Chinese queries hit known-containing messages
      (fixtures); ASCII/path/punctuation behavior byte-identical to before.
- [ ] golden basic.jsonl (broken line included) syncs successfully with
      diagnostics; fatal sources fail with line numbers.
- [ ] Multi-session file emits session diagnostics; no silent collapse.
- [ ] Robot/MCP search hits carry session_id + text within budget.
- [ ] `cargo fmt --all --check`, `clippy -D warnings`, `cargo test
      --workspace`, `cargo build --release` green.
- [ ] Gate D full-corpus re-run green.

## Constraints

- Catalog/authority semantics unchanged; FTS stays a rebuildable projection.
- No commit/push without owner authorization.
