# Windows build feedback evidence

## Bounded trial revision (protocol v2)

The current task adds a versioned capacity/Pilot/Formal path. Existing v1
protocols retain the original 80 GiB floor, fixed 100 formal pairs and live
artifact requirements described below; neither those files nor their meaning
are changed. The previous 127-test checkpoint is historical evidence.

The approved v2 budget retains the immutable first complete allocation B and
its B+40 GiB review / B+60 GiB hard limits. It requires 40 GiB free after both
the owned-output reservation and an additional 8 GiB external-growth allowance.
For the initial 8 GiB capacity probe this means at least 56 GiB starting free.
The external allowance reserves disk, not hypothetical project-cache bytes;
actual related-cache growth is still measured against the original B. All
retained logs, reports, candidate binaries, stores, seeds, failed outputs and
recycle-pending bytes count. Moving something to the Recycle Bin releases no
disk space.

`make_protocol` accepts `protocol_version=2`, strict `budget_policy`,
`reservation_provenance`, `retention_policy`, `capacity_probe` and, only for
Formal, a pinned `formal_registration`. The initial capacity scope is one
native leaf crate (`agent-session-grep-domain` or `agent-session-grep-ports`),
`check` or `test`, default features, all targets, locked/offline, Cargo jobs 2
and test threads 2. A fresh dedicated target is required. The whole Windows
process tree retains the 24 GiB committed-memory limit, quiet-source/toolchain
admission and conservative live-write/free-space guard. `capacity_limited`
records an intentional owned-cap stop separately from quality, timeout, memory
or source failures. Capacity has zero paired samples and cannot authorize
profile/tool adoption.


A separately declared expanded full-capacity run supplies the first complete
arm evidence before Pilot. It uses one selected native configuration, all18
primary/warm/edit captures and the full qualification closure, with zero paired
samples. Its declared cap limits a controlled observation; it does not
promise that the whole workload fits. After a verified leaf seal, the first
cap is at least8GiB and the prior observed peak plus the reservation margin;
8GiBcap+40GiBfloor+8GiBexternal requires56GiBfree and may stop early. The old
39GiB v1 retained-pair forecast remains unchanged. The successful leaf seal
is a prerequisite, not a full-workload peak.
Both compared configurations need full observation coverage before measured
Pilot reservation can use the maximum verified peak plus margin.

For v2, generated qualification allocations pin the official release suite
under that arm source's ignored evidence-output/build-feedback directory.
Keep the unchanged official reports and suite binary at their originating
paths; bill that suite and declared scratch with all arm outputs and evidence.
Qualification commands run with real JobObject and cumulative capacity bounds.
Use a dedicated process-local release environment compatible with the official
validator, without altering global PATH, Cargo profiles or frozen source bytes.
V2 source verification includes Git-ignored files: only the declared official
suite may create ignored source output. Validator loading uses approved source
bytes without reading/writing .pyc; release children set
PYTHONDONTWRITEBYTECODE=1. Clippy/optional-feature qualifications require real
Cargo artifact records; diagnostics bind their approved argv and source/artifact
lineage. Receipt text alone cannot establish qualification.
The frozen official producer historically removes its own successful TEMP
scratch with its existing cleanup function. No real release producer has been
run here. Its behavior must be reconciled with the recycle-only execution policy
before that stage can run; do not modify or monkey-patch the frozen producer,
claim its native cleanup went through recycling, or waive scratch accounting.

Each successful paired arm must finish its full primary, warm and edit
six-job workload and all artifact-dependent qualification while its bytes
exist. An independently approved local checker then seals the original
command captures, raw logs/reports, source/toolchain/config and allocation
identities, complete artifact hashes/sizes/NTFS identities and the qualification
closure. Keep selected final binaries and source snapshots. The approved
lifecycle acts only on exact predeclared new `targets` paths whose precreation
lease records absence; it protects daily caches, existing last-72-hour caches,
stores, seeds, quality/source failures, every Git common directory, the system
pagefile and unrelated Recycle Bin contents.

