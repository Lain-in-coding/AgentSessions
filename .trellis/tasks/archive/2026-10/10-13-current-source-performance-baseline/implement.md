# Current-source baseline execution plan

Status: implementation approved on 2026-10-14. Checkboxes record actual progress;
unchecked commands and work have not been completed.
Read `design.md` and the three research files before implementation. The current
source is the separate canonical product checkout named in `prd.md`; all
product commands below run there, not in the older coordination repository.

## Preconditions and ordered work

- [x] Obtain the user's response approving the final planning summary. Verify
  PRD/design/this plan and both context manifests are complete, then run
  `task.py start` for this child from the coordination root. Do not start its
  parent. The preceding gate-validator repair must remain in the chosen base.
- [x] Recheck product main/ref and clean state; preserve the two protected
  coordination spec hashes. If the product base changed materially, reconcile
  the plan before proceeding. Use a feature branch in the canonical repository.
- [x] Dispatch `trellis-implement` with an `Active task:` first line, native
  context injection or child-side context loading, and ownership of the three
  tooling files in `design.md`. It is not alone; no unrelated edits/reverts,
  measurements, commits or publication inside its implementation assignment.
- [ ] Extend the existing process lifecycle with an opt-in detailed observation
  while preserving all imported-helper defaults and legacy return semantics.
  Capture wall time before observer cleanup; retain the legacy duration too.
- [ ] Add the versioned opt-in baseline matrix, exact corpus phases/census,
  independent correctness oracle, budgets, explicit query/context parameters,
  raw records and failure/timeout evidence. Keep normal smoke/full unchanged.
- [ ] Add conditional strict extension validation, three-report
  `validate-baseline`, and JSON-derived Markdown projections. Preserve the
  legacy full/smoke validators and all nine existing metrics.
- [ ] Add the build/source/binary/environment receipt, exact bundled-SQLite
  provenance checks and relative-path privacy handling. Reject unsupported
  external-link overrides for this qualified local baseline; do not mutate
  global build settings or install tools.
- [ ] Add meaningful regression tests and update SLI §2.1 for the optional
  contract, two timing protocols and source/publication identity. Keep Draft.
- [ ] Dispatch `trellis-check` for the same bounded write set. Resolve review
  findings and run the validation below. Main session coordinates fixes,
  handles any spec update and forms the authorized tooling commit S.
- [ ] Freeze clean S and H=S. Finish tests, then let opt-in run 1 build and
  freeze B/receipt before starting the shared suite deadline. Runs 2/3 verify
  and reuse that B without building. Do an untimed correctness preflight. Check
  scratch capacity, deadlines and the absence of competing benchmark work.
  Check the largest 250-file argv against the native platform launch limit
  using the actual owned temporary-root layout. Prefer short owned root names;
  do not split a timed sync batch, reduce the matrix or change cwd to hide a
  launch failure. An unsupported layout fails preflight before sampling.
- [ ] Run the three complete reports serially, with fresh roots and the fixed
  scale-order rotation. Retain all raw/warmup/failure records. Enforce the
  proposed 60-minute suite and 300-second child deadlines and documented
  scratch/RSS boundary guards. Any cap breach is incomplete, not a smaller
  successful workload or automatic retry.
- [ ] Validate each report and the three-report set. Recheck clean S, H/B/lock
  identities and corpus/output hashes. Generate Markdown from validated JSON.
- [ ] Run one separate largest-scale trace trial. Verify trace output and
  stage coverage, then produce privacy-reviewed aggregates. Missing trace
  remains an explicit diagnosis limitation; no trace-enabled latency is pooled.
- [ ] Prepare only the publication files in `design.md`: three original JSONs,
  their projections, manifest and a dated performance-document entry. Record
  measurements for S, all attempted runs, noise/coverage limits and one bounded
  optimization follow-up. Preserve all existing evidence bytes.
- [ ] Independently review publication provenance, raw-to-summary math,
  correctness, trace interpretation, privacy and the S-to-P diff. Main handles
  current-head quality/governance, authorized commit/push/PR work and any later
  owner acceptance. Never autonomously merge or close the parent release gate.

## Proposed narrow command interface

These additions are a design target, **not commands supported by today's
checkout**:

