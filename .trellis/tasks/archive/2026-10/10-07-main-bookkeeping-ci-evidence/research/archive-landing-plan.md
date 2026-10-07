# Archive landing and validation plan

## Verified baseline (2026-10-07)

- PR [#22](https://github.com/LainHappy/AgentSessions/pull/22) is merged; its merge SHA is `fa7a0a92bb22270a522b0b4b3b95a16d4365ae1a` and its final head is `956f6a3afb5fc536e45f0081fe2295b0d5bbb8dd`.
- `git fetch origin` followed by `git rev-list --left-right --count HEAD...origin/main` returned `1 1`. The only branch-only commit is `b5d65d9`.
- `git diff --find-renames --name-status origin/main HEAD` reports only the 13 merge-task archive renames. Twelve are byte-identical; `task.json` changes only `status` to `completed` and `completedAt` to `2026-10-07`.
- `git merge-tree --write-tree HEAD origin/main` succeeded without conflicts and returned tree `d0bedbed5192da842eb3fabbff869eee815e3e76`. This is a planning preview, not a branch merge or implementation.

## Archive reference defect (bounded bookkeeping fix)

The historical archive is present, but both archived context manifests still name the old, absent PRD path:

- `.trellis/tasks/archive/2026-10/10-07-merge-main-and-land/implement.jsonl:2`
- `.trellis/tasks/archive/2026-10/10-07-merge-main-and-land/check.jsonl:2`

The old `.trellis/tasks/10-07-merge-main-and-land/prd.md` path does not exist; the corresponding archive path does. The current validator tests literal `repo_root / file_path` existence (`.trellis/scripts/common/task_context.py:167-175`); it does not resolve archived aliases.

Fix only the `file` value on these two lines to the archived PRD. Leave the historical PRD, implementation record, evidence files, reasons, and existing seed rows unchanged. No workflow/runtime-tool modification or alias fallback is needed. This narrowly corrects a reference broken by the already-authorized archive move.

## Delivery sequence after fresh planning approval

1. Recheck remote state and unrecognized dirty files. Preserve unrelated work rather than staging it.
2. Create a dedicated `chore/main-bookkeeping-ci-evidence` branch from the current archive-bearing history and merge the latest `origin/main` normally. Preserve `b5d65d9` and all prior product hashes; no rebase, squash, amend, force push or branch deletion.
3. Delegate the bounded document/context edits to `trellis-implement`: the evidence matrix and the two broken archived-PRD references only. The main session owns task artifacts, Git operations and final integration.
4. Use the independently inspected provider artifacts in `provider-ci-verification.md` to record the exact named run/target coverage. Promote only `IB-CI-PROVIDER-EVIDENCE-001` if the evidence meets its existing contract; leave unrelated rows and release/maturity decisions alone.
5. Validate this task and the historical archive contexts; review line/link targets and compare the full diff with an explicit allowlist. Independently check evidence fidelity and unchanged historical blobs with `trellis-check`.
6. Batch explicitly staged work commits, then the task's finish-work bookkeeping; keep runtime pointers, downloaded artifacts, personal journals and machine-local data out of Git. Use the existing workflow's commit-confirmation and task-completion gates. When archiving this task, use the supported `--no-commit` mode so its own research/context references can be updated to the archive location and validated before the explicit bookkeeping commit; do not recursively create another task just to archive this one.
7. Open a small PR to `main`; merge only after the checks that actually apply to its head pass, using a normal merge commit. Report any remaining branch-only bookkeeping explicitly rather than claiming it was landed.

## CI applicability and acceptance checks

- `.github/workflows/ci.yml:3-7` and `.github/workflows/security-audit.yml:3-7` trigger for pull requests without path filters.
- `core-beta-evidence.yml:5-24` and `release-verify.yml:22-44` filter PR/push paths to product/build/release inputs. This documentation/task-only change does not trigger them. An absent path-filtered run is not a passed run and must not be reported as one.
- The historical provider evidence run remains pinned to `fa7a0a92bb22270a522b0b4b3b95a16d4365ae1a`; it is not described as a new run of the future docs-only head. If that PR unexpectedly touches product/workflow inputs, stop and re-plan instead of treating the old run as fresh evidence.
- Local planned checks: `python .trellis/scripts/task.py validate <current-task-dir>`; `python .trellis/scripts/task.py validate .trellis/tasks/archive/2026-10/10-07-merge-main-and-land`; `git diff --check`; inspect the complete `git diff --name-status origin/main...HEAD` plus unstaged/index diffs before staging.
- Product/workflow/schema/lockfile paths must have no changes. Archived content must remain identical to `b5d65d9` except the two documented manifest path values.
- Review must retain the matrix's `ci_verified` versus `locally_verified` distinction, hosted-runner/expiry limitations, and the absence of minimum-OS, signing, clean-machine installation, real-E5 and provider-maturity certification.

## Artifact lifecycle

This note records planning and subsequent delivery evidence; remote completion must be checked against the named GitHub records. GitHub PR/run records are the durable remote delivery record. No copied CI binary, raw user transcript, local cache or personal path is to be committed by this task. Any post-merge task closure bookkeeping must be reported and handled as part of this same authorized scope, not silently left out of the claimed result.
## Execution coordination (2026-10-07)

- The user approved the final planning summary and implementation, including commit/push/PR merge after checks. The task is now `in_progress`.
- Main remains at the inspected `fa7a0a9` baseline. A dedicated `chore/main-bookkeeping-ci-evidence` branch was created, and normal merge commit `e238e09` integrates `origin/main` while retaining `b5d65d9`; no product-tree changes or conflicts occurred.
- Implementation is delegated with an exact three-file write scope. Main owns task artifacts and Git operations; independent review will cover the complete final diff, not only those three edits.
- Code-spec judgment: no command/API/schema, infra behavior, runtime convention or product evidence contract changes. The existing matrix status vocabulary already governs this promotion. The archive-reference pitfall and literal validator behavior are captured here; editing managed Trellis tooling or unrelated package specs would expand the approved scope and is not required.
- Delivery lifecycle: land the reviewed archive/evidence work first. Then archive this task only after its implementation is actually landed, repair its own moved manifest paths using `archive --no-commit`, and land that closeout under the same authorized scope (a second pure-bookkeeping PR if required). Do not create a new task for that closeout, bypass checks, claim an unrun workflow passed, or leave branch-only archive changes hidden in the final result.
## Verified implementation delivery (2026-10-07)

- Work commits: `02d89e7` (matrix and historical context references) and `6a132ac9bcc85f2796f7c37f479459675115e0f0` (task plan/research/review).
- [PR #23](https://github.com/LainHappy/AgentSessions/pull/23) merged at `2026-10-07T03:21:37Z` as `9f08ac32dc584e01eb1bc6f9b05a914929096ece`, using a normal merge and an exact-head guard; no admin bypass or history rewrite.
- Before merge, all eight checks passed at the PR head: [ci run 37565600334](https://github.com/LainHappy/AgentSessions/actions/runs/37565600334) and [security-audit run 37565600370](https://github.com/LainHappy/AgentSessions/actions/runs/37565600370) both completed successfully. No evidence/release workflow was claimed to run for this documentation-only head.
- After fetch/fast-forward, `git diff --quiet 6a132ac9bcc85f2796f7c37f479459675115e0f0 origin/main` passed: the merged main tree is identical to the checked head. The historical task exists only at its archive path in main.
- The remaining delivery is this task's own archive, its four moved research-context references, and this receipt. It is handled in the same authorized task, with a pure-bookkeeping PR and its own checks, not as a new task or product change.