The paired Python entry `run_protocol(..., arm_finalizer=...)` requires a
callable coordinator lifecycle before allocating v2 outputs. Ordinary CLI
paired v2 `run` refuses with this requirement. Initial leaf capacity runs
without that callback; expanded full capacity requires the coordinator
finalizer to capture qualifications, seal and dispose approved outputs.
Each v2 output is independently approved and records its absent
precreation lease under `run_root/leases`; raw arm/probe receipts pin that
lease. Approved stores and prewarm sources stay protected. The literal native
recycle fixture is separately limited to fixed content and at most 1 MiB and
cannot be presented as a successful capacity or performance measurement.

The narrowly scoped `build_feedback_retention.py` helper requires the pinned
v2 retention approval, known checker and verification-tool hashes, explicit
protected/source roots and a local integrity key. That key protects local
receipts against accidental or unapproved rehashing; it does not establish
independent attestation against someone with access to the same key. Seals
state the limitation: deleted artifact bytes cannot be independently reread.
Normal missing-artifact evidence remains invalid; only a matching approved
seal and complete exact-item disposition can enter the retained branch.

V2 normalized comparisons require the pinned original runner campaign
(`runner_campaign_evidence={path,sha256}`). Revalidate that campaign's complete
attempts, raw failures/statuses, observations and registered compute/storage
projection; successful arm seals cannot erase a later campaign budget or
quality failure.

The single-arm capacity projection uses `normalize_runner_arm(raw_arm,protocol)`;
it requires the immutable original capacity/arm.json, declared source/config
and explicit null ordinals with zero paired samples. Real qualifiers use
`capture_qualification(protocol,arm_record,qualification,...)` and retain every
previous guarded capture; full-capacity seals verify their cumulative budgets.
The full-capacity seal stays under the budgeted capacity arm directory.
Interrupted diagnostics or release without complete reports remain protected;
a cap-stop status alone cannot authorize their disposal.


The disposal stages are scope/evidence verification, immutable move intent,
Windows Shell recycle operation, exact item identity/content verification,
immutable purge intent, single matching Shell item purge, observed disk-release
verification, and final receipt. Unknown side effects are reconciled before
retry.
Real release accounting must sample free space after its own flushed
intent receipts, so evidence allocation is not mistaken for payload retention.
If a crash loses an exact immediate baseline, release remains inconclusive and
the coordinator never repeats a known completed purge.  Reparse points, hardlinks, unsupported named streams, changing directory
contents and inconclusive identities refuse. Recycle capacity must be known
and sufficient to avoid evicting unrelated items. Aborted Shell operations,
recycle bypass and inconclusive release stop the destructive branch; there is
no generic hard-delete fallback, `cargo clean` or whole-bin emptying.

The initial leaf-capacity command uses the ordinary CLI `run --protocol FILE`.
Expanded full-capacity and paired v2 runs need an explicit coordinator lifecycle;
callers must not silently retain
both arms under a reservation calculated for only the active arm. Before
starting a real run, regenerate admission from current free space, a complete
related-root inventory and quiet process/source/MSVC/tool identities. Other
windows may continue development; their running builds prevent measurements
and disposal, not Python tooling implementation or review.

Complete five paired native Pilot batches before selecting Formal N. Use real
variance, time, active-arm/store/retained-evidence peaks and failures to propose
N; no formal count is promised by a capacity probe. Freeze the approved N,
seed/order, source/config, stopping rule and compute/storage limits before
Formal collection. The old v1 remains fixed at 100. All current speed, storage,
P95, correctness, diagnostics, optional-feature and official full release
workload gates remain mandatory, and uncertain bounds mean no adoption.

## Current v2 validation and admission checkpoint

The final independent ten-file review passed Ruff, Mypy, whitespace and **277
unittest cases**, with zero failures, errors or skips. All five suites ran in
one TestSuite in **665.248 seconds**: inventory 35, runner 106, retention 53,
benchmark 21 and statistics 62. Their byte hashes were unchanged during testing
and were reverified by the coordinator before the local tool freeze. The older
221/226 and 127 results remain historical checkpoints. These checks verify the
infrastructure; they are not real capacity, build-speed or release samples.

The final review also closed three executable boundaries. Clippy and
optional-feature qualifications require actual Cargo records and complete
captured artifact lists. Diagnostics bind approved commands, frozen source and
original Cargo artifacts. V2 frozen-source checks reject ignored output outside
the declared official release suite. Official validator imports compile verified
source bytes without reading or writing bytecode; the release child disables
bytecode writes. Existing cached bytecode is preserved.