- `run --profile full --baseline-run 1|2|3 --suite-id <safe-id>` opts into one
  complete anchor-plus-three-scale run. Ordinal fixes the scale order; the ID
  binds three independent runs. Reject these options with smoke, missing
  paired inputs, an existing output report, external `--binary` or unsupported
  ordinals. Run 1 alone builds before the shared deadline; runs 2/3 load and
  verify its receipt/B from the common output parent and never invoke Cargo.
  Missing/incompatible receipt is failure, not a new build or restarted suite.
- `validate-baseline <report-01.json> <report-02.json> <report-03.json>` validates
  the exact complete suite and rejects duplicates or legacy-only input.

Use the existing run options for workspace/output and measured environment.
The implementer may choose internal helper names; observable protocol,
compatibility, write set and acceptance criteria are fixed by this plan.

Reproduction command shape after the tooling change (values must come from the
actual S/build/environment receipt; no private paths or invented metadata):

```powershell
$baselineSource = '<full-S-SHA-from-manifest-or-frozen-receipt>'
$baselineActual = (git rev-parse HEAD).Trim()
if ($baselineActual -ne $baselineSource) { throw 'Use the clean measured source S.' }
$baselineSuite = '<safe-suite-id>'
foreach ($baselineRun in 1..3) {
    $baselineOutput = 'evidence-output/current-baseline/run-{0:D2}' -f $baselineRun
    python -B scripts/evidence/core_beta_benchmark.py run `
      --profile full --baseline-run $baselineRun --suite-id $baselineSuite `
      --workspace . --output-dir $baselineOutput `
      --expected-commit $baselineSource `
      --disk $baselineDisk --filesystem $baselineFilesystem `
      --antivirus-state $baselineAntivirus `
      --sqlite-version $baselineSqliteVersion `
      --sqlite-version-source $baselineSqliteEvidence
    if ($LASTEXITCODE -ne 0) { throw 'Baseline attempt failed; retain its evidence.' }
}
python -B scripts/evidence/core_beta_benchmark.py validate-baseline `
  evidence-output/current-baseline/run-01/core-beta-benchmark-full.json `
  evidence-output/current-baseline/run-02/core-beta-benchmark-full.json `
  evidence-output/current-baseline/run-03/core-beta-benchmark-full.json
```

The suite deadline starts after run 1 has completed its build/receipt preflight
and is shared across the three processes via validated suite/start metadata in
their common ignored output root. Runs 2/3 verify and reuse the pinned B; they
do not rebuild or restart the deadline. Trace uses the remaining measurement
budget or is marked unavailable. The publication command embeds the actual
manifest S, never the later publication checkout's current HEAD.
Do not execute template commands before all values, the actual interface and
source/build receipt are verified. Build receipt validation is not replaced by
manually passing a plausible SQLite version string.

## Required regression and quality checks

Tests use small synthetic or mocked fixtures, not 50,000-message performance
runs in unittest. Required cases:

- Legacy full fixture and no-extension smoke accepted; profile/metric/count
  negatives still rejected; gate/semantic helper compatibility preserved.
- Fake clock/observer demonstrates corrected timing ends before cleanup and
  the legacy result still includes cleanup; byte counts and nullable memory
  observations stay aligned with the same invocation.
- Child environment isolates inherited trace/repo/clock overrides without
  mutating the parent or imported-helper defaults. Nearest-rank MAD, exact
  15-percent boundary and both zero-denominator spread cases are deterministic.
- Valid extension round-trip; unknown version, absent/duplicate workload/run,
  wrong phase/size/ordinal/order/count, bool/NaN/infinity, bad lineage and
  tampered derived statistics fail with bounded diagnostics.
- Initial/noop/shrink accounting, deletion/retention, wrong show/get identity,
  incorrect context graph/evidence and unexpected partial are caught by an
  independent synthetic oracle. Failed outcomes cannot enter successful stats.
- Child timeout, shared-suite deadline, boundary resource stop, output
  no-clobber and ownership-checked cleanup retain failure evidence and exit
  nonzero. Dirty/wrong harness, binary mismatch, unresolved build overrides
  and unproved SQLite linkage cannot qualify a published suite.
- Run 1 builds once before the deadline; runs 2/3 never build, require the
  original receipt and preserve prebuilt provenance labels. Reproduction rejects
  a later P/HEAD that differs from the expected manifest S.
- Markdown projections match validated JSON; public metadata contains only
  reviewed synthetic/relative identities, not temporary paths or ambient data.

Run from the canonical product root:

