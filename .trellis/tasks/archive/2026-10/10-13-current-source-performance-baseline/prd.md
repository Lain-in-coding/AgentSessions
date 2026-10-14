# Establish current-source performance baseline

## Goal and user value

Establish a reproducible current-source local Windows performance baseline
and use measured evidence to rank the next optimization opportunities. Future
improvements must be assessed against an explicit workload, environment,
correctness contract and repeatability envelope.

This child owns evidence and diagnosis, consistent with its benchmark parent.
It does not implement a speculative Rust optimization or certify a release.
Task-creation/planning consent was received on 2026-10-13. The final plan
assembled on 2026-10-14 was explicitly approved by the user's subsequent
message, "批准实现！". Implementation and measurement may now proceed within
the reviewed scope; current-head merge acceptance remains separate.

## Confirmed background

- Product checkout: `.trellis/.runtime/attribution-closeout-20261012/canonical-repaired`.
  Clean local HEAD and remote main were read back on 2026-10-14 as
  `8e05eb495160d5ff5979cdbef6f63856d5a7de81`; recheck before execution.
- The preceding `10-13-gate-evidence-validation` child was merged through
  PR #18 on 2026-10-13. Its PR head and merged main passed 25 checks. That
  dependency is satisfied, but does not validate this new performance work.
- The checked-in full report is
  `docs/evidence/core-beta/88d86f4/core-beta-benchmark-full.json`, stamped
  `88d86f44a0f8d4d0261aaddddb99524514e57d7b` and `locally_verified`.
- `scripts/evidence/core_beta_benchmark.py::PROFILES` (`:30`) defines full
  as 20 files x 200 generated messages, 20 startup observations per variant,
  three sync trials and 100 per query operation. Queries use the last trial's
  already-shrunk 2,000-message corpus (`:398`, `:416`), and five search
  classes are pooled. Generated count is not a verified catalog census.
- `show/get` are single-entity reads, not session context
  (`crates/agent-session-grep-application/src/lib.rs:2115`;
  `crates/agent-session-grep-cli/src/lib.rs:5402`). Existing `context`
  supports mainline/full and explicit budgets (CLI `src/lib.rs:2787`).
- The legacy timer includes sampler cleanup (`core_beta_benchmark.py:141`),
  copied-binary cold startup does not control the OS cache (`:339`), and
  periodic memory observation can be unavailable (`:89`, `:151`).
- `.github/workflows/core-beta-evidence.yml:101` runs Core/Beta smoke.
  Its separate six-metric gate has a distinct schema and purpose. The Core
  validator closes metric/profile sets but ignores unknown top-level keys
  (`core_beta_benchmark.py:536`); an extension needs new validation.
- `docs/product/SLI-AND-BENCHMARK-FORMAT.md:56` requires source, binary,
  corpus, environment and raw-sample evidence; its SLO status remains Draft.
  SQLite linkage must come from this build, not Python or the old separately
  locked WAL spike; see `research/provenance-publication.md`.
- The parent's old implementation checklist includes already-implemented
  features and obsolete NA assumptions; current code and contracts govern.

All product anchors refer to the canonical checkout above. Research records
the full supporting paths, exact behavior and limitations.

## Requirements

| ID | Requirement | Acceptance |
| --- | --- | --- |
| R1 | Bind runs to exact clean source, harness/checker, built binary, synthetic corpus phases and environment identities. Record build-derived SQLite provenance honestly. Preserve measured-source identity through later publication; do not relabel historical or merge-preview evidence. | AC1 |
| R2 | Preserve the existing full anchor and add 2k/10k/50k intact-corpus diagnostics for corrected startup observations, initial/noop/shrink sync, five separate lexical query classes, show/get and raw mainline/full context. Keep raw samples, correctness/census and coverage limits, including memory and response size. | AC2 |
| R3 | Run three independent serial full-plus-diagnostic reports with specified warmup/counts/order and comparison eligibility. Distinguish corrected process wall time from legacy observer-inclusive time, cache assumptions and coarse internal observations; retain noise and failure evidence. | AC3 |
| R4 | Produce privacy-reviewed reproducible evidence and a bounded diagnosis, distinguishing slow operations from unproven internal causes. Keep instrumented index-stage observations separate from primary latency. Resource stops are incomplete results. | AC4 |
| R5 | Recommend the first follow-up optimization and measurable correctness/performance criteria without promising an unmeasured speedup. | AC4 |
| R6 | Reuse the existing harness, statistics, subprocess lifecycle and renderer. Keep legacy v1 profiles/metrics and shared-helper defaults compatible; validate the optional versioned extension and complete three-report suite. Restrict changes to the design's tooling/publication write set and normal PR governance. | AC5 |

