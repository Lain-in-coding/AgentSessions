# Execution proposal

Status: the approved implementation and hosted acceptance completed on 2026-10-02. Final receipts below supersede the explicitly historical planning/local snapshots; the preserved exclusions remain in force.

## Ordered work packages and gates
1. **Evidence and planning (coordinator + research)**
   - [x] Fetch the completed baseline, verify PRIVATE and isolate this task from other windows.
   - [x] Create the task in planning with --no-start; preserve all session pointers.
   - [x] Inventory use sites, workflow topology, existing contract checks and the empty remote tag inventory.
   - [x] Complete official Action runtime/version/SHA research and freeze the proposed four-Action table in design.md (no latest-major auto-selection).
   - [x] Complete PRD convergence, design, this execution plan and both curated manifests; task validation passed (6 + 6 entries), the existing workflow suite passed 3/3, and the planning-only scope/privacy check passed.
   - [x] Present the final narrowed scope and obtain subsequent approval: the owner replied `keep going` on 2026-10-02 to the explicit four-Action CI dependency exception and normal private PR/CI/merge question.
2. **Bounded migration (Trellis implement agent, after activation)**
   - [x] Re-fetch main on 2026-10-02 (still ffaf69b), verify PRIVATE, and re-read all four official tags and fixed action.yml files: every approved SHA matches and declares node24. Select the owned chore/ci-runtime-compatibility branch without resetting or stashing work.
   - [x] Activate only this task on 2026-10-02; task.py current --source resolves the new session-scoped in_progress task, not the stale relocation fallback. No other pointer was cleared.
   - [x] Dispatch trellis-implement with native context injection preferred and child-side manifest loading as fallback. Its write set is only the three approved workflows and their 11 four-Action reference/comment lines; coordinator artifacts remain disjoint.
   - [x] Independent full-scope review confirmed no release.yml, inputs, jobs, permissions, runner labels, Rust/product dependencies, scripts, provider policies, spec or unrelated tracked-file edit.
3. **Full-scope check (Trellis check agent)**
   - [x] Independent Trellis check read the full task/artifacts and affected diff; report: research/check-report.md.
   - [x] Verified four exact SHAs, 11 use sites, fixed-source node24 declarations, non-target pins, checkout event applicability and unchanged setup-python inputs.
   - [x] Independent proof accepted all 1,250 tracked normalized blobs: exactly 11 approved replacements; the other 1,247 files and index/modes are unchanged. release.yml worktree bytes and CRLF are preserved.
   - [x] Existing workflow suite passed 3/3, manifests passed 6 + 6 entries, YAML/duplicate-key/structure and diff/UTF-8/privacy checks passed; no Cargo or dependency installation was required.
   - [x] Product source is unchanged and no Cargo rerun is claimed. Any future diagnostic Cargo invocation still requires --offline and, for dependency resolution, --locked.
4. **Commit and private CI (coordinator)**
   - [x] Follow Phase 3.3/3.4: retain existing specs unless a real contract change requires replanning; commit only the allowlisted migration and task evidence under the confirmed scope.
   - [x] Publish the authorized private branch/PR. Wait for current-head 7 ci + 1 security-audit + 4 core-beta-evidence jobs; do not substitute baseline runs or suppress annotations/checks.
   - [x] Verify the hosted runner versions meet 2.327.1 without changing runner policy. Download/inspect the actual four evidence artifacts into ignored output: names, unsigned binaries, expected evidence paths, source/target identity and retention. Record only portable run IDs/hashes/aggregate verdicts, never raw private logs or machine paths.
   - [x] A provider tier or product release is not approved by green evidence CI. Do not dispatch release.yml or create a tag.
5. **Integration and finish (coordinator)**
   - [x] Fresh-read PR head/base, all checks and PRIVATE; merge normally with an exact head guard only after success.
   - [x] Verify tested/merge tree identity and actual main ci (7 jobs) plus core-beta evidence (4 jobs) on the merge SHA.
   - [x] Record live receipts and residual release/Ubuntu follow-up boundaries; archive only this task through the approved finish-work flow. Leave journals, root WIP, baseline/research worktrees and other pointers untouched.

