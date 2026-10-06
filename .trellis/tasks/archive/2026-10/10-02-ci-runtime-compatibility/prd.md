# CI runtime compatibility: Node24 actions and runner readiness

Status: **all six implementation acceptance criteria verified on 2026-10-02; task.json owns the live lifecycle state**. Final hosted receipts are in implement.md and PR #19 comment 5951921995.

## Goal
Remove reliance on hosted runtime coercion for the actively verifiable CI Actions while retaining trustworthy checks, evidence artifacts and all product/release contracts. This is preventive maintenance of currently green CI, not a product bug fix or release-readiness promotion.

## Baseline and authority
- Baseline: `ffaf69b93d696d07708516a326436caae48326da` (`origin/main`, verified 2026-10-02). Reliability PRs #17/#18 are complete; main ci 36956529885 succeeded.
- The task was initially created with --no-start for planning. On 2026-10-02 the owner replied `keep going` directly after reviewing the final four-Action / three-workflow / 11-reference plan and the explicit approval question. This approves that enumerated CI dependency exception and the normal private PR/CI/merge flow, not wider dependency or release-gate changes.
- `09-27-relocation-ci-compatibility` is a different historical relocation/Clippy/SQLite task. Its records and runtime pointer are not this task's authority and remain untouched.
- Current official Action/runtime evidence and use-site/source anchors are in the two research reports. At the planning baseline, all five direct JavaScript Action pins declared node20; the accepted first batch is limited to four reviewed Actions in three verifiable workflows.

## Requirements
- **R1 — Approved bounded CI dependency exception.** Update only checkout, setup-python, cache and upload-artifact to the exact reviewed Node24-native commit pins in design.md. The three allowed workflows contain 11 use sites. Retain immutable 40-character SHAs and truthful version comments; do not move to unreviewed latest majors.
- **R2 — Preserve repository workflow contracts.** All workflow text outside those references/comments remains unchanged: events/path filters, jobs/steps/conditions, permissions, credential inputs, runner matrices, targets, cache keys/paths, Python versions and artifact settings. Upstream bug/security fixes in the reviewed versions are acknowledged, not falsely described as a zero-behavior-change dependency update. No unsafe checkout or insecure runtime opt-out is allowed.
- **R3 — Preserve and verify evidence delivery.** The four existing core-beta jobs must produce their expected unsigned evidence/binary artifacts with the same names, content layout and 7-day retention. Keep hidden-file defaults and fail-on-missing behavior; metadata inspection is not a substitute for checking the actual uploaded artifacts.
- **R4 — Fresh delivery evidence.** Complete the local workflow-contract/manifest/diff checks, all 12 expected current-head PR jobs, and main ci/evidence (7 + 4 jobs) on the actual merge SHA. Record separate run identities and tested/merge tree equivalence. PRIVATE billing restrictions cannot be bypassed or relabeled as success.
- **R5 — Honest release and runner deferral.** Leave release.yml byte-identical. The remote has no existing tag for its guarded manual rehearsal; do not manufacture one or claim the full release DAG was exercised. The reusable quality job indirectly consumes ci.yml changes, which does not satisfy the separate release gate. Record Ubuntu 26.04 migration exposure without changing labels, adding a matrix job, or claiming future-platform evidence.
- **R6 — Preserve all existing exclusions.** No product code, Cargo dependency/lockfile, schema, MSRV, Rust toolchain, release-gate, SLO, provider-tier or performance-route change. The original approximately 37.5% paired 1M gain remains below the unchanged >=50% target; Hermes SQLite and Cursor cursorDiskKV remain experimental. Do not clean up other sessions or shared pointers.

## Approved implementation scope
- `.github/workflows/ci.yml`: 7 Action reference/comment replacements.
- `.github/workflows/core-beta-evidence.yml`: 3 replacements.
- `.github/workflows/security-audit.yml`: 1 replacement.
- This task's planning, review and portable CI evidence artifacts. No source/spec/helper changes are pre-authorized; discovery of a necessary behavioral/configuration change returns to planning.

## Acceptance criteria
- [x] AC1 (R1): Owner approves the final narrowed scope in a subsequent message before task.py start, workflow edits or CI dispatch. Official ref resolution and fixed-source `runs.using: node24` agree for all four approved commits; runner requirements are checked against actual run evidence before acceptance.
- [x] AC2 (R1/R2): Exactly 11 intended use sites are updated. All other workflow text and every out-of-scope tracked path match the approved baseline; release.yml is byte-identical.
- [x] AC3 (R2/R3): Existing workflow contract tests pass, both curated task manifests validate, and complete diff/privacy review finds no prohibited toggle, permission widening or local secret/path.
- [x] AC4 (R3/R4): Current-head PR ci/security/evidence succeed (7 + 1 + 4 jobs), and the actual four core-beta artifacts are downloaded/inspected with portable receipts.
- [x] AC5 (R4): Normal exact-head-guarded merge succeeds; merge tree and tested tree agree; fresh main ci/evidence pass (7 + 4 jobs). Repository is read back PRIVATE, and no billing/visibility/gate workaround was used.
- [x] AC6 (R5/R6): Release-only pins/full rehearsal and future Ubuntu-image execution remain explicit follow-ups, not completed claims; original product/performance/provider exclusions and other windows' state are preserved.

## Out of scope
release.yml or release-only Action migration (including download-artifact); creating/moving/deleting tags or publishing releases; choosing a new runner policy or adding Ubuntu 26.04 probes; product/dependency/schema/MSRV/SLO changes beyond the four enumerated CI Action pins; performance routes A-D; beta/provider promotion; cache deletion or permissive fallback behavior; repository visibility/billing changes; other-window cleanup; personal journals/runtime state.

## Final review gate
The owner approved the exact four-Action CI dependency exception and its normal private PR/CI/merge workflow on 2026-10-02 after the final summary. Any material scope/candidate/configuration change returns to planning. AC1 is now supported by the recorded hosted runner evidence as well as approval; approval alone was never treated as acceptance.
