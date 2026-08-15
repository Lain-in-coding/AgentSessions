# Implement — Competitor borrowings P0

## Parallel development

Implement the four child tasks in separate worktrees rooted at `fc2f2d4`. Each child reads its `implement.jsonl`, `prd.md`, `design.md`, and `implement.md`, changes only its declared scope, runs focused tests, and does not commit or push.

## Integration sequence

### Phase A — Provider and time filters

1. Add normalized filter types and extend the search port/request without changing omitted-filter behavior.
2. Parse CLI provider/absolute/compact time inputs and MCP absolute inputs; reject invalid ranges/providers.
3. Push predicates into SQLite FTS query execution and bind normalized filters into cursor digests.
4. Add port/application/SQLite/CLI/MCP unit and end-to-end tests.
5. Run focused crate tests, formatting, and Clippy.

### Phase B — Message and around

6. Add typed placement/session resolution and a reusable mainline window primitive.
7. Add application request/response and MCP `get_message` schema/handler.
8. Enforce explicit ambiguity, anchor retention, chronological order, and response budgets.
9. Add unique-session, multi-session, edge-window, `around=0`, and truncation tests.
10. Run focused crate tests, formatting, and Clippy.

### Phase C — Context levels and hints

11. Extend context requests with the level enum while preserving the omitted/raw path.
12. Build structural talks/session summaries over typed context data.
13. Add one-way fallback plus bounded requested/effective level and hint fields.
14. Charge summary/hint bytes and test raw compatibility, each level, fallback, and truncation.
15. Run focused application/CLI tests, formatting, and Clippy.

### Phase D — Search guidance

16. Add deterministic literal match explanation and bounded next-command generation to search-hit assembly.
17. Target the integrated `get_message` schema and include `session_id` when available.
18. Preserve human output, ranking, cursor order, and default search behavior.
19. Add machine/JSON/JSONL/MCP tests and byte-budget regression coverage.
20. Run focused application/CLI tests, formatting, and Clippy.

## Integration review

21. Integrate child diffs in Phase A-D order. Resolve overlapping files hunk-by-hunk; never replace a whole integrated file with a child copy.
22. Run `cargo fmt --all --check`.
23. Run `cargo clippy --workspace --all-targets -- -D warnings`.
24. Run `cargo test --workspace`.
25. Run `cargo build --release`.
26. Re-run Gate D on the authorized full corpus and record only aggregate/privacy-safe evidence.

## Rollback points

- After each phase, the integrated tree must be green before the next child is applied.
- If a port shape prevents a later child, revise the shared typed DTO rather than adding a parallel code path.
- If filtered search requires a schema migration or changes ranking semantics, stop that child and return to design.
- No commit or push until the integrated review is complete and the owner explicitly authorizes it.