The complete related inventory on 2026-10-20 measured 192324873512 allocated
bytes (179.116 GiB), with zero errors, while fixed B remains 156436568120 bytes.
Its free-space observation was 56053633024 bytes (52.204 GiB). This receipt
retains its original timestamp. A later read-only resource receipt observed
**38.211 GiB free and quiet=false**, with another window's ASG process active.
That resource observation is not a regenerated inventory or complete admission.
The 8 GiB initial probe still needs at least **56 GiB** plus all other checks.

A new initial-leaf v2 protocol, independently scoped retention approval and
budgeted control/key/evidence store passed protocol, rebuild and retention-policy
validation. The previously selected source contains recent ignored validator
bytecode, so v2 refuses it and preserves it. The protocol uses another existing
clean frozen worktree with the same approved source/HEAD/tree/lock identity.
The old v1 protocol stays unchanged. The trial run root and target remain absent;
no Cargo capacity probe has started. Pilot remains **0/5** and Formal 0; formal N
awaits the completed Pilot and approval. Neither a profile nor a tool is adopted.

Native Windows Shell read-only preflight found 11 existing Recycle Bin items
and admitted a conservative 1 MiB fixture plus the no-eviction safety margin.
This proves only preflight; actual recycle/purge and disk release remain untested.
The literal fixture target is absent and quiet disposition is refused. Both Git
common directories, all 110 registered frozen source worktrees, existing caches
and recent outputs remain protected. The current task stays in_progress and is
not archived; this checkpoint ends after scoped local commit and journal record.

## Historical v1 workflow and evidence

The following describes the original v1 contract. Current v2 behavior is
explicitly revision-bound as above; historical forecasts or synthetic tests
are not actual Pilot/Formal qualification.

The build feedback harness measures the default-feature agent-session-grep
workspace on Windows NTFS/MSVC. It is development tooling, separate from the
product CLI, provider adapters and the official `core_beta_benchmark.py`
evidence validator. No command adopts a Cargo profile, installs a tool,
changes PATH or deletes a cache. Python 3.10+ and the standard library suffice.

All paths in protocol, inventory, campaign and qualification receipts are
machine local. Keep them in the ignored `.trellis/.runtime/` or
`evidence-output/` trees. Commit code, reproducible contracts and reviewed
summaries; do not commit raw paths, logs, transcripts or machine receipts.

## Read-only inventory and admission

Run from the repository root. `--help` is the executable command reference;
the following commands use local relative receipt paths only:

```text
python scripts/evidence/build_feedback_benchmark.py --help
python scripts/evidence/build_feedback_benchmark.py inventory --help
python scripts/evidence/build_feedback_benchmark.py inventory --root . --store-root evidence-output/cache-store --output evidence-output/inventory-001.json
python scripts/evidence/build_feedback_benchmark.py preview --protocol evidence-output/protocol.json --inventory evidence-output/inventory-001.json
```

Specify the coordinator, Beta, registered/known worktrees, historical runtime
build roots and every tool store. Discovery is not permission to scan source,
Git objects or Cargo downloads. Actual allocated bytes are globally deduplicated
by volume/file identity, including aliases and hard links; logical size is
reported separately. An unknown path, incomplete discovery, unreadable file or
unsupported allocation measurement blocks admission. Do not substitute `du`,
file lengths or a directory-name guess for a formal allocation scan.

Freeze the first complete inventory as baseline **B** and bind its SHA-256 in
every protocol. Do not rebase B while other windows develop between segments.
The project review point is B+40 GiB, the hard stop is B+60 GiB, and C drive
free space must remain at least 80 GiB. Projected growth consumes both project
and disk headroom. All remaining stores, seeds, failed attempts and retained
trial targets count. Initial store paths must remain explicit store declarations
in later inventories; a discovery root with the same path does not cover opaque
cache blobs. After a pilot measures a store peak P, reserve
`ceil(1.25 * P / GiB + 1) GiB`. A target peak is not a measured store peak.
External growth between segments reduces the remaining allowance.

