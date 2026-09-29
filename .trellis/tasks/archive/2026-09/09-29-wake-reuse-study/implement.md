# Implementation and validation

Approval: latest proposed plan accepted by the user with "Implement the plan." on 2026-09-29. The following artifacts persist that plan; no additional product scope is introduced.

## Ordered work

1. [x] Create isolated worktree at baseline and task; verify existing entry-worktree changes are untouched.
2. [x] Clone/check out the fixed Wake commit under the requested ignored source directory; verify LICENSE and clean status.
3. [x] Curate context, activate this research task and dispatch three disjoint trellis-implement workers.
4. [x] Complete report/source coordinates while workers implement snippet, search and provider probes; workers run only bounded tests/smoke.
5. [x] Review each worker's diff and run focused tests. Run unmodified baseline and full-scale measurements serially; retain actual failures/timeouts as evidence.
6. [x] Assemble and validate Chinese report, reuse matrix, raw results, provenance and follow-up ranking.
7. [x] Run the check pass in the main session (sub-agent dispatch returned a provider quota error twice): independent experiment validators, tamper-rejection tests, replay validation, product-scope diff, privacy scan, and Wake hash/blob verification all passed; no research-only findings remained open.
8. [x] Spec-sync decision recorded in the research report: no `.trellis/spec/` or formal product-doc promotion in this research scope.
9. [x] Verified product-scope diff empty vs `2b8f895`, Wake checkout clean at the pinned commit, `Github_src/Wake` ignored by the entry repository, and only this task directory is dirty. Commit only approved research artifacts after the finish-work commit gate; do not push/merge PR #12.

## Validation commands

- `python -B -m unittest discover -s scripts/evidence -p test_core_beta_benchmark.py -v`
- Each independent Rust crate: `cargo fmt --manifest-path <experiment>/Cargo.toml --check`, `cargo clippy --manifest-path <experiment>/Cargo.toml --all-targets --all-features --offline -- -D warnings`, `cargo test --manifest-path <experiment>/Cargo.toml --all-features --offline` (generate and retain the experiment lock before locked replay).
- Provider probes: Python unittest discovery scoped to that experiment plus its report validation CLI.
- Search full-run command and JSON validators are documented in the search experiment README; no home autodiscovery or default product database is permitted.
- `git diff --check`; compare all `crates/`, `schemas/`, root Cargo manifests/lockfile and existing product documentation/scripts against baseline.

## Completion evidence

Use task-local JSON/Markdown results, not journals, as authority. Never claim all tests/experiments passed when a report has incomplete status. Record limits transparently. Keep large generated artifacts ignored and outside task tracked results. Independent review must inspect result/prototype claims as well as code.
