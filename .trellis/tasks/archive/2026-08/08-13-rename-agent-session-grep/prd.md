# Rename project to agent-session-grep (deep rename)

## Goal

Rename the project from `agentsessions` / `AgentSessions` to
`agent-session-grep` across every layer: workspace, crates, binary name,
CLI surface, internal identifiers, docs, CI, and scripts. The rename must
be complete and consistent — no stale `agentsessions` reference may remain
in tracked files (except historical git history and archived Trellis task
dirs, which are not rewritten).

## Context

- Owner decision (2026-08-12, Q5/Q7): rename to `agent-session-grep`,
  deep scope. The name already appears in docs as the search tool
  (`docs/operations/REUSE-LICENSE-AUDIT.md`, `spikes/search-backend/SPIKE-CARD.md`,
  `docs/adr/ADR-0001`).
- Executes immediately after the performance task (08-10) closes — it has
  closed (Gate D green 2026-08-13, acceptance criteria 1/6 done).
- After this task completes and is accepted, the owner self-tests, then
  open-source release preparation begins (README/LICENSE/audit) — a
  follow-up task.

## Requirements

- R1 Workspace and crate names: `agentsessions-*` → `agent-session-grep-*`
  for all 8 crates (domain, ports, application, adapters-sqlite,
  provider-claude, provider-codex, cli, testkit).
- R2 Binary name: `agentsessions` → `agent-session-grep` (Windows
  `agentsessions.exe` → `agent-session-grep.exe`).
- R3 Internal identifiers: crate paths in `use` statements, `Cargo.toml`
  package names, test imports, `include_str!` paths — everything that
  references the crate names.
- R4 CLI surface: any user-visible command name, help text, or error string
  referencing `agentsessions`/`AgentSessions` becomes the new name.
- R5 Docs: all `.md` files under `docs/`, CONTRIBUTING.md, AGENTS.md,
  CONTEXT.md. (A root README is created later in the open-source-prep
  task; references in it, if any, use the new name.)
- R6 CI: `.github/workflows/*.yml` — binary names, cargo package selectors
  (`-p agentsessions-cli`), artifact names, smoke-test paths.
- R7 Scripts: `scripts/evidence/real_data_regression.py` (binary default,
  messages), `scripts/install/*` (install/uninstall binary name),
  `scripts/evidence/*.py` references.
- R8 No schema change: data-root layout, DB file names, and stable IDs are
  NOT renamed (data compatibility). `writer.lock`, `CURRENT`, `*.db` stay.
- R9 All quality gates green after the rename: fmt/clippy/test/deny/release
  build/harness tests.

## Acceptance Criteria

- [x] `rg -i "agentsessions" --glob '!target/**' --glob '!.git/**'` over
      tracked files returns zero matches (except .trellis/archived task
      dirs and git history). (Remaining 6 hits are deliberate
      data-compat/history preserves, check-verified.)
- [x] `cargo build --locked --release` produces `agent-session-grep` /
      `agent-session-grep.exe` binary.
- [x] `cargo test --workspace` all green; harness unit tests green.
- [x] CLI `--version` and `--help` print the new name; no stale string in
      output.
- [x] CI workflows reference only the new name.
- [x] Docs/scripts/AGENTS.md/CONTRIBUTING.md/CONTEXT.md carry no stale name
      (except .trellis/archived).
- [x] Data compatibility: an existing v7 data root still opens and serves
      queries with the renamed binary (spot-check on a copy).
      (DONE: 1,484 MB real store — status/list/search all correct.)
- [x] Full-corpus Gate D still passes with the renamed binary (re-run).
      (DONE 2026-08-13: all six invariants PASS, 1,330 sources / 1.255 GB,
      outcome passed.)

## Constraints

- Do NOT rewrite git history (no rebase/filter-branch); archived Trellis
  task dirs under `.trellis/tasks/` are left as historical records.
- Do NOT rename stable IDs, data-root paths, or DB file names (data
  compatibility with existing installs).
- Repository stays private; no push/public.
- No feature changes; this is a mechanical rename.