`preview` reads existing receipts and prints the protocol identity, budget,
reasons and exact disposable target paths. It creates no source worktree,
target, store, receipt or configuration. A review-required preview is not
approval. Above the soft budget, `run` requires the owner's local receipt
covering that exact protocol and batch; hard limits cannot be overridden.

## Freeze the protocol before starting a segment

Only the coordinator creates source snapshots/worktrees. The snapshot must
include the complete Beta source, dirty/untracked product files, lockfile and
resources while preserving the live Git index and source. It has a dedicated
snapshot commit/tree and an immutable source manifest. The runner consumes
preallocated dedicated baseline/candidate worktrees; it never creates or
reuses the live source as an experimental arm. Dedicated source worktrees
inside the coordinator are permitted only under `.trellis/.runtime/` or
`evidence-output/`, and Git must report that exact directory as its worktree
root. Ordinary source directories and roots overlapping the live source are
rejected. Prewarm worktrees must also be independent of every arm source;
nested overlap is rejected. Output directories must remain outside source
roots, even when both live under the ignored runtime.

The Python API `build_feedback_runner.make_protocol(configuration)` validates
and returns a version 1 protocol. Save it once to a new local path. Its input
contains these explicit fields (consult the runner docstring for exact types):

| Input | Required evidence |
| --- | --- |
| `source_root`, `source_commit`, `source_tree`, `source_receipt_path` | Dedicated frozen source, full Git IDs and source manifest |
| `baseline_receipt_path` | Complete fixed-B inventory and immutable hash |
| `run_root` | New dedicated local run directory |
| `toolchain` | Absolute cargo/rustc/MSVC compiler/linker paths, SHA-256 and full versions; Windows MSVC target |
| `msvc_environment_receipt_path` | Required for real segment capture: verified compiler/linker identity, vcvars script/hash and allowlisted LIB/INCLUDE/SDK environment |
| `baseline_config`, `candidate_config` | Known debug, incremental, layout and tool variants |
| `allocations` | `{pair_index, attempt, baseline_source_root, candidate_source_root}`; fresh frozen worktrees for every attempted arm |
| `inventory_roots`, `store_roots`, `predicted_growth_bytes` | Full known allocation scope and conservative next-batch forecast |
| `phase`, `seed` | `pilot`=5 pairs or `formal`=100 pairs; frozen balanced order |
| `pilot_receipt_path`, `pilot_receipt_sha256` | Formal phase requires a complete same-source pilot and measured margin |

Cargo and rustc are fixed at 1.97.1 and recorded with full version output and
binary hashes. The six fixed jobs, in order, are workspace check, domain/ports
tests, CLI tests, SQLite tests, application tests and workspace Clippy. Every
job uses `--locked --offline -j2`; tests additionally use `--test-threads=2`.
Clippy denies warnings. Fetch frozen dependencies outside the measured time.
Protocol construction can describe a configuration before MSVC environment
verification, but a real segment refuses to capture the toolchain or build
without the hash-bound environment receipt. The verified environment is applied
only to children; global PATH and compiler configuration stay untouched.

The original native baseline uses full debug, incremental on and six isolated
targets. Candidate debug is `full` or `line-tables-only`; incremental is `on`
or `one-shot-off`; layout is `isolated` or manual `3lane`. Isolated launches
six concurrent Cargo commands with six explicit targets. Three-lane launches
three concurrent queues: check then Clippy, domain/ports then CLI, and SQLite
then application. Each Cargo command is `--locked --offline -j2`; all four test
jobs use `--all-targets` and `--test-threads=2`, with default features unchanged.
Results stay in canonical JOBS order even when completion order differs. Disposable
one-shot incremental-off never disables incremental in the daily edit probe.
The explicit full-debug configuration remains available. Release profiles
remain unchanged until all adoption requirements have been met.

Tools are opt-in native, verified local sccache, or MBX. They require explicit
binary/version/hash/store identities and a prewarm receipt from a different
frozen same-project worktree. Cold-store seed cost is disclosed separately.
No wrapper stacking or global PATH/wrapper change is allowed. MBX uses the
verified local v1.24.1 executable, explicit targets, `MBX_CACHE_DIR`,
`MBX_GC_AUTO=0`, `MBX_TARGET_VIEWS=0`, deliberate per-phase `MBX_INCREMENTAL`,
plain output and pinned CARGO. A private child APPDATA isolates MBX config;
there is no setup/adopt/native check lane. Every descendant must be captured
in the arm's Job Object; unsupported ambient configuration fails closed.

