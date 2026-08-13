# Implement — UX review fixes

Ordered checklist. Each step ends in a green check; do not proceed past a
red one.

## Phase A — Application-layer plumbing (R1/R4)

1. `agent-session-grep-ports`: add `get_many(&self, ids) -> Vec<(StableId, Option<Vec<u8>>)>` to the catalog port.
2. `adapters-sqlite`: implement `get_many` with the existing variable-limit
   chunking (≤500 params per IN batch).
3. `testkit`: implement `get_many` on `InMemoryStore`.
4. `application`: reject U+0000 / C0-C1 controls in search query →
   `invalid_request`; unit test.
5. `application`: generate snippets during search assembly — batched
   `get_many`, per-item `max_snippet_chars` cap, total snippet bytes into
   `max_response_bytes` clamp; explicit truncation fields. No redaction
   (owner decision, ADR-0004).
6. `cli/main.rs`: delete `attach_search_snippets`; wire the new
   application snippets; human renderer prints the snippet line only.
7. Verify: `cargo test -p agent-session-grep-application -p agent-session-grep-adapters-sqlite`.

## Phase B — Privacy-safe errors (R2)

9. `cli/main.rs` get/show: fixed generic not_found message (no id echo).
10. `cli/main.rs` sync directory rejection: path-free, platform-neutral.
11. Update/extend e2e: robot + human-stderr assertions that error bodies
    contain neither id nor path.
12. Verify: `cargo test -p agent-session-grep-cli --test e2e`.

## Phase C — Help/version envelope (R3, ADR-0006)

13. `cli/main.rs`: early help/version interception stage after global flag
    scan, before db parse / store open / special dispatch; covers
    top-level `--help`/`--version` and `<cmd> --help|-h` for all known
    subcommands incl. `index` / `index rebuild`.
14. Machine modes: success envelope with `data.help_text` / `data.version`;
    jsonl single frame; request-id echo.
15. e2e: no-db help works; no file created on help with new db path;
    envelope shape per mode.
16. Verify: `cargo test -p agent-session-grep-cli`.

## Phase D — operator_action (R5)

17. `cli/protocol.rs`: per-code action mapping from the catalog
    `operator_action` column.
18. Unit test: every code has an action; cursor/snapshot/generation match
    catalog semantics.
19. Verify: `cargo test -p agent-session-grep-cli`.

## Phase E — Renderer (R6)

20. `cli/human.rs` show: msg curated / ses+doc generic kv fallback.
21. `cli/human.rs`: dedicated `render_ingest`.
22. `cli/human.rs` context truncation hint prints real wire id.
23. Unit tests per entity kind; update existing snapshots.
24. Verify: `cargo test -p agent-session-grep-cli`.

## Phase F — Parser robustness (R8)

25. Value-flag guard: `--db`/`--request-id`/`--output` reject missing value
    or value-that-is-a-flag.
26. Reject duplicate/conflicting global flags.
27. `doctor`/`config` reject unknown positionals.
28. `command_name` reflects the failing command.
29. e2e matrix additions; assert no accidental file creation.
30. Verify: `cargo test -p agent-session-grep-cli --test e2e`.

## Phase G — Contract migration (R7)

31. `scripts/install/smoke.ps1` + `smoke.sh`: exit-4 not_found assertions;
    remove stale comments.
32. Run smoke.ps1 locally against the release binary; `bash -n` smoke.sh.

## Phase H — Docs (R9)

33. SKILL.md exit-3 removal; INSTALL doctor wording; sync help wording;
    Obsidian tutorial refresh; stale main.rs comments.

## Phase I — Full gates

34. `cargo fmt --all --check`
35. `cargo clippy --workspace --all-targets -- -D warnings`
36. `cargo test --workspace`
37. `cargo build --release`
38. Gate D full-corpus re-run on the official catalog.

## Rollback points

After each phase: the work is uncommitted but logically separable; if a
phase destabilizes, revert that phase's edits only.
