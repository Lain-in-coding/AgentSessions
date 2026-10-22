# Current-source performance baseline design

Status: implementation approved on 2026-10-14. This is an evidence-tooling and
measurement task. No runtime optimization or new performance result is claimed.
Paths below are relative to the authoritative product checkout unless they
begin with `.trellis/`.

## Boundary and dependencies

The canonical product main observed locally and remotely on 2026-10-14 is
`8e05eb495160d5ff5979cdbef6f63856d5a7de81`. The preceding
`10-13-gate-evidence-validation` child is complete and its repair is included
in that revision. Its six-metric gate and historical 25-check acceptance do
not provide this task's performance evidence.

This child delivers one reproducible baseline bundle and diagnosis to
`08-15-benchmark-install-open-source-gate`. Tooling preparation and subsequent
measurement/publication are dependent stages of that deliverable, not two
independent product features. Keep one task, with separate atomic commits.
The parent retains installation, broader scale/platform and release decisions.

Use the existing Core/Beta harness, generator, hashing, subprocess lifecycle,
statistics and Markdown renderer. Do not introduce another benchmark runner,
external benchmark dependency, model, schema file or Rust instrumentation.

## Compatibility and process observations

Keep `schema_version=agent-session-grep.core-beta-benchmark/v1`, `PROFILES`,
the nine legacy metric names/counts, legacy corpus bytes, sampling sequence,
nearest-rank statistics and the existing `run_process` default/dict contract
(`duration_ms`, `peak_rss_mb`, `stdout`).
The gate and semantic harnesses import that helper and must retain their
current behavior. Old smoke/full reports remain valid without an extension.

Add one optional top-level `baseline` object with
`schema_version=agent-session-grep.current-source-baseline/v1`. It is opt-in;
existing smoke CI does not run the diagnostic suite. The current old validator
ignores unknown top-level keys, so only the new identified checker can certify
the extension. Presence with an unknown version, invalid shape, missing
coverage or invalid values is an error, never an ignored optional feature.

The process lifecycle records two timestamps without launching a second copy
of a measured command:

1. `process_wall_ms`: before process creation through output/process completion,
   stopped before monitor cleanup. Protocol `process-wall-before-observer/v1`.
2. The existing elapsed duration after sampler cleanup, retained in the nine
   legacy metrics. Protocol `legacy-wall-with-observer/v1`.

Capture corrected observations for the original anchor invocations as well
as new scale invocations. Associate anchor observations with their existing
metric/sample index. Do not pool the two clocks or describe their difference
as product optimization. An internal detailed result/helper may be introduced
inside the same script; preserve the public helper adapter and avoid two
independent process runners.

Per-invocation data includes ordinal, workload, phase, trial, query class or
entity/context anchor, redacted reproducible argv, start/end time, exit and
outcome, wall duration, stdout byte count including final LF, optional
`cli_reported_duration_ms`, and nullable memory observation with method,
interval/count and reason. CLI-reported time is a coarse pre-render observation
covering parse/open/dispatch/drop/wake; it is not pure engine time. Do not
publish full command stdout, ambient environment or raw trace files.

For opt-in baseline children, clone the launcher environment while preserving
normal executable/system bootstrap settings, then explicitly remove
`ASG_INDEX_TRACE`, `ASG_CURRENT_REPO` and `ASG_CLOCK_MS`. Use the frozen product
source root as cwd. Record the effective policies as trace disabled, repo
derived from that source checkout, system application clock, and logical cwd
`source-root`; capture the UTC run interval and safe repository identity.
Three-report validation requires the same policies/repository context. The
separate trace trial uses the same policy plus its owned trace target. Do not
modify the parent/global environment or non-opt-in/imported-helper behavior.

## Workload matrix

| Workload | Generated layout | Query phase/messages | Shrink result | Purpose |
| --- | --- | --- | --- | --- |
| Existing full anchor | 20 x 200 | Existing post-shrink / 2,000 | 2,000 | Preserve v1 workload, expose its actual phase and corrected observations |
| `scale-2000` | 10 x 200 | Intact / 2,000 | 1,000 | Small baseline with explicit scope |
| `scale-10000` | 50 x 200 | Intact / 10,000 | 5,000 | Bounded growth observation |
| `scale-50000` | 250 x 200 | Intact / 50,000 | 25,000 | Largest approved local diagnostic |

Files are input files, not an asserted number of canonical Sessions. Record
generated source occurrences, observed Message/Placement/Session counts and
phase fingerprints separately. Keep the existing deterministic Claude fixture
semantics. No implicit discovery of user directories is allowed.