The sccache candidate is specifically a **prewarmed READ_ONLY local store**:
`local_rw_mode=READ_ONLY` prevents new inserts during measurement. Internal
LRU capacity is set to 2^63-1, not the pilot reservation, so the external budget
guard is not competing with internal eviction. Separate RW prewarming is
outside timing and requires its own source/ownership/budget evidence; seed
cost is disclosed. The dedicated foreground server, loopback listener PID,
binary and all descendants must be verified in the same Job Object. The
store remains explicitly reserved and externally guarded. Store admission
requires its actual allocation to be strictly below its own reservation; the
whole-pair forecast also reserves fresh targets and is not added to the store
again. MBX write growth is bounded separately by its live reservation guard.
Prewarm receipts must contain all six successful, owned offline jobs. Neither a review
receipt nor promising timings waive missing attribution, non-eviction or
budget guarantees. Implementation support is not an adopted tool or a claim
that a complete quiet-time smoke/pilot/formal campaign has passed.

```text
python scripts/evidence/build_feedback_benchmark.py run --protocol evidence-output/protocol.json --pair-limit 5
python scripts/evidence/build_feedback_benchmark.py run --protocol evidence-output/protocol.json --review-receipt evidence-output/batch-review.json --pair-limit 20
```

A segment is at most 20 valid pairs. This is a health boundary, not an early
statistical decision: formal total N remains 100. Each arm uses a fresh target
identity and runs all six primary jobs, all six feedback-warm jobs with
incremental on, then applies the predetermined comment edit in its own
worktree and runs all six edit jobs with incremental on. With `incremental=on`,
each job/lane keeps the **same primary target** across primary, feedback-warm
and edit; warming does not allocate a second target set. Only the explicit
`incremental=one-shot-off` variant uses a separate incremental-on feedback
target set, shared by feedback-warm and edit, so disposable primary settings
do not disable daily-edit incremental reuse. All retained target sets remain
in allocation and budget accounting. Per-phase elapsed
time is the actual concurrent phase wallclock, never the sum of job durations;
artifact parsing and hashing happen after phase drain, outside timed phases. It restores
only the verified edit it introduced. Process launch is suspended until
association with a Job Object; the whole tree, including a cache daemon, has a
24 GiB committed-memory limit and kill-on-close. Peak committed memory must
not be described as RSS.

Full related-root allocation scans run before/after quiet segments, never
inside a timed polling loop. During capture, conservative C free-space and
owned-job write-accounting bounds protect the fixed baseline B. The guard
anchors free space at the admission receipt and retains the minimum observed
free space, so external writes before launch or between polls consume headroom
even if a later sample recovers. It enforces both the 80 GiB floor and any
stricter floor in the owner review receipt. Final allocation and free-space
checks reject an over-budget result rather than recording it as successful.
Owned store admission remains independent. Uncertain/incomplete samples stop
safely without inventing a global receipt. Contaminated pairs retain their full
evidence and require a complete
retry using fresh preallocated worktrees in the original order. Failed,
timed-out, memory-limited, budget-limited and interrupted attempts are retained
as failures; a later success cannot remove them. Missing process, source or
artifact identity prevents a formal start. Cargo JSON names artifacts from
this invocation, and receipts pin those files; never pick a stale executable
by scanning target directories or relaxing the official Beta validator.

Runner state is stored in `run_root/campaign.json`; raw arm and process
receipts remain immutable. Repeating `run` resumes matching verified state.
An interrupted inflight attempt is preserved and does not silently restart.
The runner prints actual completion and reasons. Compute-hour forecasts from
pilot durations exclude dependency fetch, seed creation, qualification jobs,
quiet-time waiting and retries; they are not completion-date promises.

## Compare complete evidence

```text
python scripts/evidence/build_feedback_benchmark.py compare --campaign evidence-output/run/campaign.json --protocol evidence-output/protocol.json --qualifications evidence-output/qualifications.json --output evidence-output/comparison-001.json
```

