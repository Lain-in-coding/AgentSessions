# Design — Core usability

## 1. CJK bigram transform (R1, ADR-0007)

- New `cjk.rs` in `agent-session-grep-application` (or `adapters-sqlite` if
  the index path owns it): `fn bigram_cjk(s: &str) -> String`. Scan chars;
  a CJK run (Unicode Han ranges) of length N becomes N-1 bigrams joined by
  single spaces; non-CJK runs pass through verbatim.
- Applied at the two FTS boundaries: (a) `SearchIndex::index` text before
  insert; (b) the query before `safe_fts_query` (transform then literalize —
  order matters: bigram output contains spaces that are the token
  separators). The digest for cursor binding stays on the raw user query
  (unchanged contract).
- Migration: `index rebuild` path already re-derives index text from catalog
  payloads — no schema change, no catalog migration. Generation bump on the
  rebuild invalidates old cursors (documented behavior).
- Growth: worst case ~2× tokens for pure-CJK text; measured and documented
  in the rebuild test.
- Single-char CJK queries ("了") remain weak — known boundary, documented,
  no action this round.

## 2. Probe tolerance (R2)

- The probe sample window (first N lines) currently fails the whole source
  on any unparseable line. Change: count non-JSON lines in the window; if
  below a tolerance (e.g. ≤3), degrade confidence by one tier instead of
  AmbiguousVariant; the parse phase already skips broken lines with
  diagnostics, so ingest then proceeds.
- Rejection path keeps line numbers: the probe/parse error gains a
  `line` detail (bounded, in `details`), and the human/robot message says
  "第 N 行不是有效 JSON" with repair direction.
- Golden `basic.jsonl` ships an intentionally broken line — becomes the
  regression fixture (both providers).

## 3. Merged-file diagnostics (R3)

- Parse already sees every record's sessionId. Add a post-parse diagnostic
  when >1 distinct sessionId appears in one source: report count + ids
  (bounded), keep the existing first-session assignment behavior, document
  single-file=single-session in sync help/INSTALL.

## 4. Search-hit session context (R4, ADR-0008)

- Application search assembly already resolves hit ids; add a batched
  placement→session lookup (`get_many` on placements, or a dedicated
  `session_of(message_ids)` port call — prefer the latter, one query).
- `SearchHit` gains `session_id: Option<String>` and `text: Option<String>`
  (the summary, clamped by `max_snippet_chars`); bytes charged in the same
  `clamp_items` est as snippets.
- CLI robot/json/jsonl and MCP `search_sessions` serialize the two fields
  (additive); human renderer unchanged.

## Rollback

Each R lands as one logical change; reverting any R is independent. FTS
re-projections are rebuild-only — no catalog writes.
