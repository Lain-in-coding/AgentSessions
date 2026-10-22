# Measurement methodology and index diagnostics

Planning research, 2026-10-14. This file proposes the protocol for review; it
contains no new benchmark results. Product paths below are relative to the
canonical checkout identified by the task PRD, not the coordination checkout.

## Methodology evidence

The primary references were fetched and read on 2026-10-14:

1. [Google Benchmark user guide](https://google.github.io/benchmark/user_guide.html),
   sections on warmup, repetitions, JSON output and reporting. Warmup values are
   excluded from reported measurements; repeated runs and their context remain
   available. These principles transfer to this task; the microbenchmark
   library and its default iteration calibration do not.
2. [pyperf result analysis](https://pyperf.readthedocs.io/en/latest/analyze.html),
   sections on raw values, outliers, distributions, median and MAD. Preserve
   samples and inspect variability rather than selecting the fastest result.
3. [ACM artifact review and badging](https://www.acm.org/publications/policies/artifact-review-and-badging-current),
   sections on functional artifacts and results validation. Reproduction needs
   documented, complete, exercisable artifacts; repeated local execution does
   not constitute independent reproduction or an ACM badge.

The exact workload sizes, repetition counts and resource bounds below are
task-specific proposals, not numbers prescribed by those references. No new
pyperf/Google Benchmark dependency, global system tuning, cache flushing,
security-service changes or external benchmark framework is needed.

## Proposed workload and run protocol

- Keep the existing `full` anchor: 20 files x 200 generated messages, 20
  startup measurements per variant, three fresh/noop/shrink trials, and 100
  measurements per existing query operation. Preserve the original profile
  dimensions, metric names and sequence; make its post-shrink query state
  explicit rather than changing historical report interpretation.
- Add a separately validated optional baseline extension to the same harness.
  Its fixed synthetic scale matrix is 10/50/250 files x 200 messages: 2,000,
  10,000 and 50,000 generated messages. Query the intact corpus at each scale,
  before shrinking to 1,000/5,000/25,000 retained messages. Record all phases.
- Produce three independent complete report runs, with new corpus/store roots
  per run and fresh stores per sync trial. Execute serially, with scale orders
  small/medium/large, medium/large/small, and large/small/medium. Record order;
  no concurrent builds, tests, benchmark agents or trace collection.
- For each diagnostic scale/run, take three independent initial/noop/shrink
  sync trials. Initial sync uses a fresh database. Noop means one unchanged
  sync immediately following that trial's initial sync, not a general claim
  about arbitrary repeated maintenance. Query sampling uses the last trial's
  intact database, followed by that trial's shrink sync and correctness check.
- Before each diagnostic query class or retrieval operation, execute three
  untimed warmups on the same intact store. Preserve separate warmup records.
  Measure 100 invocations per search class, 100 each for show/get, and 100 each
  for context mainline/full. Keep query classes separate; use deterministic
  round-robin ordering across search classes after all class warmups.
- Use the five existing query strings: `benchmarktoken`, `alpha`, `配置`,
  `"src/main.rs"`, and `EVIDENCE42`. They describe synthetic retrieval paths,
  not multilingual recall quality or realistic selectivity. Use explicit
  output/limit/context-budget flags and validate expected results.
- Startup is independent of corpus size. Reuse the anchor's startup series;
  do not duplicate it per scale. The copied-binary variant is only
  best-effort cold startup. OS cache state is uncontrolled and recorded as
  such; new processes do not establish a cold page cache.

## Timing and interpretation

- Opt-in primary children explicitly remove inherited `ASG_INDEX_TRACE`,
  `ASG_CURRENT_REPO` and `ASG_CLOCK_MS` and run from the frozen product source
  root. Record trace-disabled, repo-derived, system-clock and logical-cwd
  policies plus the safe repo identity/UTC interval; verify three-run agreement.
  Preserve normal system bootstrap variables and the parent environment.
  Only the separate trace trial receives its owned trace target.
- The new extension's primary measurement is end-to-end subprocess wall time,
  including process creation, CLI setup, database open/close, output and exit.
  Capture its stop timestamp after output/process completion and before monitor
  cleanup. Preserve the legacy nine metric durations and the shared
  `run_process` default/return contract, including observer cleanup, because
  gate/semantic harnesses import it. One shared process lifecycle may expose
  both observations; do not duplicate runners. Record corrected observations
  for anchor invocations as well as scale diagnostics, with distinct timing
  protocol IDs. Legacy and corrected times are not matched improvements.
- Record optional Robot `meta.duration_ms` only as a separately named,
  integer-millisecond CLI pre-render observation (parse/open/dispatch/drop/wake).
  It excludes some process/output work and is not pure SQLite time. Never infer engine latency
  by subtracting separately sampled startup medians.
- Keep all successful raw samples and all failure/abort records. No trimming,
  best-run selection or silent retries. Report sample counts, per-run P50/P95
  for the 100-sample query series, median absolute deviation, and the range of
  the three run medians. Preserve the existing percentile convention and test
  the recomputation against raw values.
- Three-point sync series and twenty-point startup series have limited tail
  resolution. Their legacy P95 fields may remain for compatibility, but the
  narrative emphasizes median/range and does not certify a tail-latency SLO.
  Do not pool different classes, corpus sizes, phases or protocols.
- For new summaries, use the existing nearest-rank function
  `Qp(x) = sorted(x)[ceil(p*n/100)-1]`. Define `M=Q50(x)` and unscaled
  `MAD=Q50(abs(x_i-M))`; never substitute the average of two middle values.
  Calculate from the persisted six-decimal-ms raw values without intermediate
  rounding; round summary/display values only at the existing output precision.
- For three comparable run P50s `m`, define spread as
  `100*(max(m)-min(m))/Q50(m)`. Warn only when it is greater than 15 percent.
  If the denominator and range are both zero, spread is zero. If the denominator
  is zero but the range is positive, encode spread as null, warn, and record
  `zero_median_with_positive_range`; do not emit infinity. This is a noise
  warning, not a performance gate. Keep all results and defer small speed
  claims; additional runs need an explicit reason and remain visible.
- Initial-sync throughput uses that trial's actual generated messages/source
  bytes divided by its wall time. Do not label unchanged scans or tombstone
  deletion as new-message indexing throughput. Store size/ratio samples must
  identify database/journal/WAL coverage and the corpus phase of the divisor.
- Periodic RSS observation is an approximate process high-water observation,
  not guaranteed peak memory. Record method, interval and observation count;
  an unobserved short process is null with a reason, never a fabricated zero.

## Bounded execution

Proposed measurement limit: one serial suite of three complete runs, at most
50,000 generated messages per diagnostic workload, at most 60 minutes of
measurement time, and at most 300 seconds for one child command. Run 1 builds
and freezes B/receipt before starting the suite clock; runs 2/3 only verify and
reuse that B. Build/test time is separate and never included in performance
samples; there are no intervening builds. Check at least
8 GiB of scratch space before the suite. Stop before a new trial if managed
scratch exceeds 4 GiB or observed benchmark-process RSS exceeds 4 GiB; these
sampled/boundary checks are guardrails, not hard OS memory or disk quotas.

A deadline or resource stop is an incomplete run with a recorded reason,
non-success suite result and retained partial evidence. It does not pass the
matrix, silently lower a scale, substitute smoke, or trigger a global machine
change. Cleanup may remove only the exact temporary roots created by the
harness, after validating their ownership; never touch provider directories.

## Existing ASG_INDEX_TRACE evidence

Both `crates/agent-session-grep-cli/src/trace.rs:13` and
`crates/agent-session-grep-adapters-sqlite/src/trace.rs:13` cache the target in
a crate-local `OnceLock`. With no `ASG_INDEX_TRACE`, no trace file is opened.
The emitter (`trace.rs:44`) appends `asg.index-trace/v1` records containing
`pid`, `t_ms`, `label`, `stages_ms`, and `detail`. File-open/write errors are
ignored (`trace.rs:63`), so a successful CLI exit does not prove a trace exists.

| Label | Current stages and evidence anchors |
| --- | --- |
| `cli:sync` | `prelude`, `namespace`, `capture`, `jsonl_health`, `stage`, `assemble`, `commit`; CLI `src/lib.rs:5244` |
| `adapter:catalog` | `validate_incoming`, `noop_probe`, `load_catalog_state`, `merge_sources`, `merge_stored`, `tombstones`, `manifest`, `outbox_intent`, `apply_commit`, `finalize`; adapter `src/lib.rs:3718`, `:3809`, `:4095`, `:4368`, `:4443`, `:4469`, `:4546` |
| `adapter:commit` | `verify_outbox`, `affected_sessions`, `catalog_upserts`, `fts_projection`, `catalog_deletes`, `relation_upserts`, `source_replacements`, `session_projection`, `integrity_checks`, `generation_activate`, `tx_commit`; adapter `src/lib.rs:6869`, `:6933`, `:6967`, `:7192`, `:7364`, `:7438`, `:7457` |
| `adapter:fts` | `fts_projection_rows`, `fts_ids_insert`; adapter `src/lib.rs:7767` |
| `adapter:bytes` | Approximate counters in `detail`, not stage durations or RSS; adapter `src/lib.rs:4521` |

CLI `commit` wraps `commit_source_batches_if_changed` (`src/lib.rs:5221`);
adapter `apply_commit` wraps the inner commit (`src/lib.rs:4457`), which in
turn includes FTS projection. These scopes overlap. Do not sum labels into
one wall-clock total or infer a complete flame graph. Each crate initializes
its own `ORIGIN` at first emit (`trace.rs:54`); `t_ms` is not a shared timeline
across crates. Early noops omit deeper stages (`adapter src/lib.rs:3722`).

After all primary measurements, use one additional 50,000-message
initial/noop/shrink trial in a separate store with trace enabled only in its
child environments. Check that files exist, records parse, labels and phase
counts are appropriate, and values are finite/nonnegative. Missing output is
an unavailable diagnostic, not an empty successful trace. Preserve every
record's invocation identity and order. Raw traces stay private; publish only
reviewed aggregate phase observations. No Rust instrumentation change belongs
to this task.

## Comparison and follow-up rules

Compare only compatible binary/toolchain/features, corpus/query identities,
timing/warmup/output protocols and host conditions. The old `88d86f4` report
is a historical anchor, not a matched control. New runs and the unchanged
legacy artifact can coexist without claiming a measured improvement.

Rank observed operations by cost and scale behavior. Use trace observations
to narrow indexing hypotheses, keeping overlap/coverage limitations explicit.
Recommend one subsequent optimization task with the affected operation,
evidence, correctness invariants, repeatable workload and proposed measurable
performance criterion. Do not promise a speedup or implement the optimization
inside this evidence task.