For runner campaigns, `--protocol` is mandatory and arm/state receipt hashes
are rechecked before normalization. The separate `owned-allocation.json`
must still match its recorded hash, inline payload and owned byte count;
a storage-complete boolean alone is insufficient. Primary, feedback-warm
and edit phases must each contain all six successful captured jobs. Current
log hashes from every phase and the final Cargo invocation identity per
artifact path are rechecked. Logs pin earlier
versions that were intentionally replaced by a lane or edit rebuild. The CLI keeps every attempt; it does not
let operators choose only the fastest or successful samples. If a historical
allocation receipt does not cover both owned targets and stores, the
comparison reports missing storage evidence and remains unqualified. Current
scans cannot retroactively establish historical peaks. A forecast from five
usable timing pairs with incomplete evidence is explicitly labeled as such;
its `evidence_complete=false` does not authorize a formal start.

The statistics API also accepts an explicit normalized local campaign. This
interchange is for independently verified evidence import, not a way to make a
pilot formal or certify guessed booleans. It requires:

- `schema_version=1`, `phase`, `pairs_required=5|100`, `comparison_kind=native|tool`,
  `protocol_sha256`, `seed`, and the protocol's balanced `schedule`. Release
  qualification also requires `protocol_evidence={path,sha256}` for the actual
  approved protocol file. The comparator binds phase, fixed N, comparison kind,
  seed, pair order, identity and arm allocations to that file. Arm receipts and
  target roots must also match the exact preallocated pair/attempt/arm output
  layout under its run root; changing a
  normalized campaign cannot turn a pilot into formal evidence. CLI `--protocol`
  supplies this absolute local reference when normalizing a runner campaign.
- `identity`: source commit/tree, lock/resources hashes, full rustc/cargo/MSVC
  compiler/linker versions and hashes, Windows host, jobs=2 and test_threads=2.
- `job_names`: exactly `check`, `domain-ports`, `cli`, `sqlite`, `application`,
  `clippy`, in the fixed order.
- `records`: every `{pair_index, attempt, order, status, reason, baseline,
  candidate}`; indices start at 0. Only contaminated predecessors may be retried.
- Each successful arm: matching identity/protocol hash, `status=success`,
  verified source/process/artifact/quiet flags, a complete allocation scope of
  `owned_targets_and_stores`, `allocated_bytes`, primary `duration_seconds`,
  optional total `elapsed_seconds`, and six successful jobs with exit code 0
  and positive `duration_seconds`.
- Each arm additionally pins `arm.json` with `evidence={path,sha256}`, an
  absolute source root and canonical config hash. `feedback_phases` projects
  all six feedback-warm and edit job receipts and phase wallclocks. The
  comparator rereads the arm, invocation logs, emitted Cargo artifact paths,
  final artifact hashes and `owned-allocation.json` payload/hash linkage.
- `qualification_receipts`: `edit`, `clippy`, `diagnostics`,
  `optional_features` and `release` are **file references** `{path,sha256}`,
  not inline passing objects. All referenced paths are absolute and local;
  missing, changed, malformed or identity-mismatched proofs fail closed.
  Qualification JSON has `kind=build-feedback-qualification`, version 1,
  qualification name, source/toolchain identity, protocol hash, complete
  passing status and explicit `samples` keyed by pair_index/attempt/arm.
- Edit/release require exactly one baseline and candidate sample for each
  valid predetermined pair (5/100); old attempts, duplicates and omissions
  fail. Edit samples reference that arm's exact `arm.json`; timing comes from
  its edit phase wallclock. Clippy/diagnostics/optional-feature proofs must
  cover both configurations with actual commands; they do not invent fixed-N
  timing claims. Optional inline durations/arrays must equal captured values.
- Other samples pin `build-feedback-qualification-command` JSON with matching
  pair/attempt/arm, absolute source root, config hash, immutable identity and
  protocol hash. Each includes a successful `build-feedback-process` capture
  (argv/cwd/PID/timestamps/returncode/process-tree/quiet/24-GiB limit), both
  stream hashes, artifact references and a hashed qualification report.
  The report binds the same pair/attempt/arm/source/config, argv, exit code,
  canonical capture hash, artifact references and stream hashes to nonempty
  passing checks. A status flag by itself is never qualification evidence.
