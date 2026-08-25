# Implement — Core usability

Phases; each ends green before the next.

## Phase A — CJK bigram (R1)
1. `bigram_cjk` transform + unit tests (Han ranges, mixed ASCII/CJK, empty,
   single-char, punctuation runs).
2. Wire into `SearchIndex::index` text and the query path (before
   `safe_fts_query`).
3. `index rebuild` smoke on a fixture; assert generation bumps and old
   cursor expires.
4. CJK recall fixtures: 2-char query hits longer sentences; no ASCII/path
   regression on the literal table.
5. `cargo test -p agent-session-grep-adapters-sqlite -p agent-session-grep-application`.

## Phase B — Probe tolerance (R2)
6. Probe: tolerate ≤3 non-JSON lines in the sample window (degrade
   confidence, keep adapter); parse's skip-with-diagnostics path already
   exists.
7. Rejection path: line-numbered detail + repair-direction message.
8. Golden fixtures (both providers, intentional broken line) sync green;
   fatal-source test asserts line diagnostics.

## Phase C — Merged-file diagnostics (R3)
9. Post-parse multi-session diagnostic (count + bounded ids); sync help/
   INSTALL wording.
10. Tests: multi-session file emits diagnostic; single-session unaffected.

## Phase D — Search-hit session context (R4, ADR-0008)
11. Port: `session_of(message_ids)` batch lookup (or reuse placements
    get_many); sqlite + testkit impls.
12. `SearchHit` gains session_id + text; byte accounting in clamp_items.
13. CLI robot/json/jsonl + MCP `search_sessions` serialize additive fields;
    human unchanged.
14. e2e: robot+MCP hits carry session_id/text within budget; no field
    removals.

## Phase E — Full gates
15. fmt / clippy -D warnings / test --workspace / build --release.
16. Gate D full-corpus re-run on the official catalog.

## Rollback
Per-phase revert is independent; FTS changes are rebuild-only.
