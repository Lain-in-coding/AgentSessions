# Execution proposal

Status: the latest scoped summary was approved on 2026-10-02. Workflow edits begin only after task.py start succeeds; the preserved exclusions remain in force.

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
   - [ ] Follow Phase 3.3/3.4: retain existing specs unless a real contract change requires replanning; commit only the allowlisted migration and task evidence under the confirmed scope.
   - [ ] Publish the authorized private branch/PR. Wait for current-head 7 ci + 1 security-audit + 4 core-beta-evidence jobs; do not substitute baseline runs or suppress annotations/checks.
   - [ ] Verify the hosted runner versions meet 2.327.1 without changing runner policy. Download/inspect the actual four evidence artifacts into ignored output: names, unsigned binaries, expected evidence paths, source/target identity and retention. Record only portable run IDs/hashes/aggregate verdicts, never raw private logs or machine paths.
   - [ ] A provider tier or product release is not approved by green evidence CI. Do not dispatch release.yml or create a tag.
5. **Integration and finish (coordinator)**
   - [ ] Fresh-read PR head/base, all checks and PRIVATE; merge normally with an exact head guard only after success.
   - [ ] Verify tested/merge tree identity and actual main ci (7 jobs) plus core-beta evidence (4 jobs) on the merge SHA.
   - [ ] Record live receipts and residual release/Ubuntu follow-up boundaries; archive only this task through the approved finish-work flow. Leave journals, root WIP, baseline/research worktrees and other pointers untouched.

## Stop/replan conditions
- A reviewed Action requires changed inputs/permissions/artifact shape, an unapproved major-version semantic change, or a runner below its required version.
- A release rehearsal would require a new v* tag or publication condition change.
- A need emerges to change labels, gates, SLOs, product dependencies, schema, MSRV or provider policy.
- Main advances in a way that invalidates the scoped baseline. Do not reset back to an older commit.
- Current-head CI fails or is externally billing-blocked: diagnose truthfully within scope; no retry-to-green, visibility switch or billing workaround.

## Coordinator local acceptance and spec-sync judgment (2026-10-02)

The independent Trellis checker found no blocking issue and made no workflow repair. Its three workflow blob IDs match the accepted 11-line patch. The reviewer report is local working-tree evidence, not a future commit or hosted-run receipt. AC2/AC3 local gates are satisfied; AC1 runner evidence and AC4/AC5 remote gates remain pending.

Phase 3.3 was reviewed using trellis-update-spec: no new command/API/schema, environment/secret wiring, artifact shape, platform guarantee or release/SLO contract was introduced. Existing CLI evidence-honesty and external gate specs still apply. Keep .trellis/spec/ unchanged rather than adding volatile vendor version tables to product specs; those tables and reviewed compatibility risks belong in task research. This matches the independent check recommendation and the approved no-spec-edit scope.

The owner confirmed the one-shot Phase 3.4 two-batch commit plan on 2026-10-02. The three workflow files are committed as `8611c41656b86d23a45435f9dab11189533b2b44`; the nine task/planning/review files form the separate documentation batch containing this record. No unrelated work is staged. Remote PR/main/runner/artifact gates remain pending and require their own receipts.

Staged-document gate: the initial `git diff --cached --check` caught one redundant EOF blank line in `research/action-runtime-upgrades.md`, which the earlier untracked-file per-line check did not cover. The terminal blank line was removed and the staged check passed. No workflow/product change or lint suppression was used.