- Diagnostics report checks pin actual source files and expected/actual
  file/line; those exact file:line locations must appear in command output.
  Release proofs pin the unchanged official `core_beta_benchmark.py`
  validator, its invocation and artifact/report hashes plus `official_report`
  as an absolute hashed file reference. The comparator loads only the official
  validator pinned in the approved frozen source manifest and exact arm
  worktree, through `validate_report(path, expected_profile="full")`. The
  coordinating checkout's sibling script or a receipt-selected arbitrary script
  cannot replace that approved source. It rechecks the validator and report
  hashes after validation, retains original validation errors, requires
  originating baseline build evidence, matches commit/lock/compiler/Cargo/binary
  identity, and binds the full run, workspace, output directory, release profile
  and deterministic dataset to this source. The captured command must directly
  execute that script's `run` subcommand, and the complete report run interval
  must be contained within the capture interval. The official suite executable is
  the Cargo artifact copy at `suite_root/baseline-cli.exe`. Operators must
  generate receipts from actual official runs; existing official validation is
  unchanged.
- Optional `pilot_store_peak_allocated_bytes`: an independently measured store
  peak for the reservation forecast. Missing store peaks are reported as such.

**Release workload evidence:** timing comes exclusively from the verified
full report's `baseline.anchor.observations[*].process_wall_ms`. Neither enclosing
harness `capture.duration_ms` nor inline duration/sample arrays establish CLI
performance. Each arm must contain these successful measured invocations:

| Report metric | Invocation | Samples per arm | Measurement state |
| --- | --- | --- | --- |
| `cli_startup_warm_latency_ms` | `--version` using the suite binary | 20 | warm |
| `cli_startup_cold_latency_ms` | `--version` using a fresh binary copy | 20 | `cold_best_effort` |
| `search_latency_ms` | JSON `search` after shrink | 100 | warm |
| `show_latency_ms` | JSON `show` after shrink | 100 | warm |
| `get_latency_ms` | JSON `get` after shrink | 100 | warm |

The copied-binary state does not mean OS caches were flushed. Invocation
indices, operations, phases, argv, exit codes and legacy/raw sample links must
match the official report. Both startup operations use observation phase
`startup`; their warm/cold state is recorded separately in the metric. Raw
process timings must be finite and positive,
observations must be serial and contained within their captured command, and
paired commands must follow the approved order without overlap. Dataset, state
and release-profile facts must match across arms and pairs. Reusing an official
report hash across arms or pairs fails qualification.

An outer paired run is the independent sampling unit. The release comparison
uses 5,000 seeded paired cluster resamples, retaining every inner invocation
within each selected pair and calculating pooled nearest-rank P50/P95. Each of
the five workloads requires a candidate/baseline upper 95% interval bound
<=1.05 for both estimators. Results use `release_<report metric>` keys. Missing,
changed, failed or incomplete raw evidence retains
`qualification_release_cli_workload_timing_unverified` and remains unqualified.
The import path is implemented; actual qualifying paired reports have not yet
been generated for this campaign.

Use nearest-rank P50/P95, without interpolating the even-N median. Ratios are
candidate/baseline; reduction percent is `100*(1-ratio)` and speedup factor is
`baseline/candidate`. A fixed PRNG seed and 5,000 bootstrap resamples preserve
the baseline/candidate indices together. The central percentile 95% interval
uses nearest-rank endpoints at 2.5% and 97.5%. This is the predeclared percentile
bootstrap, not an adaptive significance search or a claim of BCa correction.

| Comparison | Required bound |
| --- | --- |
| Native profile/incremental/layout | Allocated P50 ratio upper 95% bound <=0.90, allocated P95 <=1.05; primary and all primary/feedback job P50/P95 upper bounds <=1.05 |
| Cache tool | Primary P50 ratio upper 95% bound <=0.85; primary P95 and all primary/feedback job P50/P95 upper bounds <=1.05 |
| Both | Edit and each of the five release workloads P50/P95 upper bounds <=1.05; all independent qualification receipts pass; complete exact fixed-N evidence |