## Stop/replan conditions
- A reviewed Action requires changed inputs/permissions/artifact shape, an unapproved major-version semantic change, or a runner below its required version.
- A release rehearsal would require a new v* tag or publication condition change.
- A need emerges to change labels, gates, SLOs, product dependencies, schema, MSRV or provider policy.
- Main advances in a way that invalidates the scoped baseline. Do not reset back to an older commit.
- Current-head CI fails or is externally billing-blocked: diagnose truthfully within scope; no retry-to-green, visibility switch or billing workaround.

## Historical local-phase acceptance and spec-sync judgment (2026-10-02, before PR CI)

The independent Trellis checker found no blocking issue and made no workflow repair. Its three workflow blob IDs match the accepted 11-line patch. The reviewer report is local working-tree evidence, not a future commit or hosted-run receipt. AC2/AC3 local gates are satisfied; AC1 runner evidence and AC4/AC5 remote gates remain pending.

Phase 3.3 was reviewed using trellis-update-spec: no new command/API/schema, environment/secret wiring, artifact shape, platform guarantee or release/SLO contract was introduced. Existing CLI evidence-honesty and external gate specs still apply. Keep .trellis/spec/ unchanged rather than adding volatile vendor version tables to product specs; those tables and reviewed compatibility risks belong in task research. This matches the independent check recommendation and the approved no-spec-edit scope.

The owner confirmed the one-shot Phase 3.4 two-batch commit plan on 2026-10-02. The three workflow files are committed as `8611c41656b86d23a45435f9dab11189533b2b44`; the nine task/planning/review files form the separate documentation batch containing this record. No unrelated work is staged. Remote PR/main/runner/artifact gates remain pending and require their own receipts.

Staged-document gate: the initial `git diff --cached --check` caught one redundant EOF blank line in `research/action-runtime-upgrades.md`, which the earlier untracked-file per-line check did not cover. The terminal blank line was removed and the staged check passed. No workflow/product change or lint suppression was used.

## Final hosted implementation acceptance (2026-10-02)

All six PRD acceptance criteria are satisfied. The two work commits are
`8611c41656b86d23a45435f9dab11189533b2b44` (approved workflow pins) and
`318352af7dcf0faedeb4bacc7e26545dbd42d0d0` (task plan and independent local review).
The earlier reports' pending-remote statements are historical execution snapshots,
not outstanding gates after this dated receipt.

### Exact-head merge and run receipts

PR #19 was merged normally with its exact head guard, without admin bypass, at
`2026-10-02T11:41:43Z`; merge commit:
`ad7e0424ea0d8bd9c120929be591a0d6c15a86e9`.
The PR evidence source is GitHub test-merge
`84e1dc2980ba645120f46d392c37dcf8a449d8a4`, with parents
`ffaf69b93d696d07708516a326436caae48326da` and the accepted PR head `318352af`.
Test-merge, accepted head and actual merge all have tree
`8051e83cb59bc4f1164fead0b67d6d457c59dfa1`.

| Event / workflow | Run | Head | Result |
|---|---|---|---|
| PR / ci | 36963112662 | 318352af | SUCCESS, 7/7 jobs, attempt 1 |
| PR / security-audit | 36963112637 | 318352af | SUCCESS, 1/1 job, attempt 1 |
| PR / core-beta-evidence | 36963112703 | 318352af | SUCCESS, 4/4 jobs, attempt 1 |
| main / ci | 37002416149 | ad7e0424 | SUCCESS, 7/7 jobs, attempt 1 |
| main / core-beta-evidence | 37002416098 | ad7e0424 | SUCCESS, 4/4 jobs, attempt 1 |

Main ci completed at `2026-10-02T11:57:15Z`; main evidence completed at
`2026-10-02T11:48:51Z`. These are the implementation merge's runs, not promises
about the later task-only archive PR. `security-audit` is not a main-push workflow.

