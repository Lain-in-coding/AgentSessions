# Design proposal: bounded CI runtime maintenance

Status: the bounded first batch was approved by the owner on 2026-10-02 after final artifact review. Exact version/SHA provenance is recorded in `research/action-runtime-upgrades.md`; current use sites and contracts are recorded in `research/workflow-contracts.md`.

## 1. Recommended first batch
Update only the four Node24-native JavaScript Actions used by the actively verifiable CI workflows: checkout, setup-python, cache and upload-artifact. The proposed allowlist is `.github/workflows/ci.yml`, `.github/workflows/core-beta-evidence.yml` and `.github/workflows/security-audit.yml`, containing 11 target references total. Keep full immutable commit SHAs and matching human-readable version comments. Select versions from the reviewed official-source table, not a moving major tag or an unverified latest release.

No repository workflow configuration edit is bundled beyond the reviewed reference/comment changes. The upstream Action versions do carry documented bug/security fixes; this is not a claim that dependency internals change only their Node declaration. After normalizing only the four approved reference values and their version comments, each workflow must equal its approved baseline. If a candidate needs an input, permissions, cache layout or trigger change to work, return to planning rather than silently adding a workaround.


### Approved immutable targets (2026-10-02)

| Action | Version | Full commit SHA | Sites |
|---|---|---|---:|
| actions/checkout | v5.1.0 | `fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09` | 5 |
| actions/setup-python | v6.3.0 | `ece7cb06caefa5fff74198d8649806c4678c61a1` | 3 |
| actions/cache | v5.1.0 | `caa296126883cff596d87d8935842f9db880ef25` | 2 |
| actions/upload-artifact | v6.0.0 | `b7c566a772e6b6bfb58ed0dc250532a479d7789f` | 1 |

All four fixed action.yml files declare node24; their documented minimum runner is 2.327.1. Capture the actual hosted runner version during acceptance rather than claiming it was measured during planning. These are first-native-major candidates with the verified fixes described in research, not an automatic update to every repository's latest major or a permanent maintenance/security guarantee. In particular, upload v5 advertised Node24 support but still declared node20; the candidate v6 fixes that distinction.

### Compatibility applicability

The repository's three target workflows have no pull_request_target or workflow_run event; the known in-repo ci.yml caller is release.yml, whose events are push/tag and workflow_dispatch. Checkout v5.1.0's safer fork-PR default therefore does not require an unsafe opt-out for these known flows. This is not a blanket promise for arbitrary external reusable-workflow callers. The explicit persist-credentials:false in security-audit stays false.

Every scoped setup-python use passes an explicit python-version (3.10 or 3.11) and does not enable its optional dependency cache. New pip inputs are not enabled and Python versions are not raised. The distro-sensitive Python-cache change is therefore not activated by this first batch. The separate Cargo cache keeps its existing keys/paths; a read-only cache token may produce the upstream truthful save warning without being a test failure or a reason to widen permissions.

Core-beta's artifact producer does not feed release.yml's upload/download chain. Its v6 metadata preserves the current classic ZIP, inputs, outputs and hidden-file defaults; actual archive inspection still gates acceptance. The untouched release-only v4/v4 producer/consumer pair and any future cross-version migration remain outside this batch.

## 2. Flow and consumers
- Generic CI expands to three OS tests, three installation smokes and cargo-deny. Security audit is separate and retains its explicit credential-persistence refusal.
- Core-beta evidence expands to four fixed runner/target pairs. Its uploader transports unsigned reports and binaries with the existing artifact names, paths, 7-day retention and fail-on-missing setting. Verify actual artifacts after the run.
- The release quality job reuses ci.yml and therefore indirectly consumes its approved pins. The separate release preparation/build/assemble/publish steps remain unchanged; current CI evidence is not an executed full release rehearsal.
- The pinned Rust toolchain composite Action and cargo-deny Docker Action remain outside this JavaScript runtime update. Rust/product dependencies, Cargo.lock, MSRV and tool versions remain unchanged.

## 3. Preserved contracts
Keep event names and path filters; all job IDs/names/counts, needs/if expressions and step commands; runner labels/matrices/target triples; Python versions; shell selection and timeout behavior; credentials and permissions; cache paths/keys; artifact names/paths/retention; MACOSX_DEPLOYMENT_TARGET and target-specific RUSTFLAGS. Do not suppress migration warnings with insecure runtime opt-outs or add new fallback paths.

The fixed Ubuntu 22.04 evidence/release GNU builders, Windows 2022 and macOS target mappings remain intact. No minimum-OS, glibc floor, signing, notarization or provider-tier claim changes. Shared product specs and external gate documents remain unchanged because their contracts do not change.

## 4. Why release.yml is deferred
The authenticated remote tag inventory is empty. Existing workflow_dispatch requires a real v* tag, while pushing a new v* tag can activate real publication. This task must not create/move/delete tags, relax the tag/commit/version guards, or bypass publication conditions to manufacture a rehearsal.

Leave release.yml byte-identical in the first batch. Its remaining Node20-native pins, including all download-artifact use sites, remain explicit follow-up work. The full release rehearsal is not an acceptance criterion for this narrowly scoped batch and is not falsely marked complete; the independent external release gate stays outstanding under its existing owner/process.

## 5. Ubuntu image readiness
The announced ubuntu-latest transition starts 2026-10-19 and is planned to finish 2026-11-19. This batch records the impact boundary only. Retain the existing floating generic CI labels and fixed evidence/release builders. Do not add a new runner/matrix job, pin Ubuntu 24.04, or claim Ubuntu 26.04 verification without a separately scoped probe and actual evidence.

## 6. Verification and rollout
Before edits: refresh origin/main and candidate provenance; materially changed baseline or compatibility facts require an updated plan. Use the existing isolated worktree, and select an owned implementation branch without resetting or stashing other work.

After the approved 11 pin/comment changes: run the standard-library workflow contract suite and an exact scope/semantic diff check. A fresh PR should naturally trigger 12 jobs (7 ci, 1 security audit, 4 evidence). Inspect all four evidence artifacts under ignored local output. Merge only after current-head gates succeed, with an exact head guard and no admin bypass; verify tested/merge tree equality and main ci/evidence at the merge SHA (7 + 4 jobs).

Remain PRIVATE throughout. If hosted CI is billing-blocked, retain a pending external gate, not a visibility change, billing modification, skip or local-success substitute.

## 7. Rollback
Rollback is a new scoped revert of the approved Action references after identifying the failure; do not rewrite history, reset another worktree, lower a gate, broaden permissions or add runtime opt-out flags. Historical green receipts are not proof for a rollback head; validate its real runs separately.