The exact repetition/warmup/order/resource protocol is in
`research/methodology.md`. In brief, three independent full-plus-diagnostic
report runs each use fresh roots, three sync trials per scale, three untimed
query warmups, 100 observations per each of five query classes, 100 each for
show/get, and 100 each for mainline/full raw context. Legacy full sample counts
remain unchanged. Rotate the three scale orders across the three runs.

Each diagnostic trial is initial sync then one noop, with a later shrink.
Use only the final trial's intact database for timed queries; shrink it after
the queries. Noop describes that exact immediate-after-initial condition.
Append ingestion, directory discovery, concurrent load, sustained background
maintenance and adversarial query distributions are outside this matrix.

Diagnostic search is explicitly lexical, first page of 20, with fixed output
and budget flags. Preserve the five existing query strings and categorize
their raw samples separately. For context, collect observed Session wire IDs
through an untimed verified lookup; never invent them from Message IDs.
Use raw level, explicit `--policy mainline|full`, `--max-messages 500` and
`--max-bytes 4194304`. Choose one deterministic eligible synthetic Session
per scale with an oracle small enough to fit both budgets; keep that Session
fixed within the run. Record its actual membership and anchor identity. Choose
it by deterministic generator file/record identity, not temporary-path or
canonical-wire-ID ordering. Likewise freeze the show/get generator-ID sequence
across runs and map it to each run's observed wire IDs before measurement.
Unexpected partial output is failure; intentionally truncated context is
deferred. Show/get remain separately named single-entity retrieval.

## Correctness before performance

Use generator-defined native identities, content, parent and sidechain
relationships as the oracle, mapped to observed canonical wire identities.
Do not derive the expected graph by copying a successful response. Reuse
existing CLI/adapter test contracts rather than creating a second product
implementation in the benchmark.

- Initial sync: exact synthetic source/occurrence accounting, zero skipped or
  retained/deferred input, and expected visible entity/placement census.
- Noop: no new commit and unchanged active generation; distinguish unchanged
  messages from newly emitted/committed messages.
- Shrink: exact retained census, all removed fixture identities absent, all
  retained identities available, expected source replacement and generation.
  Untimed read-only census/fixture verification may be used; do not turn
  checking into an additional timed ingestion step.
- Search: correct command/outcome, nonempty and relevant expected results,
  explicit query/mode/budget, correct generation and requested first-page
  behavior. This is a retrieval correctness check, not recall@k certification.
- Show/get: requested identity and fixture role/content match the response.
- Context: expected policy-specific graph membership/order, message/placement
  and evidence alignment, leaf selection, effective level and no truncation.
  Full order follows the product contract, not assumed source-file chronology.

Time successful execution only; parse and correctness assertions run after
the external stop timestamp. Any failure keeps its attempt and invocation
records and makes the run unsuccessful. Never convert it to zero, omit it to
improve the distribution, retry silently, or weaken existing exit behavior.

## Extension contract and validation

The extension is a closed versioned contract. Its logical sections are:

| Section | Required meaning |
| --- | --- |
| Identity | suite ID, ordinal 1/2/3, UTC interval, source/harness/checker identities, binary/build receipt, protocol versions |
| Plan | fixed workload definitions, query/context settings, scale order, warmups/counts, bounds and clock/memory methods |
| Anchor | original full phase/census/hash metadata; ordered corrected observations bound to legacy samples |
| Workloads | exactly the three scale IDs; per-phase input/hash/census/store snapshots; trial, warmup and measured observations |
| Summaries | per-operation/per-class raw-derived statistics; no cross-phase/clock/class pooling |
| Correctness | bounded checks and observations that bind measured results to the fixture oracle |
| Coverage | complete requested matrix and limitations; no missing operation represented as successful |

Reject bool-as-number, NaN/infinities, negative measurements, malformed
containers, duplicate IDs, unsupported phases/protocols, wrong sizes/counts,
invalid lineage, unexpected outcomes and inconsistent statistics. Validate
relationships between raw duration, source bytes, throughput and size ratios.
Nullable memory values require reasons and retain sample alignment. The
legacy validator's unrelated historical gaps are not silently declared fixed.

`validate-report` validates an extension when present while preserving absent
extension compatibility. Add a narrow `validate-baseline` subcommand to the
same script: it requires three distinct complete reports, the same suite/S/H/B,
compatible environment/protocol/corpus inputs, all three ordinals/orders, and
valid raw-derived summaries. It fails if given three copies of one run or
legacy-only evidence. Validation proves internal consistency, not authenticity
against an adversary who rewrites every correlated field.

