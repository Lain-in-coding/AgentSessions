# Resume protocol prerequisites — 2026-08-14 audit convergence

## Quality gate status

Full workspace gate GREEN (in `integration-08-13-four-features-v2` worktree,
`CARGO_TARGET_DIR=C:/AgentSessions/target-resume-integration`):

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets -- -D warnings` — pass
- `cargo test --workspace` — pass (SQLite 135, Application 133, CLI 181,
  Claude 38, Codex 33, MCP 37, E2E 89, others green)
- `cargo build --release` — pass

## Defects fixed this round

### `installation_namespace` cross-file split (production)

- **Before**: unknown-provider fallback used `provider_id:file` (the source's
  filename), so two files from one installation declaring the same native
  Session received different canonical `ses_v1_*` IDs. The split-file E2E
  tests (`session_split_across_files...`, `shared_message...`, `session_synced
  _in_separate_invocations...`) broke.
- **Fix** (`crates/agent-session-grep-cli/src/main.rs:1549`):
  - claude-code/codex branch now uses the full prefix path through and
    including the marker (`.claude`/`.codex`), e.g.
    `claude-code:C:/users/x/.claude`.
  - fallback branch now uses the source's parent directory (e.g.
    `synthetic:C:/fixtures`), grouping sibling sources.
- **Regression tests added**: `installation_namespace_groups_sources_by_provider_root`,
  `installation_namespace_fallback_groups_sibling_sources`.
- **Renamed**: `fallback_message_and_placement_ids_are_path_independent` →
  `fallback_message_id_is_path_independent_but_placement_is_installation_scoped`;
  assertion flipped: Message ID stays path-independent, Placement ID is now
  installation-scoped (correct scoped semantics).

### Stale scoped-ID E2E tests

- 16 CLI E2E + 12 MCP E2E failures all compared legacy
  `ses_v1_<provider-native>` to the new scoped canonical ID, or issued
  `context`/`get-message` with the legacy ID.
- **Fix**: added `session_wire_for_message(db, message_wire)` helper in both
  `tests/e2e.rs` and `tests/mcp_e2e.rs` that resolves the canonical Session
  from `message_placements` after ingest. Fixtures now return anchor Message
  IDs; tests resolve canonical Session IDs from placements.
- `migrated_v6_catalog_stays_readable...`: keeps `legacy_session_wire` for
  pre-reingest checks; resolves new canonical `session_wire` after reingest;
  asserts `session_wire != legacy_session_wire`.

### Human snippet assertion (stale UX contract)

- `snippet_renders_in_human_search_but_is_stripped_in_machine_modes` asserted
  Human search prints the message body. Product decision froze Human search to
  the five-column session table.
- **Fix**: updated fixture to carry `timestamp` + `sessionId`; assertion now
  checks the frozen table header + a real Provider row + real date
  (`2026-07-26`) + real title (`snippet vis`).

### Clippy `cloned_ref_to_slice_refs`

- Three call sites used `&[x.clone()]`; replaced with `std::slice::from_ref(&x)`.

### CLI discoverability

- Added `get-session-resume` to top-level help text, `known_subcommand`, and
  `subcommand_help_text`. `KNOWN_COMMANDS` bumped 14 → 15.
- `skills/agent-session-grep/SKILL.md` synced to schema `1.1`, 8 MCP tools,
  `get_session_resume` CLI command, two-ID contract note, cursor result-set
  note. Removed stale `list_sessions` interleaved-IDs caution.

### Human table real date + title (Step 1 of roadmap)

- Added `SqliteStore::latest_activity_ymd_for_sessions`
  (`crates/agent-session-grep-adapters-sqlite/src/lib.rs`): batched
  `MAX(json_extract(catalog.payload, '$.timestamp'))` per canonical Session,
  truncated to `YYYY-MM-DD`, chunked via `BATCH_IN_CHUNK` (no N+1).
- `attach_session_resume_rows` (`crates/agent-session-grep-cli/src/main.rs:1435`):
  日期 from that query; 会话标题 from the highest-relevance hit `text` on the
  current page (preserves search ordering). Missing → `—`.
- No port/DTO/schema change; Robot/MCP output unchanged (Human-mode only).

## Confirmed open debt (RFC-0001 §5.1 — identity migration)

The `identity-migration-audit` confirmed the domain layer
(`StableId::native_session_scoped`, `SessionIdentityNamespace`) is correct.
Gaps are composition-root only:

1. `installation_namespace` derives from absolute path (violates §5.1
   "不从当前绝对路径直接派生"). No relocation invariance.
2. No `id_alias` table — old `ses_v1_<native>` IDs deleted on reingest;
   external bookmarks break (violates §5 line 200/204).
3. No path normalization (Windows `C:` vs `c:` splits one installation).
4. `source_session_resume_claims` keyed by `source_path` — file move
   orphans claims.
5. Stability tier stays `Native` even when namespace is unstable
   (violates "honest" contract).

**Minimal correct design (follow-up task)**: persist an
`installation_namespaces` registry table (path heuristic → stable integer ID),
add an `id_alias(old_id, new_id)` table with TTL, normalize path case on
Windows, key resume claims by `(namespace_registry_id, session_id)`. These
are tracked as a separate task, not blocking the current Resume contract.

## Audit agents still running (at time of write)

- `sqlite-resume-audit` — adversarial check of claims constraints/transactions.
- `privacy-contract-audit` — frozen contract + no-leak verification.

Both target the same worktree; conclusions will be appended.

## Audit conclusions (appended)

### `privacy-contract-audit` — ALL PASS

All five privacy/contract checks passed (frozen 6-field shape, no
transcript/source path exposure, resume metadata isolation from FTS/payload/
diagnostics/progress/errors, no Provider command construction/execution, no
terminal/cd/copy). No defects; no action required.

### `sqlite-resume-audit` — 9 test-coverage gaps (no code defects)

The audit confirmed the claims constraints, transactionality, and conflict
semantics are correct. It proposed 9 additional test-coverage gaps. Three of
these cover PRD R1 acceptance criteria and have been added this round:

1. **`resume_of_hides_cwd_when_pair_not_observed`** — Provider Session ID
   resolved but `pair_observed=false` returns cwd `None` while
   `resume_available` stays true. Asserts the pair-association privacy rule
   at the port layer (`lib.rs:580` `resume_metadata_from_claim`).
2. **`resume_of_empty_input_returns_empty_without_query`** — Empty input
   short-circuits to an empty Vec with zero SQL statements (verified via
   `counted_statements`). Guards against full-table scans on degenerate input.
3. **`resume_claims_indexed_by_session_id_without_full_scan`** — `EXPLAIN QUERY
   PLAN` asserts the `session_id` lookup uses the
   `source_session_resume_claims_session` index, not a full scan.

The remaining 6 gaps (claim insert rollback on partial batch failure,
membership rejection, delete guard, rebuild preservation, post-migration
backfill, identical multi-source claims) are partially covered by the
existing `resume_claim_written_replaced_and_cleared_atomically_with_source`
test; full coverage of each is tracked as follow-up hardening.