## Acceptance criteria

- [ ] AC1: exact source/harness/binary/corpus identities and environment
  metadata are recorded; reproduction commands identify the measured revision
  and all nondefault inputs. Three runs use frozen source and binary, with
  clean-state and actual build/SQLite linkage evidence. The bundle distinguishes
  measured source from publication and later merge commits.
- [ ] AC2: the approved matrix has raw samples and validated summaries,
  including three complete runs of each scale, 100 diagnostic observations per
  query class/retrieval operation, explicit warmups and all three sync phases.
  Initial/noop/shrink, query relevance, entity identity and context
  graph/evidence checks pass against synthetic expectations. Actual query
  phases and unavailable memory observations are disclosed; smoke does not
  substitute for full or scale sampling.
- [ ] AC3: repeated-run methodology and comparison eligibility are explicit.
  Raw records recompute per-class/per-operation summaries, derived throughput
  and size ratios. Legacy/corrected clocks stay distinct; every run and its
  instability are reported. Historical figures are not matched controls without
  compatible workload/protocol/build/environment evidence.
- [ ] AC4: findings rank observed costs, separate hypotheses, and define one
  optimization follow-up with testable correctness/performance acceptance.
  Original reports, generated Markdown, manifest, exact reproduction commands
  and limitations form a privacy-reviewed bundle. Missing trace is disclosed;
  an incomplete primary matrix cannot satisfy the task with surviving samples.
- [ ] AC5: applicable tests, independent review and privacy/governance checks
  pass. Historical full and ordinary smoke stay valid, consumers retain helper
  semantics, and invalid new fields/missing runs fail. Product changes are
  atomic and reviewable, with new-head owner acceptance before any ordinary
  merge. This child does not close parent release gates.

## Constraints and exclusions

Synthetic data only. Preserve historical evidence and keep private task,
runtime, journal, corpus stores, raw traces and scanner logs out of product
commits. Product source comes from the canonical checkout, not the coordination
root's older code.

The bounded proposal is one serial three-run suite, at most 50,000 generated
messages per diagnostic workload, 60 minutes of measurement, and 300 seconds
per child invocation. Scratch/RSS boundary guards and incomplete-run handling
are defined in `design.md` and `research/methodology.md`; they are not OS quotas.
Build/test time is separate.

Excluded: real transcripts; restricted competitor sources; model downloads;
global tuning/cache flushing/disabled security services; the roadmap's 50 GB
or 10-million-message scale; full Linux/macOS/CI expansion; runtime features,
providers or engine migrations; semantic/recall certification; intentional
partial context, append/discovery/concurrency workloads; SLO freeze, release
certification and competitor speed claims. No changes to Rust, Cargo inputs,
gate/semantic harnesses, workflows, scanners or old evidence are in scope.
Necessary expansion requires a revised plan.

Preserve both pre-existing dirty coordination specs byte-for-byte:

- `.trellis/spec/agentsessions-cli/backend/index.md`:
  `2F117568EE2D4504C030E2774AEA25F49084E5B09537CCF2D24D09C70D8907B7`
- `.trellis/spec/guides/cross-layer-thinking-guide.md`:
  `0FA97553EF5421F4F65A2F516743AAF0DB157ABABC0B9B615C278BF73EF17B54`

## Planning artifacts and review boundary

`design.md` fixes architecture, compatibility, data contracts, exact write
set and source-to-publication lineage. `implement.md` fixes ordered work,
proposed command interface, validation and rollback checkpoints. The research
files resolve metric coverage, methodology/trace and provenance/publication.
Both JSONL manifests carry only applicable specs/research for later agents.

No blocking research question remains. Local timing variance, limited tail
resolution, source-derived SQLite version strength and Windows-only coverage
are explicit limitations. Actual measurements and the final optimization
recommendation remain execution deliverables, not facts established in planning.
The final summary was reviewed and approved on 2026-10-14. The implementation
checklist now tracks execution; unmet acceptance items remain unchecked.