### Actual artifacts and runner receipts

All four PR artifacts were downloaded/inspected before merge. The coordinator
also downloaded/checked the four main artifacts: exactly 14 nonempty files per
artifact, expected evidence/binary layout, source/target identity, unsigned
metadata, binary size/SHA256 against environment, gate and benchmark records,
passing adapter/provider test summaries, all 18 cases across four spike reports,
and 7-day artifact retention (expiry on 2026-10-09).

| Platform | PR artifact ID | Main artifact ID | Target | Binary bytes | Main binary SHA256 |
|---|---:|---:|---|---:|---|
| linux-gnu-x64 | 11208079517 | 11224636038 | x86_64-unknown-linux-gnu | 8045784 | `9737b1ddfe552490421bc72d3fa44ef86c30ba771e743378ca2467d62b9baa6c` |
| macos-intel | 11208673986 | 11223349816 | x86_64-apple-darwin | 7589976 | `8362fce37cc87fc3c3f6a3edcdb5a7416faa2e756b29179f232e689a77655ff7` |
| macos-arm64 | 11209121267 | 11224765252 | aarch64-apple-darwin | 6868432 | `5b2dead1b16ce176dc2720e750cff5075f83b21add7844b4ce3d102239ee28d5` |
| windows-x64 | 11208699072 | 11224094981 | x86_64-pc-windows-msvc | 7352320 | `d811f0566e447110a1f485e57a60534860cfe6a861dcc78fd89d8e753e84d2a3` |

Main environment/gate/benchmark source records all identify merge `ad7e0424`;
PR environment records identify test-merge `84e1dc29`, not the PR head directly.
The Windows PR binary digest is
`d136419494a63de5643898281f7b7764cbc1d4ef6ecc30eba0bbd4afc8c180a8`;
it differs from the main binary but each matches its own run's records. No
bit-for-bit cross-build reproducibility or release certification is claimed.
Raw binaries/logs and local verification receipts remain ignored, not committed.

Main evidence job IDs `110822904950` (Linux), `110822905058` (ARM64 macOS),
`110822905110` (Windows), and `110822905155` (Intel macOS) all report runner
`2.337.0`, meeting the approved `>=2.327.1` requirement. Four earlier PR runner
samples also passed. No runner label or permissions were changed.

The accepted independent implementation check is `research/check-report.md`.
A fresh post-merge Trellis check sub-agent failed before executing because of a
service-side resource error. The additional main-artifact checks are explicitly
coordinator verification, not a claimed second independent product review.
No Rust source changed and no local Cargo rerun is claimed for this closeout.

### Privacy, archive and deferred boundaries

Repository visibility was independently read back PRIVATE at
`2026-10-02T12:02:53Z`. PR #19 comment **5951921995** records the implementation
acceptance and the separate task-only archival handoff. The archive is delivered
on `docs/ci-runtime-compatibility-closeout` through a normal private PR; its
publication/CI result is recorded in PR comments rather than guessed here.
The archival pass updates only this task and its relocated context references.
Local identity/journals/runtime state, other-window tasks, and preserved
research/baseline worktrees are not part of the commit. No optional local
journal is recorded because personal journals are excluded by this task.

Spec-sync was reviewed again: the closeout only records acceptance and task
lifecycle; no new product/executable contract is introduced, so shared specs
stay unchanged. Release-only Action pins/full rehearsal and future Ubuntu-image
execution remain separately scoped follow-ups. No tag, release dispatch,
publication, visibility/billing change, runtime opt-out or CI bypass was used.
Successful private CI does not imply future billing/spending capacity.

The paired 1M initial-sync/peak-RSS improvement remains approximately 37.5%, below
the unchanged >=50% target and subject to the recorded conditional-delivery stop
rule. Hermes SQLite and Cursor cursorDiskKV remain experimental. Performance
routes A-D, provider promotion/product-policy choices, product dependencies,
schema, MSRV, release gates and SLOs were not changed.