Uncertain bounds fail adoption. Pilot output has **no `adopted` field** and
`qualified=false`; formal output may set `adopted=true` only when every
requirement passes. Incomplete/failed evidence retains raw statuses and reason
codes. Missing qualification receipts mean `qualified=false` even if point
estimates look good. Select at most two finalists: qualified native A versus
original native, then tool B versus A; if A fails, repilot B versus original.
No rejected pilot-only variant receives a formal claim.

Exit 0 means inventory is complete, preview is allowed, run has completed its
fixed N, or compare is fully qualified. Exit 2 means blocked, incomplete,
pilot-only, unqualified or invalid input. JSON errors go to stderr. Existing
output receipts are preserved; choose a new explicit local path per report.
Preview and stdout comparison are read-only; writing a requested local result
is the only mutation performed by those CLI reporting commands.

## Recorded operational state (2026-10-19)

The corrected initial inventory was complete with zero errors and froze
**B = 145.6929 GiB allocated**. That baseline remains fixed. A related-cache
scan then detected drift from another live Beta build; admission stopped
before Cargo and preserved the refusal evidence. External-window growth
consumes allowance; it does not justify rebasing B.

After same-target reuse was corrected, the native incremental-on/on trial
still has a conservative **39 GiB predicted growth**, not a measured pilot
peak or a qualified store reservation. The retained disk-refusal receipt
observed **113.249 GiB** free; subtracting the forecast leaves **74.249 GiB**,
below the required **80 GiB** floor. Quick disk admission refused the build
before Cargo and saved an immutable reason receipt. Later samples continued to
show insufficient space; the continuation observed approximately **76.04 GiB**
free, below the **119 GiB** needed to start that pair. Free space is an
observation, not a permanent guarantee; a subsequent attempt must recompute
admission against the same B.

**Pilot: 0/5 pairs; formal: 0/100 pairs.** No further build attempt is made
in this final integration state. The task remains `in_progress`.
There is no cleanup authority assumed, no baseline rebasing and no empirical
speedup or completed pilot/formal claim. Product source, shared Cargo defaults
and release profiles remain unchanged. Runner source verification is recorded
only after final raw-manifest and Git revalidation; conservative JobIO bounds
protect admission but do not replace formal owned-allocation evidence.

## Retention and validation

The harness performs no automatic GC or cleanup. Only the coordinator may
recycle individually previewed and owner-approved successful disposable
trial target paths, and must verify Recycle Bin movement before any purge.
Daily caches, stores, seed outputs and failed attempts protect at least 72
hours. Do not use `cargo clean`, broad recursive deletion, old-cache cleanup
or manual `.git` edits in this campaign. The long-term 40/60 GiB total-cache
goal is separate from this fixed-baseline campaign and is not claimed achieved.

The focused Python suites use synthetic in-memory evidence and temporary
files, not Cargo builds or real transcript data:

```text
python -m unittest discover -s scripts/evidence -p "test_build_feedback_*.py"
python scripts/evidence/build_feedback_benchmark.py preview --help
python scripts/evidence/build_feedback_benchmark.py compare --help
```

When the local Ruff and Mypy tools are available, check all ten current helper
and test files, including retention, with `ruff check --target-version py310` and
`mypy --no-incremental --follow-imports skip --python-version 3.10`. These are
development checks, not runtime dependencies. The earlier `e6162ad` checkpoint
passed 116 aggregate tests, including 48 runner tests. That is historical
validation of the earlier implementation. The release-import continuation
passed **127 aggregate tests** (inventory 28, CLI 14, runner 49, statistics 36),
all eight files in Ruff/Mypy, Rust formatting and whitespace checks. The actual
approved frozen validator also passed a complete synthetic full-report import
smoke with source revalidation and raw sample extraction. These results verify
implementation and format compatibility; they are not release performance data.

Native Windows tests check real allocation handles, hardlink/alias dedup,
Job Object descendants, process/output limits and artifact identity as
implemented by the corresponding inventory/runner suites. A synthetic pass
is not a real workload pilot, formal campaign, hosted release check or tool
adoption. Parent review owns actual inventory, operational admission,
optional-feature/release qualification and the final scoped commit.
