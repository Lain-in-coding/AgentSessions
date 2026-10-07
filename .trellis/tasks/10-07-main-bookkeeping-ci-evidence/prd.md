# Main bookkeeping and provider CI evidence closure

## Goal

Make `main` reflect the completed merge-task archive and publish accurate, auditable cross-target provider CI evidence. This is a lightweight documentation/bookkeeping task, not a new product-remediation wave.

## Background

- PR [#22](https://github.com/LainHappy/AgentSessions/pull/22) merged as `fa7a0a92bb22270a522b0b4b3b95a16d4365ae1a`. The pushed `b5d65d9` archive commit remains outside `main` and contains 13 archive renames with only lifecycle metadata changes.
- Main-branch [run 37534613157](https://github.com/LainHappy/AgentSessions/actions/runs/37534613157) of `core-beta-evidence` passed all four target jobs. All four artifacts were downloaded and inspected on 2026-10-07; their archive digests and binary provenance agree with the run metadata and manifests.
- The current matrix still labels `IB-CI-PROVIDER-EVIDENCE-001` as `ci_configured_only` because it records no qualifying run for the provider/open-source-gate steps (`docs/operations/core-beta-evidence-matrix.md:20,39,52`). The new run closes that specific evidence gap, not every provider-certification requirement.
- The user approved the final planning summary and implementation on 2026-10-07, including commit, push and PR merge after applicable checks pass.

## Requirements

### R1 — Complete the existing archive accurately

Land `b5d65d9` on `main`, preserving historical source and evidence contents. The only additional edits allowed inside that historical archive are the two `file` values in `implement.jsonl:2` and `check.jsonl:2`, which currently point to the absent pre-archive PRD. They must resolve to `.trellis/tasks/archive/2026-10/10-07-merge-main-and-land/prd.md`. Evidence and validator behavior are recorded in `research/archive-landing-plan.md`.

### R2 — Record narrowly justified CI verification

Update the matrix's recorded-run section and only the provider row's verification accounting. Name the exact run/commit, review date, four targets (`x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `x86_64-apple-darwin`, `aarch64-apple-darwin`), successful relevant steps, and job/artifact links. Preserve the older run records and their separate installer/build claims.

The correct provider result is **169 passed / 0 failed / 4 ignored per target**; totals across targets are repeated execution occurrences, not distinct tests. Each target has **four passing threshold gate metrics, zero failures, and zero deferred threshold metrics**. Semantic/hybrid observations have no threshold verdict. Exact commands, provenance, counts, integrity checks and caveats are in `research/provider-ci-verification.md`.

### R3 — Preserve evidence and product boundaries

Promotion is to `ci_verified`, not `locally_verified`, provider Beta/GA, or release readiness. Preserve the matrix's status vocabulary, all unrelated rows, hosted-runner/expiry limitations and composite milestone rule (`docs/operations/core-beta-evidence-matrix.md:12-16,81-87`).

The gate uses synthetic fixtures; resume is preview-only; semantic/hybrid measurements use bigram-hash rather than real E5. Do not turn the ignored Rust helpers into an E5 skip count, invent a `skip_real_e5` field for this run, or erase separately deferred real-E5 verification. No minimum-OS, clean-machine, signing, privacy, real-data or upstream-provider-process certification is implied.

### R4 — Deliver a small, reviewable change

Limit writes to the evidence matrix, the historical archive transition/two reference repairs, and this task's planning/research/context/closeout artifacts. Preserve prior hashes and unrelated work. No runtime caches, downloaded binaries, personal paths, credentials or local journals may be committed.

Use independent implementation review, explicit staging, ordinary commits/pushes and a reviewed PR to `main`, without bypassing applicable checks. Task-closeout bookkeeping remains part of this scope; do not silently leave another branch-only archive or create recursive follow-up tasks just to archive this task. Do not mark a path-filtered workflow that did not run as successful.

## Acceptance Criteria

- [ ] **AC1 (R1):** `main` has the completed merge task only at its archive location; content matches `b5d65d9` except the two documented context-path corrections; the archived context validator passes.
- [ ] **AC2 (R2):** the provider row is `ci_verified` and its named-run record resolves to the inspected four-target run, full source SHA, exact jobs/artifacts and bounded test/gate outcomes.
- [ ] **AC3 (R3):** ignored helpers and informational metrics remain distinguished from passes; existing installer/build evidence, maturity/release accounting and deferred real-E5 verification are unchanged.
- [ ] **AC4 (R4):** task context validation, whitespace/link checks, full-diff allowlist inspection and independent review pass. Rust source, workflows, schemas, lockfiles, public APIs and provider capability records have no changes.
- [ ] **AC5 (R4):** all task-owned delivery/closeout changes are committed and pushed, the intended archive/evidence changes are merged into `main`, and the final report names the remote result and actually executed checks without hiding remaining bookkeeping.

## Out of Scope and Deferred Verification

New optimization waves; real E5 execution; large-scale benchmarks; provider capability expansion or owner promotion decisions; dependency upgrades; minimum-OS or clean-machine certification; signatures/notarization; rewriting history; publishing releases; and changes to Trellis runtime/configuration. Historical artifact retention ends seven days after the run; this task preserves a sanitized research record, not the raw archives or a new certification level.

## Planning Artifacts

PRD-only is sufficient for this bounded task; no `design.md` or `implement.md` is required. The execution order and Git/CI constraints are in `research/archive-landing-plan.md`; artifact-level findings are in `research/provider-ci-verification.md`. Both `implement.jsonl` and `check.jsonl` contain real curated spec/research entries. No product or historical-archive implementation edits have been made during planning.
