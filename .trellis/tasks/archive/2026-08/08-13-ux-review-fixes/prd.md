# UX review fixes (privacy, robot contract, parser robustness)

## Goal

Fix every finding introduced by the 2026-08-13 newbie-UX diff plus the
parser-robustness batch, per the 2026-08-13 multi-agent review and the
owner-grilled decisions Q1-Q7 (CONTEXT.md decision log 2026-08-13;
ADR-0003..ADR-0006).

Out of scope (owned by `08-13-hardening-backlog`, which starts after this
task lands): perf P1s (changed-sync O(n), context full-load, v7 read-open
backfill, deep pagination), MCP pre-existing contract gaps (initialize
gate, jsonrpc validation), evidence/install script defects
(real_data_regression retry classification, install.ps1 old-prefix
migration, core_beta_benchmark schema id).

## Requirements

### R1 Privacy — snippet budget (ADR-0004, owner decision: no redaction)

- R1.1 Human search snippets keep working as-is — no secret redaction
  (local-first tool; on-screen secret display is accepted risk).
- R1.2 Snippets obey the ResponseBudget: per-item `max_snippet_chars`
  (currently unused, becomes live) and total snippet bytes count toward
  `max_response_bytes`; truncation is explicit, never silent.
- R1.3 Snippet payloads are loaded in batched catalog reads (chunked IN) —
  no per-hit N+1.

### R2 Privacy — error message redaction

- R2.1 `get`/`show` not_found: exit 4 + `error.code == "not_found"`;
  message is generic and never echoes the wire/native ID.
- R2.2 `sync` directory rejection: message is path-free and
  platform-neutral (no PowerShell-only example).

### R3 Robot/JSON contract — help and version (ADR-0006)

- R3.1 Known `<cmd> --help` is handled before `--db` parsing, store open,
  and special-command dispatch: `search --help` works without `--db`;
  help has no file side effects; all subcommands listed in the help text
  are reachable, including `index` and `index rebuild`.
- R3.2 Machine modes (json/jsonl/robot): `--help`/`--version` exit 0 with a
  success envelope; help text or version string in `data`; jsonl is a
  single frame; `--request-id` echoed.
- R3.3 help/version exit code is mode-independent (always 0).

### R4 Search semantics and hardening (ADR-0003)

- R4.1 Keep `safe_fts_query` literalization; add table-driven tests pinning
  the plain-text semantics (AND/OR/NOT/NEAR/phrase/prefix treated as
  literal tokens).
- R4.2 NUL (U+0000) and control characters are rejected at the Application
  boundary as `invalid_request` — never sanitized by deletion (which could
  splice tokens).
- R4.3 Raw backend/FTS parser detail never reaches CLI/MCP callers; user
  message is generic, internal detail is bounded and diagnostic-only.

### R5 Error operator actions

- R5.1 `protocol.rs::operator_action` becomes a per-canonical-code mapping
  matching `schemas/robot/v1/error-catalog.json` semantics: cursor codes →
  discard cursor and re-run the query; `snapshot_failed` → non-retryable
  filesystem/metadata inspection; `generation_mismatch` → re-run the query
  from the first page (index advanced); etc.
- R5.2 Unit test asserts each code has an action and that the three codes
  above match catalog semantics (drift guard).

### R6 Human renderer correctness

- R6.1 `show`: `msg_v1_` gets the curated projection; `ses_v1_`/`doc_v1_`
  fall back to generic key/value rendering (no `?` fields, no fabricated
  `context <session>` hint for non-messages).
- R6.2 `ingest`: dedicated renderer — no `sources: ?`; keeps `variant`,
  `source_fp`, `committed`, `diagnostics`.
- R6.3 `context` truncation hint includes the actual message wire id.

### R7 Missing-entity contract migration (ADR-0005)

- R7.1 `scripts/install/smoke.ps1` and `scripts/install/smoke.sh` assert
  exit 4 + `ok:false` + `error.code == "not_found"` for absent `get`;
  stale "only context carries not_found" comments removed.

### R8 Parser robustness

- R8.1 Value flags (`--db`, `--request-id`, `--output`) reject a missing
  value or a value that is itself a known flag → `invalid_request`; no
  accidental file creation (e.g. `--db --robot status` must not create a
  file named `--robot`).
- R8.2 Duplicate/conflicting global flags are rejected (no silent
  first-wins for `--output`/`--robot`, no silent last-wins for `--db`,
  duplicate `--request-id` rejected).
- R8.3 `doctor`/`config` reject unknown positional tokens instead of
  silently dropping them (no misleading success / `db:not-checked`).
- R8.4 The envelope `command` field reflects the command that actually
  failed.

### R9 Documentation sync

- R9.1 `skills/agent-session-grep/SKILL.md`: remove the exit-3 row (no
  such exit code).
- R9.2 `docs/operations/INSTALL-AND-UPGRADE.md`: doctor without `--db`
  reports environment/version + `db:not-checked` + actionable hint.
- R9.3 sync help + directory error wording: accepts multiple files, no
  directories; platform-neutral expansion guidance.
- R9.4 Obsidian tutorial (`agent-session-grep 自测详细步骤教程与使用文档.md`):
  flag-position semantics (post-command `--robot` is a literal query;
  `<cmd> --help` is subcommand help), snippet line in the human search
  example, current pagination wording.
- R9.5 Stale comments in `main.rs` (the `search --help` literal note).
- R9.6 THREAT-MODEL §5/§6.1 and R0-ARCHITECTURE-REVIEW aligned with
  ADR-0004 (no redaction). Done during planning; re-verified at check time.

### R10 Tests

- R10.1 Unit: FTS literal-semantics table; NUL
  rejection; per-code operator actions; snippet budget accounting.
- R10.2 e2e: machine-mode help/version envelopes; help without db; no
  id/path leakage in error envelopes (robot + human stderr); parser matrix
  additions; renderer tests per entity kind; smoke-parity assertions.
- R10.3 Every new/updated test asserts the review contract, not merely the
  implementation.

## Acceptance Criteria

- [ ] R1-R10 implemented with tests; no new clippy warnings.
- [ ] `cargo fmt --all --check` green.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` green.
- [ ] `cargo test --workspace` green.
- [ ] `cargo build --release` green.
- [ ] Install smoke green where runnable locally (smoke.ps1 executed;
      smoke.sh at least `bash -n` clean).
- [ ] Gate D full-corpus re-run green on the official catalog.
- [ ] Docs, CONTRACT, ADRs, CONTEXT.md consistent with behavior.
- [ ] No commit/push without owner authorization.

## Constraints

- Repository stays private; provider transcripts remain read-only.
- Robot/MCP envelope shape: no field removals; additions only where the
  schema permits (help/version `data`).
- No new production features beyond the fixes listed above.
- A parent/child split was considered and rejected: all fixes land in the
  same five files and share one gate sequence; child tasks would add
  ceremony without independent verifiability.
