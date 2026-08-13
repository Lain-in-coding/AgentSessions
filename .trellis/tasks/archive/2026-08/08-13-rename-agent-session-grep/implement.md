# Implementation plan — Rename to agent-session-grep

## 1. Workspace manifest (root Cargo.toml)

- [ ] Rename `[workspace]` member paths `crates/agentsessions-*` →
      `crates/agent-session-grep-*`; rename the crate directories.
- [ ] Rename `[workspace.dependencies]` keys (e.g. `agentsessions-domain` →
      `agent-session-grep-domain`).
- [ ] Version stays 0.1.0 (not part of this task).

## 2. Crate manifests (8 × Cargo.toml)

- [ ] Each crate: `name`, `[lib] name` (if set), `[dependencies]`
      references to sibling crates, `[dev-dependencies]` (testkit).
- [ ] The cli crate: `[[bin]]` name → `agent-session-grep` if set, or
      default derived from package name.

## 3. Rust source (26 files mentioning the name)

- [ ] `use` paths: `agentsessions_ports::` → `agent_session_grep_ports::`
      (crate names in Rust are snake_case: `agentsessions-domain` → package
      `agent-session-grep-domain`, lib name `agent_session_grep_domain`).
- [ ] `include_str!` / `include_bytes!` paths if they reference the name.
- [ ] Test modules referencing crate names.
- [ ] Any string literals containing `agentsessions` (help text, error
      messages) → new name.

## 4. CLI surface

- [ ] `--version` output (crate version string is auto, but any hardcoded
      name), `--help` text, human/robot envelope fields if they carry the
      name.
- [ ] Default data-root path if it embeds the name (verify: config paths
      use platform dirs + `agentsessions`? — keep data compatibility,
      see §7).

## 5. Docs and markdown (357 files)

- [ ] `docs/**/*.md`, `CONTRIBUTING.md`, `AGENTS.md`, `CONTEXT.md`,
      root-level docs.
- [ ] Do NOT touch `.trellis/archived/**` (historical records) and
      `.trellis/tasks/**` completed task dirs (except the current two).

## 6. CI workflows

- [ ] `.github/workflows/ci.yml`: `-p agentsessions-cli` →
      `-p agent-session-grep-cli`, binary names in smoke tests
      (`./ci-prefix/agentsessions` → `agent-session-grep`,
      `.exe` variant), artifact names.
- [ ] `.github/workflows/core-beta-evidence.yml`: binary names, `-p`
      selectors.

## 7. Scripts

- [ ] `scripts/evidence/real_data_regression.py`: default binary basename,
      docstring, messages.
- [ ] `scripts/install/*` (smoke.ps1/smoke.sh, install/uninstall): binary
      name.
- [ ] `scripts/evidence/*.py` references.
- [ ] Verify data-root path logic unchanged (data compatibility).

## 8. Verification

- [x] `rg -i "agentsessions" --glob '!target/**' --glob '!.git/**'` over
      tracked files → zero matches (except archived/current task dirs).
      (Remaining hits: operation_digest separator, data-root platform
      paths, one real-log line, .trellis spec/config/workflow — all
      deliberate data-compat/history preserves, check-verified.)
- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `cargo deny check`
- [x] `cargo build --locked --release -p agent-session-grep-cli` → produces
      `agent-session-grep(.exe)`
- [x] `python -m unittest scripts/evidence/test_real_data_regression.py`
- [x] `agent-session-grep --version` (`agent-session-grep-cli 0.1.0`) /
      `--help` show new name; zero stale references
- [x] Data compat spot-check: copy a v7 data root, open with renamed
      binary, run `search`/`list` (owner self-test — no local real root)
      (DONE 2026-08-13: renamed binary opened the real 1,484 MB v7 store —
      status 165,882 catalog / generation 1323 / 178,528 placements; list +
      search both return correct results, zero errors.)
- [x] Full-corpus Gate D re-run with renamed binary (all six invariants)
      (DONE 2026-08-13 08:26: `agent-session-grep.exe` (v0.1.0,
      sha256 42331907...) → all six invariants PASS, outcome passed on
      1,330 sources / 1.255 GB / 180,718 emitted, 0 skipped; 242 sessions;
      630/630 byte spans; rebuild 166,882 → 166,882 ids match. Zero
      functional regression from the rename.)

## 10. Full-repo review fixes (added 2026-08-13, owner-grilled scope)

- [x] Five review agents (domain/ports/application, adapters-sqlite,
      providers/testkit, cli, docs) read every line; 2 major + ~48 minor
      findings. Owner decisions: fix everything (Q1 a, Q6 a+c, Q8 a),
      including perf minors; domain-parallel implement agents (Q9 b);
      logical commit sequence (Q10 c).
- [x] Major-1: FTS projection now derived via `searchable_text(merged
      payload)` for merged entities (single source of truth with rebuild).
- [x] Major-2: `--help`/`--version` route through `write_stdout_line`
      (EPIPE-safe, exit 0, no panic).
- [x] All ~48 minors fixed across 4 domains; full workspace gates green
      (fmt/clippy/test).
- [x] Docs P1+P2+P3 all updated (8 files): Gate D evidence, runbook v7,
      INSTALL v7, harness retry, perf baseline, merge semantics.
- [x] Ultimate validation: full-corpus Gate D v5 with review-fixed binary
      (sha256 481d8c98...) → **all six invariants PASS**, 1,340 sources /
      1.267 GB / 182,886 emitted, 0 skipped; 242 sessions, 0 internal;
      630/630 byte spans; rebuild 169,060 → 169,060 ids match.
      outcome passed (2026-08-13T01:46:55Z).

## 11. Review and rollback points

## 9. Rollback

- Individual commits per logical unit (workspace → crates → src → docs →
  CI → scripts); revert per commit. No schema change, so rolling back a
  commit fully restores the old name.