For resource/error stops, retain structured attempt evidence only under the
ignored local output root, including the failed phase and successful records
so far. It must not masquerade as a completed v1 report. A baseline publication
requires all three complete reports; failed attempts and reasons remain
disclosed, not cherry-picked away. There is no timing threshold for task
success; substantial run-to-run noise remains an explicit limitation.

## Source, build and publication lineage

1. Recheck canonical main before implementation. Create a feature branch based
   on it. Prepare only the three tooling files listed below, verify them and
   form clean tooling commit S through the authorized commit workflow.
2. Run the harness and checker from S itself (H=S). Record tracked/index and
   relevant untracked cleanliness before and after, script/lock hashes, target,
   features, toolchain, allowed build variables and actual binary B. Outputs
   belong under ignored `evidence-output/`; source dirt cannot be hidden by
   excluding arbitrary untracked files.
3. In opt-in run 1, build release once with locked dependencies and confirm
   the actual executable output. Do not trust an old hard-coded binary location
   when target settings redirect Cargo. Freeze B and a hash-checked receipt in
   the common ignored suite output root, then start the shared suite deadline.
   Runs 2/3 require that matching receipt and reuse B without invoking Cargo;
   verify S/H/B, features and environment before each run. No build occurs
   after the suite clock starts. Reject external `--binary` with this opt-in
   suite; ordinary non-suite `--binary` remains supported as before.
   A report consuming any prebuilt binary, including run-2/3 receipt reuse,
   retains `caller_supplied_prebuilt` and its attestation caveat. The extension
   separately records linkage strength and the originating run-1 build receipt;
   it never upgrades the legacy provenance label or claims a fresh build.
4. Confirm the linked SQLite through the exact locked bundled crate, resolved
   features, override checks and this build's output. The researched locked
   package is `libsqlite3-sys 0.38.2`, bundled header 3.53.2. Revalidate at S;
   do not assume a future build uses it. Mark this as build-derived evidence,
   not a CLI runtime query. The stale WAL spike and Python SQLite are invalid
   substitutes. Missing linkage proof blocks qualified publication.
5. After measurements, create publication commit P containing only new reports,
   projections, a manifest and the existing performance-document entry. Each
   report continues to name S. Use an unambiguous S prefix for the directory;
   write full S and file hashes in the manifest. Do not embed P's future hash
   into a file inside P or relabel P/merge-preview/merge as the measured source.
6. Run actual checks and current-head owner acceptance for the PR and any
   ordinary merge as required by canonical repository rules. A later source
   change needs remeasurement or preservation of the old S label. No release,
   independent reproduction or cross-platform claim follows from local runs.

All paths and build/environment displays in public reports are normalized
relative paths or reviewed logical labels. Preserve reproduction-relevant
settings without hostnames, usernames, private roots or whole-environment dumps.
Raw diagnostic traces and governance findings remain private.

## Index diagnosis and follow-up

Use existing `ASG_INDEX_TRACE` only for one separate largest-scale
initial/noop/shrink trial after primary measurements. The exact labels,
nesting, missing-output checks and per-crate clock limits are documented in
`research/methodology.md`. Publish only reviewed stage aggregates and clearly
label them instrumentation-enabled observations. They never enter primary
latency distributions. An unavailable trace stays unavailable.

The final manifest ranks observed operation costs and scale behavior,
distinguishes hypotheses from demonstrated stage costs, and recommends one
follow-up optimization with correctness and measurable performance acceptance.
Existing optimizations and old figures remain historical context. This task
does not implement the recommendation or promise a particular speedup.

## Exact product write set and rollback

Tooling commit S:

- `scripts/evidence/core_beta_benchmark.py`
- `scripts/evidence/test_core_beta_benchmark.py`
- `docs/product/SLI-AND-BENCHMARK-FORMAT.md`

Publication commit P:

- `docs/evidence/core-beta/<S-prefix>/manifest.md`
- `docs/evidence/core-beta/<S-prefix>/run-01/`, `run-02/`, `run-03/`, each
  containing `core-beta-benchmark-full.json` and its generated `.md` projection
- `docs/performance-baseline-0.2.md` (new dated entry; preserve historical text)

No Rust, Cargo lock/manifest, CI, gate/semantic harness, old evidence directory,
scanner/governance configuration or operations-matrix changes are included.
If one becomes necessary, revise the plan before making that change. Keep
private task/runtime/journal material out of product commits. Both protected
coordination spec hashes must remain unchanged.

Default operation and historical reports remain usable when the extension is
not requested. Before S, fix or revert only owned edits without touching others'
work. After S or P is shared, corrective commits preserve existing history;
no amend/rebase/force push. Failed or superseded experiments retain their source
labels and reasons; never overwrite an older evidence bundle.