```text
python -B -m unittest scripts/evidence/test_core_beta_benchmark.py -v
python -B -m unittest discover -s scripts/evidence -p "test_*.py" -v
python -B -m unittest discover -s scripts -p "test_*.py" -v
python -B scripts/evidence/core_beta_benchmark.py validate-report docs/evidence/core-beta/88d86f4/core-beta-benchmark-full.json
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run new-report `validate-report` for all three files and `validate-baseline`
after measurement and again on the publication copies. Recompute report hashes
and compare every Markdown projection in memory with the same S renderer.
Existing full fixture validation is a compatibility test, not a fresh benchmark.

If only documentation/evidence changes after S, repeat the affected evidence,
projection and governance checks; rerun broader tests only for new source,
failures or unresolved concerns. Record actual test counts and limitations;
old 25-check success is not current-head verification.

## Publication/governance and rollback checkpoints

Use canonical `CONTRIBUTING.md` and the exact interfaces recorded in
`research/provenance-publication.md`: commit/PR checks, path/privacy scanning
and credential scanning over the actual tree, introduced commits, PR metadata
and proposed merge message. Check mandatory hosted results for the actual SHA.
Use the already approved pinned scanner archive; unavailable required inputs
are reported, not auto-installed or replaced by skipped tests.

Before S, inspect the exact three-file diff. Before measurement, confirm clean
S and reproducible build linkage. Before P, require only the declared report
and documentation write set and unchanged S-bound inputs. After publication,
retain source labels and use corrective commits for changes; do not rewrite
history or overwrite historical evidence. Raw experiments, corpus stores,
traces, scanner logs and local task metadata remain outside product commits.

At finish, main reviews whether any new cross-task convention merits a separate
approved spec update, preserves protected dirty specs, and completes only this
child's acceptance. Parent benchmark/install and final release decisions stay
open. Planning is not implementation or merge approval.

## Execution record (2026-10-14)

- The user approved the final summary with "批准实现！". Context manifests
  validated with six entries each; the task moved to `in_progress` and product
  branch `perf/current-source-baseline` was created from clean canonical main
  `8e05eb495160d5ff5979cdbef6f63856d5a7de81`. Remote main matched this SHA.
- The implementation agent owns only the three tooling files. Main owns task
  state, independent validation, source freezing and subsequent measurements.
  Both protected coordination spec hashes matched their recorded values.
  Ownership was then split, with explicit handoff before edits: one implementer
  owns only the core harness; a second owns its test module and the SLI document.
- Environment preflight: Rust/Cargo 1.97.1, Python 3.10.11, native
  `x86_64-pc-windows-msvc`, NTFS/NVMe, about 31.5 GiB RAM and 154 GiB free
  scratch space. Defender antivirus and real-time protection are enabled.
  Relevant ASG/Cargo/Rust/SQLite override variables were absent. These are
  preliminary observations; actual report receipts must still capture the run.
- `cargo fmt --all --check` and workspace/all-targets Clippy passed. The full
  Rust workspace passed 2,006 tests with 23 pre-existing ignored and zero
  failures. Rust/Cargo inputs are unchanged by this task. A current debug
  binary is available solely for Python correctness integration tests.
- The native Gitleaks 8.30.1 archive matched its pinned SHA-256. Eighteen
  governance files were exported byte-for-byte from the canonical base and
  verified identical to the owner-reviewed frozen checker source. Governance
  tests passed 93 with no skips using the existing archive, without downloads.
- Current repository permissions/protection were read back: ordinary merges
  only, administrator enforcement, strict `gate` and
  `all required verification checks` from Actions App 15368. The authenticated
  owner matches the public attribution used by the preceding reviewed commit.
  These base observations do not substitute for checks/acceptance at a new head.
- Native Git schannel and default GH API requests encountered transport errors.
  Per-command OpenSSL Git and process-local `GODEBUG=http2client=0` GH requests
  succeeded; certificate checks were not disabled and no global configuration
  was changed. This records a working transport, not a proved underlying cause.
- Private validation logs and trusted-tool receipt are under
  `.trellis/.runtime/current-source-performance-baseline-20261014/`.
  No full or scale performance measurement has been run yet.
- Earlier project-wide user messages explicitly authorized commit/push delivery
  and delegated project execution. The latest final-plan approval authorizes
  this bounded implementation. Do not ask again for already-granted commit/push
  permission; exact-current-head ordinary-merge acceptance remains separate.
- Read-only test-discovery research confirmed that the evidence suite needs
  `AGENT_SESSION_GREP_BINARY` pointing to the current debug executable to enable
  four otherwise-skipped synthetic E2E cases. Those tests use explicit sources
  and a temporary catalog, not real transcript discovery. The old benchmark
  root unit test mocks its runner and never invokes unsafe discovery.
  Root discovery does not recurse into evidence/release/governance directories.
  Windows root tests will receive the existing Git Bash through
  `AGENT_SESSION_GREP_TEST_BASH` and `AGENT_SESSION_GREP_REQUIRE_NATIVE_BASH=1`,
  with `PYTHONDONTWRITEBYTECODE=1`, all scoped to the test process.
- Resumed the approved implementation after a model switch. The on-disk diff
  remains limited to the three approved tooling files. Prior agent sessions
  could not resume because their provider rejected cross-resource response
  state; no product changes were lost. A fresh independent `trellis-check`
  agent is reviewing the full diff and a small actual-CLI correctness preflight.
- Initial post-implementation verification passed: 48 core harness tests,
  137 evidence tests with the current debug CLI, and 29 root tests with the
  required native Git Bash; no tests were skipped. Historical `88d86f4` full
  evidence remains accepted by the new checker. Review fixes require the
  affected suites to be rerun before freezing S.
- Remote main was rechecked at `8e05eb495160d5ff5979cdbef6f63856d5a7de81`.
  Current repository identity, ordinary-merge-only policy, administrator
  enforcement and both strict Actions required checks still match the plan.
  The pinned Gitleaks archive, all 18 exported trusted governance files and
  both protected coordination-spec hashes were reverified unchanged.
- Independent full-scope review found and fixed one P1 issue before formal
  measurement: read-only SQLite WAL census can create sidecars, so store
  file/byte observations must be frozen before the census. The real-WAL
  regression demonstrated failure before the fix and success afterwards.
  The fix, regression and contract clarification remain in the same three
  approved tooling files. No other concrete finding remains open.
- Actual-CLI preflights passed for 1 x 20 and 2 x 200 synthetic layouts,
  including initial/noop/shrink, all five query classes, both context policies
  and all three trace phases. These are untimed verification, not baseline
  measurements. Full fixture JSON is 5,641,914 bytes, below the unchanged
  16 MiB governance per-blob limit; actual publication sizes remain to verify.
- Post-fix evidence verification passed independently in both main and review:
  137 tests with no skips (90.162 s / 93.664 s). Main confirmed harness/test
  bytes did not change during its run. Syntax compilation, diff whitespace
  checks and historical full compatibility also passed. No Linux/macOS local
  execution is claimed. Both verification processes finished before sampling.
- Phase 3.3 review: the new command/receipt contract and observer-order pitfall
  are captured in the approved SLI document and executable regression tests.
  No additional cross-task Trellis convention is needed for this bounded
  evidence task; preserve both protected dirty specs and the exact write set.
- Trusted scanner export closure was repaired locally before source acceptance:
  `scripts/release/export_public_tree.py` and `scripts/evidence/privacy_scan.py`
  were missing from the private 18-file governance export. Both supporting
  files were exported from base, verified byte-identical to the frozen checker,
  and added to its local receipt. No product/scanner code or policy changed.
  The initial unavailable-input scan failures remain in private logs; complete
  candidate and actual-source commit/path/credential scans then passed.
- Tooling S is frozen as `07f75b0fe5aadb8c7e85e7a6d2e636f5c6b57970`
  (`perf(evidence): add source-bound baseline`), with tree
  `f2ee9ec73a48c20ff903cc8b4d9fb06fdc10321d`. It contains exactly the three
  approved tooling files. The canonical worktree is clean, and actual-S
  governance scans covered all 581 tree files and all introduced content
  with zero findings and no unsupported coverage. PR/merge metadata is later.
- Source/build preflight passed at S: native target/toolchain, no unsupported
  Cargo/compiler/link overrides, exact H=S, and no competing Cargo/Rust/CLI
  process. The formal suite ID/output parent is
  `current-source-20261014-07f75b0fe5aa`; reports will remain bound to S.
- The first invocation at that source stopped in build provenance before any
  receipt, shared deadline or observation existed. Its private attempt records
  `sections=[]` and `bundled native SQLite library unavailable`; the original
  output/claim/log remain intact. It is not a completed performance run.
- Root-cause evidence: this native MSVC build supplies both `sqlite3.lib` and
  `libsqlite3.a`, each 4,945,172 bytes with SHA-256
  `4aa2a3bdfc6df250ec888a19aa5498c6b32b36f52ff3395196c61b250901d9a6`.
  Locked `cc 1.3.0` source, verified against its Cargo.lock archive digest,
  explicitly creates these aliases. The harness incorrectly required exactly
  one filename. A bounded corrective tooling review selects the native target
  artifact and verifies every present alias without relaxing static linkage.
- The correction remains within the same three approved product files. A new
  tooling commit will supersede the first frozen source for measurement;
  preserve both commits and explicitly disclose the zero-sample preflight
  failure. The matrix/resource limits are unchanged, and no suite deadline was
  started or reset. The successful suite must use one newly frozen source and
  a new output identity, never overwrite the failed attempt.
- The archive correction passed independent review: target-native selection
  requires MSVC `.lib` or other native-target `.a`; any present alternate name
  must have identical bytes/hash. Existing static-library/output-directory
  checks and receipt schema are unchanged. Three new tests exercise the actual
  receipt builder, including missing/conflicting aliases and dynamic-link
  rejection. Focused tests passed 9; the complete evidence suite passed 140
  with zero skips (82.756 s), plus syntax/diff/old-full checks. The existing
  real release archive passed the new selection helper without rebuilding.
- Corrected measured S is now `6dc0e55792db365fc687c1a2db6cb864ec941210`
  (`fix(evidence): verify MSVC archive aliases`), tree
  `dfd70dec62bb88599a8a9251582347022c19440c`. The earlier tooling commit is
  preserved as its parent. Candidate and actual-S commit/path/credential
  scans passed across both introduced commits. Remote main still matches the
  original base, and the worktree is clean. The new suite/output identity is
  `current-source-20261014-6dc0e55792db`; only this S can label its results.
- That suite's first run completed 3,169 successful observations and every
  planned phase/census, then failed final anchor-size linkage validation. The
  immediate shrink snapshot was 19,656,704 bytes; the legacy post-read/doctor
  measurement point retained an additional 32,768-byte SHM and empty WAL.
  These are different valid observation times, not a failed product sync.
  The old failure-phase label named the last operation and is also corrected.
  No successful report was published and no raw observations were overwritten.
- Independent read-only review recomputed all 48 statistic groups and 39 phase
  ratios, confirmed serial ordinals/UTC, source/receipt/binary identities and
  run-2/3 reuse logic, and found no additional P1/P2 issue. The editing checker
  preserves early snapshots and adds an explicit final post-doctor store bound
  to the legacy fields, with a complete producer-to-validator integration test.
- Cumulative measurement accounting: the failed measured attempt ran from
  `2026-10-14T12:04:07.800521Z` through at most
  `2026-10-14T12:13:57.158487Z` (589.358 s). Reserve 605 s conservatively,
  leaving 2,995 s for the next complete suite and trace within the original
  3,600 s total measurement cap. Build, review and test time are excluded.

### Report assembly retrospective (trellis-break-loop)

1. Root cause: test coverage gap plus an implicit observation-time assumption.
   Hand-built valid fixtures used identical phase/final sizes, so they did not
   expose stateful WAL side effects across the complete producer sequence.
2. Why the earlier fix was incomplete: freezing store metadata before census
   correctly repaired each phase snapshot, but the legacy final measurement
   still occurred later, after all reads and doctor. Comparing both points as
   one observation was invalid.
3. Prevention: preserve both observations; bind the final store to the final
   doctor ordinal; validate exact files/totals; exercise the actual anchor
   producer followed by its link validator; label report-validation failures
   separately from successful measured operations.
4. Bounded expansion: an independent reviewer checked other real
   producer/validator and receipt/suite relationships rather than relying on
   internally consistent fixtures alone. No unrelated product layer changed.
5. Knowledge capture: the executable contract and regressions remain in the
   approved SLI/harness/test files. The user's protected-spec/write-set rules
   take precedence over generic guide/template update suggestions; both
   protected coordination specs remain unchanged.
- Final-store repair verification completed: 143 evidence tests passed with
  no skips (105.464 s), syntax/diff/old-full checks passed, and one actual
  frozen-CLI full-anchor integration passed the Core v1, anchor-section and
  producer-to-link validation (353 observations / 9 snapshots). Its immediate
  shrink footprint was 19,775,488 bytes and final post-doctor footprint was
  19,808,256 bytes; both are preserved and the latter binds legacy final size.
  This was a correctness test, not a replacement performance run. No remaining
  concrete finding was reported by either independent checker.
- The final repaired measured S is `b0fc44d756909cc840050351dd1329896a8edf75`
  (`fix(evidence): bind final anchor storage`), tree
  `55a71757bb8f8faf213fd500b01c02afd5988d2c`. All three tooling commits are
  preserved; candidate and actual-source commit/path/credential gates passed.
  The new suite is `current-source-20261014-b0fc44d75690`. Main reserves the
  prior 605 seconds and monitors a remaining 2,995-second measurement budget;
  neither earlier attempt is overwritten or presented as qualified evidence.
- The qualified suite completed and passed all validations. Run 1 built and
  froze B/receipt before the shared deadline (started
  `2026-10-14T12:50:05.949642Z`), then completed `13:00:00.493795Z`; run 2
  completed `13:30:50.276567Z`; run 3 report completed `13:41:23.011842Z`
  followed by a separate 50k trace trial (status available), ending
  `13:42:44.919763Z`. Runs 2/3 reused the frozen B without Cargo, and the
  suite wall of about 52.7 minutes stayed within the 60-minute shared
  deadline and per-child 300-second bounds. The earlier 589.4-second failed
  attempt remains disclosed separately, so total local measurement wall
  across both attempts is about 62.5 minutes.
- `validate-report` accepted each of the three reports; `validate-baseline`
  accepted the three-report set, and every Markdown projection was confirmed
  byte-identical to the S renderer output. Cross-run median-spread warnings
  above 15 percent were recorded for cold startup and several sync/noop and
  short context series; these are disclosed noise, not gates.
- Publication P is being assembled under
  `docs/evidence/core-beta/b0fc44d75690/` with manifest and three run
  directories, plus a dated entry in `docs/performance-baseline-0.2.md`
  recommending the 50k changed-sync commit path as the first optimization
  follow-up with testable acceptance criteria. Reports keep measured source
  `b0fc44d75690`; P itself is a separate later commit and never relabels S.
- Publication P is committed as
  `4f34287c6060fe0b40a5deb0feb99f358c28a931`
  (`docs(evidence): publish current-source baseline`, tree
  `f79eb09a70a425480f02c5ac3073140f1dcf9232`, 8 files). The staged blobs
  were hash-checked against the manifest before commit; the worktree was
  clean afterwards. Full-range gates on `8e05eb4..4f34287` passed:
  commit syntax (4 commits), path/privacy (588 tree files, 17 introduced,
  zero findings, no unsupported coverage) and credentials (Gitleaks 8.30.1,
  zero findings), before and after freezing the PR event snapshot.
- PR #19 opened as `docs(evidence): publish current-source baseline` with
  base `main` at `8e05eb495160d5ff5979cdbef6f63856d5a7de81` and head P.
  PR metadata + introduced-range check passed with the frozen event
  snapshot; metadata_sha256
  `d513be879de0df590a82f9a942066e98d677b1c8259a045da2a51f3a2298379e`.
  Hosted checks are still running at the time of this entry; merge remains
  blocked until the owner accepts the current head.
- Hosted checks finished: all 25 checks on head
  `4f34287c6060fe0b40a5deb0feb99f358c28a931` passed (including `gate`,
  the required quality-gate aggregate, cross-platform builds, tests,
  security audit and release `verify` jobs), and GitHub reports
  `mergeStateStatus: CLEAN` with no merge conflicts. The pushed branch is
  `perf/current-source-baseline`; PR #19 is the review surface.
- Remaining lifecycle step: explicit human-owner acceptance for this exact
  head, then a separately authorized ordinary merge. No AI approval or
  autonomous merge was performed.
- The owner granted explicit acceptance and full Git authorization on
  2026-10-14. Merge-time governance re-passed with the proposed merge
  message and identities included: commit syntax, PR metadata + full range
  (`metadata_sha256
  42eb6bb214ab5dc02842c63954363313502e2a4916b2de262526dcc5a76b0988`),
  path/privacy and credential scans (both `merge_message_scanned: true`,
  `merge_identities_scanned: true`, zero findings).
- PR #19 was merged with an ordinary merge commit
  `de2c95d2c93f7453ce5b5ccc169614d3aa7892ee` (parents `8e05eb4` and
  `4f34287c6060`; author `Lain在coding`, committer `GitHub`, message exactly
  the validated text). All four commits are preserved on `main`.
  Remote branch head and merged main were re-read back; local `main` was
  fast-forwarded to the merge commit. Parent release gates remain open.
