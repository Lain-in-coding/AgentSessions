# Research: Final build-feedback validation execution contract

- Query: Define a complete, auditable and executable final plan that can continue from the current workspace through tooling freeze, native lifecycle fixture, a valid initial capacity leaf, baseline/candidate full-capacity closures, measured reservation, a five-pair Pilot and a final evidence-based decision without inventing performance or adoption claims.
- Scope: mixed (current task contracts, current Python evidence tooling, runtime receipts, Cargo/Windows behavior already verified in this task; no new code execution beyond focused coordinator unit tests).
- Date: 2026-10-21 Asia/Shanghai.

## Findings

### 1. Executive position

The final plan is now technically reachable, but the current files are not yet a frozen runnable baseline.

Two changes since the last ten-file verification materially affect execution:

1. A coordinator module now supplies the previously missing full-arm finalizer, reservation-evidence producer, synthetic fixture and command-line entrypoints:
   - `scripts/evidence/build_feedback_coordinator.py`
   - `scripts/evidence/test_build_feedback_coordinator.py`
2. An initial capacity leaf has actually completed and produced useful capacity evidence:
   - `.trellis/.runtime/build-feedback-infrastructure-20261018/capacity-domain-check-v2-001/campaign.json`
   - `.trellis/.runtime/build-feedback-infrastructure-20261018/capacity-domain-check-v2-001/capacity/capacity.json`

The leaf result is successful capacity evidence:

- status `ok`;
- one Cargo `check` job completed with exit code `0`;
- Cargo process wall time `16.022802 s`;
- final target allocation `63,385,960 bytes`;
- total owned-output peak `806,961,893 bytes`;
- observed minimum C free approximately `116.533 GiB`;
- all capture, source, allocation and artifact checks true.

The whole capture lasted about `20 min 31 s`, rather than roughly 16 seconds. Job Object accounting shows one process still active from immediately after Cargo started until `16:26:19Z`, when it finally drained. Cargo had already exited. This is a process-tree tail, not Cargo build time. The final report must keep `duration_ms` and `capture_duration_ms` separate and must not describe the 20-minute cycle as compiler time without identifying the surviving process.

The current leaf cannot be sealed with the current tools. Its approval binds these earlier hashes:

- runner `e3986281d942e19bec2e8cf0f8bb8ef54945eaa4f80a24459097efecd70d6725`;
- statistics `52829685f6baf70d0670d5a6bab67aabc3dd5c63776e7e215813784b2e01baeb`.

The current files are:

- runner `ac930592608b6bae66801938933b12919990d755ffd5237ee678d81cfd82d5b8`;
- statistics `9840d7eb211cf066005db79bc62014a4d75542dd45e31e8d934a21766b8515e4`.

`retention.validate_policy()` therefore refuses the old approval with `approved verification tool changed`. Editing the approval in place would break the leaf protocol's approval hash. Restoring old files solely to seal this leaf would mix tool identities and is unsafe. The controlled resolution is to freeze the final tools, create a new leaf protocol and approval, and run a new leaf in a new run root. The old successful leaf remains preserved as historical capacity evidence but cannot be used as the sealed prerequisite for full-arm capacity.

### 2. Current read-only state

Snapshot at approximately `2026-10-21 00:54 +08:00`:

| Fact | Observation |
|---|---:|
| C free | `124,867,846,144 bytes = 116.292244 GiB` |
| Build processes | none found for cargo/rustc/rust-analyzer/sccache/mbx |
| Latest complete inventory | `194,523,596,544 bytes = 181.163539 GiB` |
| Inventory roots/caches | 116 roots, 69 caches |
| Inventory errors | 0 |
| Leaf preview | `allowed=true`, `review_required=true`, `reasons=[]` |
| Existing leaf review | approved, bound to old canonical protocol hash `14747f7b9845e53773af08ae893fba980b11b42ae2261c0dbdee0b923a41e2d3` |
| Current leaf run | complete and successful, but unsealed |

This moves admission well beyond the prior 38/56 GiB blocker. The final plan should not delete the six recent worker target directories to manufacture space. They remain inside the 72-hour protected window and are not needed to pass the current v2 floor:

- fixed B = `156,436,568,120 bytes`;
- B+40 GiB soft review threshold = `199,386,241,080 bytes`;
- B+60 GiB hard stop = `220,861,077,560 bytes`;
- current fixed-B growth = `38,087,028,424 bytes = 35.4715 GiB`;
- an 8 GiB leaf reservation remains below the hard stop and requires owner review.

### 3. Authority and the real end state

The approved task contract has two distinct decision levels.

**Current-stage validation** is a complete, valid five-pair Pilot with:

- baseline: native, full debug, incremental on, isolated layout;
- candidate: native, line-tables-only debug, incremental on, three-lane layout;
- six primary jobs;
- feedback-warm and deterministic edit phases;
- guarded process-tree, source, allocation and disk evidence;
- qualifications: Clippy, diagnostics, optional features and official release;
- sealing and exact recycle/purge of each successful arm intermediate;
- preserved raw attempts and failure evidence.

The five-pair Pilot produces a measured recommendation. It does not by itself authorize changing shared defaults. The PRD explicitly retains this requirement:

- a fixed formal N is proposed from Pilot variance/compute/storage evidence;
- N, seed, order, stopping rule and identity are preregistered before formal data;
- only a complete formal campaign plus all speed, P95, release, correctness and optional-feature gates may adopt a candidate.

The user's latest instruction asks to execute the plan until validation finishes. The defensible end state is therefore:

1. complete infrastructure verification;
2. complete native fixture;
3. complete fresh leaf;
4. complete baseline and candidate full-capacity closures;
5. complete five valid Pilot pairs;
6. produce a final `recommended`, `rejected` or `inconclusive` result with a proposed formal N where relevant.

Do not change a repository-wide Cargo profile or make MBX/sccache default merely because the five-pair Pilot passes. Doing that without the separately preregistered formal campaign would violate the current contract.

### 4. Code contracts the plan must preserve

| Contract | Current implementation |
|---|---|
| Initial leaf cannot supply a paired finalizer | `runner._execution_retention_policy()` requires `arm_finalizer is None` at `scripts/evidence/build_feedback_runner.py:1900-1905`. |
| Full-capacity and paired arms require a callback | Same function requires a callable before output allocation at `runner.py:1898-1903`. |
| Runner CLI cannot run v2 Pilot | `build_feedback_benchmark.py:392-404` refuses v2 paired runs and points to the coordinator. |
| Coordinator CLI is for full-capacity/Pilot | `build_feedback_coordinator.py:450-460` calls `runner.run_protocol(..., arm_finalizer=...)`. |
| Coordinator CLI is not the initial-leaf command | Because it always supplies the callback, it cannot run the callback-forbidden initial leaf. Use the benchmark CLI for the leaf. |
| Full arm runs all phases before sealing | `runner.py:1978-2014` runs primary, feedback-warm and edit. |
| Qualification runs while artifacts are live | `build_feedback_coordinator.py:132-160` resumes in fixed order and refuses a failed qualification before disposal. |
| Seal/disposition must be complete before callback success | `build_feedback_coordinator.py:163-218` performs normalize, qualifications, seal, exact disposal and independent link verification. |
| Capacity observation must come from a sealed full arm | `runner._reservation_observation_peak()` at `runner.py:333-430` verifies the seal and all raw captures; status flags alone are insufficient. |
| Reservation covers both configurations | `build_feedback_coordinator.py:281-317` requires exactly two verified observations matching the two config hashes. |
| Retry needs a fresh predeclared allocation | `runner.py:2771-2803` derives `attempt = len(prior)` and fails if that allocation is absent. Five pairs with three possible attempts therefore require 15 allocation records and 30 source worktrees. |
| A failed non-contaminated pair cannot be silently replaced | `runner.py:2775-2780` and `2846-2849` retain failure state. |
| Initial leaf cap is exactly 8 GiB | `runner.py:547-552`; full-cap cap is at least 8 GiB and at least the prior leaf peak reserve. |

### 5. Required coordinator validation before physical runs

The current coordinator has only three unit tests:

- synthetic authority is separate and exact;
- reservation evidence uses two verified distinct configurations;
- duplicate configuration/insufficient growth are refused.

The three tests currently pass in `0.123 s`. This is useful interface evidence but insufficient for lifecycle use. Before freezing the tool hash, add and pass tests covering:

1. `finalize_arm()`:
   - calls qualifications in exact order;
   - does not seal or dispose when a qualification fails;
   - resumes an existing immutable link without repeating disposal;
   - rejects a changed arm receipt;
   - rejects wrong phase/source/target/output lease;
   - verifies capacity null ordinals and paired exact ordinals;
   - rejects a callback-produced link that does not bind the original arm.
2. `run_campaign()`:
   - supplies the callback for full-capacity and Pilot;
   - preserves the runner's initial-leaf callback prohibition;
   - propagates `ProtocolError` before output creation.
3. `make_reservation_observation()`:
   - rejects incomplete/failed/cap-stopped campaigns;
   - validates the retained seal against the original protocol;
   - rejects a changed campaign or protocol hash.
4. `make_reservation_evidence()`:
   - rejects one config, duplicate config, wrong peak, wrong workload scope and nonpositive margin;
   - recomputes the peak from observation evidence;
   - writes immutable output.
5. CLI behavior:
   - output paths are immutable;
   - exceptions produce a nonzero exit and no false success JSON;
   - `make-observation` cannot be reused for another campaign identity.
6. `prepare_fixture_protocol()`/`run_fixture()`:
   - approval uses a literal one-file target;
   - copied approval cannot redirect the target;
   - extra files, extra directories, alternate layout and reparse paths are refused;
   - failure after recycle does not repeat an uncertain delete.

After these code changes stop, run at least:

```text
ruff check --target-version py310 scripts/evidence/build_feedback_coordinator.py scripts/evidence/test_build_feedback_coordinator.py
mypy --no-incremental --follow-imports skip --python-version 3.10 scripts/evidence/build_feedback_coordinator.py scripts/evidence/test_build_feedback_coordinator.py
python -B -m unittest scripts.evidence.test_build_feedback_coordinator -v
```

Then rerun the full Python suite used for the last ten-file checkpoint. The coordinator also changes the effective runner/statistics integration; its tests must be included in the aggregate count.

### 6. Native fixture phase

The fixture is synthetic lifecycle evidence only. It must never be described as Cargo capacity or performance evidence.

Sequence:

1. Choose a unique control root under ignored runtime, for example a new `recycle-fixture-v2-002-control`, whose sibling run root is absent.
2. Prepare the fixture protocol from the frozen source protocol with `prepare-fixture`.
3. Snapshot Recycle Bin item count, identities, logical bytes and allocated bytes without changing them.
4. Require quiet: no cargo/rustc/rust-analyzer/cache daemon or other build process.
5. Run `run-fixture` with exactly one `fixture.bin` of `b"ASG recycle fixture\n" * 256`.
6. Verify:
   - only the exact new target entered the Recycle Bin;
   - the exact matched item was purged;
   - pre-existing Recycle Bin entries are unchanged;
   - actual C free did not decrease after completion;
   - the disposition has no terminal ambiguity;
   - the result explicitly says `performance_evidence=false`.
7. If any stage is ambiguous, retain the item and stop that destructive branch. Never clear the Recycle Bin wholesale.

The fixture is a prerequisite for using the coordinator's real arm finalizer, not merely a smoke test of the approval parser.

### 7. Fresh initial leaf phase

The existing successful leaf remains historical because its tool approval became stale.

1. Freeze all current evidence tools first.
2. Create a new unique control/run identity, for example `capacity-domain-check-v2-002` plus `capacity-domain-check-v2-002-control`.
3. Generate a new retention approval after the freeze. Its `verification_tools` must hash the final runner, statistics, benchmark and inventory files; `checker` must hash retention. The coordinator itself is orchestration code and is not currently in the approval's four-tool list, but its output is independently revalidated by retention.
4. Include all protected roots, both Git common directories, source roots, existing build-cache roots, stores and the new control root.
5. Reuse a clean frozen source worktree with the approved commit/tree/lock/manifest. Do not touch source or Git.
6. Run a fresh complete inventory with:
   - all original baseline roots;
   - `C:\AgentSessions-worktree-rescue-2026-08-15`;
   - explicit MBX and sccache stores;
   - the new control root;
   - the new run root while absent.
7. Require `allowed=true`, `reasons=[]`, `review_required=true` and no budget overflow.
8. Generate an owner review receipt using the canonical preview protocol hash, baseline receipt hash, finite allocated ceiling and C-free floor.
9. Run the initial leaf through the benchmark CLI, not coordinator:

```powershell
python -B scripts/evidence/build_feedback_benchmark.py run `
  --protocol <leaf-protocol> `
  --review-receipt <owner-review>
```

10. Require `status=ok`, one Python/Cargo job, successful Cargo SQLite/domain check, `capture_complete=true`, full source/drain/allocation proof and an observed peak below 8 GiB.
11. Seal and dispose the exact target through coordinator `seal-leaf`.
12. Preserve both `duration_ms` and `capture_duration_ms`. Investigate the process-tree tail if it recurs; do not count it as Cargo time.

Expected leaf evidence should be much smaller than 8 GiB. The purpose is to prove capture and create the sealed prerequisite; it is not the final capacity reservation.

### 8. Two full-capacity closure phases

Full capacity is one complete arm at a time, never a simulated pair.

For baseline and candidate separately:

1. Use a new run root, new control/approval, new selected source worktree and the prior sealed leaf observation as prerequisite.
2. Set `capacity_probe.kind="full-arm-and-qualification"`, `stage="expanded"`, exact arm name and exact source.
3. Set `phase="capacity"`, `pairs=0`, `allocations=[]`.
4. Choose a controlled cap. A reasonable first value is 48 GiB when current free space supports it; it is approximately 25% above the old whole-pair 39 GiB forecast and still leaves substantial C headroom. It remains a cap, not a forecast.
5. Require cap >= 8 GiB and >= `cache_reservation_bytes(prior_leaf_peak)`.
6. Create the same full config map for both protocols so each one is identity-compatible with the eventual Pilot.
7. Run inventory, preview and owner review before each arm.
8. Run through coordinator `run` with `--pair-limit 1`.
9. The arm must execute:
   - six primary jobs;
   - six feedback-warm jobs;
   - six edit jobs;
   - actual Clippy;
   - actual all-feature tests;
   - approved diagnostics;
   - the frozen official full release validator.
10. Qualification output remains protected on any failure.
11. On successful completion, the coordinator seals and purges only the exact target.
12. If the cap stops deliberately, retain the honest cap-stop observation and start a new run with a larger controlled cap based on the measured stop. Do not enlarge the same immutable protocol.
13. On quality, source, memory, timeout or unknown failure, stop. Do not use the object as reservation evidence.

The official release producer's owned scratch cleanup needs an explicit boundary. The frozen validator calls `cleanup_scratch()` and ultimately `shutil.rmtree()` for its own suite-owned scratch at `.trellis/.runtime/build-feedback-infrastructure-20261018/seed-source/scripts/evidence/core_beta_benchmark.py:1072-1078`. The final plan must declare this exact producer-owned scratch as an ephemeral task output whose native cleanup is part of the frozen validator, while all capacity targets continue through recycle/match/purge. Predeclare `release_suite_root`, bind `TEMP`/`TMP` to its `scratch` child, verify before/after that no unrelated path is touched, and never broaden this exception to user caches or arbitrary directories. Without this explicit decision, the release qualification conflicts with the task's recycle-only target policy.

### 9. Reservation evidence

After both full arms succeed:

1. Run coordinator `make-observation` independently for baseline and candidate.
2. Each observation must bind its original protocol, campaign, full seal, full workload and actual measured peak.
3. Compute:

```text
max_peak = max(baseline_observed_peak, candidate_observed_peak)
reserved_growth = cache_reservation_bytes(max_peak)
margin = reserved_growth - max_peak
```

`cache_reservation_bytes()` is the established `ceil(1.25 * peak / GiB + 1) GiB` rule at `runner.py:315-318`. This gives a positive margin and preserves the existing reservation formula.

4. Run `make-reservation` with both observations, the computed margin and the exact reserved growth.
5. Verify the producer refuses:
   - duplicate configuration;
   - one missing configuration;
   - cap-stop rather than completed-workload evidence;
   - a peak not re-derived from the seal;
   - identity drift.
6. Do not treat the declared full-arm cap as measured reservation.

### 10. Five-pair Pilot protocol and allocations

The Pilot protocol must be frozen before any pair starts.

It needs:

- phase `pilot`, exactly five pairs;
- the final source commit/tree/lock/resources manifest;
- exact baseline and candidate configs;
- seed, balanced order, two jobs and two test threads;
- the measured reservation evidence;
- a retention approval covering every possible target;
- predeclared timeout/poll and resource limits;
- no adaptive pair count.

Each pair can use at most three attempts because only contamination is retryable. Because the protocol cannot gain allocations after creation, preallocate:

```text
5 pairs × 3 attempts × 2 arms = 30 dedicated source worktrees
```

These are represented by 15 allocation records:

```text
pair-0000-attempt-00 ... pair-0000-attempt-02
pair-0001-attempt-00 ... pair-0001-attempt-02
...
pair-0004-attempt-00 ... pair-0004-attempt-02
```

Every source must be an independent Git worktree at the same frozen identity. No source may contain an undeclared ignored output. The source roots and every possible target must be listed in the independent retention approval and protected-root scope.

The target allowlist must contain all 30 possible target directories:

```text
<run_root>/pairs/pair-XXXX-attempt-YY/<arm>/targets
```

The preparer must also reserve the control/key/evidence store and include the absent run root as an explicit store in inventory.

### 11. Pilot execution

Use coordinator `run`, not the generic benchmark CLI.

Recommended execution pattern:

```powershell
python -B scripts/evidence/build_feedback_coordinator.py run `
  --protocol <pilot-protocol> `
  --review <owner-review> `
  --pair-limit 1
```

Repeat the same immutable command after each `segment_complete` checkpoint. This is resumption of one fixed five-pair campaign, not a new experiment and not adaptive sampling.

Before each pair, require:

- no external build process;
- source identities unchanged;
- current budget still below the hard threshold;
- free space after reservation still above the selected floor;
- current allocation below the reviewed ceiling;
- toolchain MSVC segment identity unchanged.

After each pair, verify:

- both arm records are immutable;
- both qualifications succeeded before disposal;
- both seal/disposition links verify;
- the pair is `ok`;
- completed count advances exactly once;
- a contaminated pair uses its next predeclared allocation;
- non-contamination failures remain visible and block silent replacement.

On completion require:

- `planned_pairs=5`;
- `completed_valid_pairs=5`;
- status `complete`;
- no dropped failed attempts;
- all retained links and source/toolchain identities valid.

### 12. Final comparison and decision

Run the statistics/benchmark comparison against the exact campaign and protocol. The final result must contain:

- primary, feedback-warm and edit timing distributions;
- six-job correctness outcomes;
- Clippy, diagnostics, optional-feature and release qualification outcomes;
- allocated growth and peak reservation;
- every attempt, including contamination and failures;
- total seal/recycle/purge overhead separately from timed build phases;
- process-tree-tail duration separately from Cargo wall time.

Native profile/layout recommendation criteria remain:

- at least 10% allocated savings;
- primary/edit/check/test/Clippy P50 and P95 no more than 5% slower;
- every correctness, diagnostics, optional-feature and release gate passes.

The five-pair Pilot can report `recommended-for-formal`, `rejected` or `inconclusive`.

Do not emit a final adopted configuration unless a separate formal N is preregistered and completed. If the Pilot does not pass, do not run formal. If it passes, report the proposed formal N and remain at the current-stage boundary until the formal campaign is authorized.

### 13. Failure and recovery matrix

| Failure | Required action |
|---|---|
| Tool hash changed after approval | Do not dispose. Freeze tools and create a new protocol/approval; the old successful but unsealed leaf remains historical evidence. |
| External build process before/during arm | Mark contaminated; finish evidence retention; retry only with the next preallocated exact pair. |
| Cargo/test/Clippy/diagnostic failure | Preserve target and raw logs; do not seal for reservation and do not replace the pair. |
| Memory limit | Preserve and report `memory_limited`; do not call it an OS out-of-memory or generic timeout. |
| Whole-tree timeout | Drain, preserve, and fail the arm. |
| Owned-output cap reached | Distinguish `capacity_limited`; seal only under cap-stop rules; no performance sample and no full reservation. |
| Source/Git drift | Refuse before further mutation. |
| Seal mismatch or reparse/path ambiguity | Preserve target; stop disposal. |
| Recycle item absent/ambiguous | Stop. Never repeat an uncertain purge. |
| Free-space release inconclusive | Publish inconclusive disposition; never invent released bytes. |
| Process exits but descendant remains | Keep capture honest; inspect the descendant identity and report capture-tail time. |
| Reservation producer finding | Preserve the independent finalizer gaps found in this audit and fix them before freezing tool hashes. |
| Coordinator bug after partial arm | Read campaign state first; resume only through documented idempotent checkpoints. Do not create a duplicate run root. |

### 14. Concrete completion gates

The final plan is complete only when all of the following are true:

1. Current coordinator and all integration tests pass, including the added lifecycle cases.
2. Ruff, Mypy, focused unit tests and the aggregate Python suite pass at one frozen hash set.
3. Native one-file Recycle Bin fixture verifies movement, exact purge, release and unrelated-item preservation.
4. Fresh leaf succeeds under current approval and produces a verified sealed observation.
5. Baseline full-capacity arm completes and supplies a verified full-capacity seal.
6. Candidate full-capacity arm does the same.
7. Reservation evidence covers exactly both config hashes and derives peak from evidence.
8. Pilot has 30 predeclared sources and 15 allocation records, with fixed five-pair order and no adaptive additions.
9. Pilot completes 5/5 valid pairs with all required evidence and exact disposal links.
10. Final result reports performance, disk, correctness, release, qualifications, failures, trust limits and an adoption/non-adoption recommendation without overstating the evidence.

### 15. Files found

| Path | Role |
|---|---|
| `.trellis/tasks/10-18-build-feedback-infrastructure/prd.md` | Authoritative goal, v2 budget, capacity, recycle, Pilot and adoption gates. |
| `.trellis/tasks/10-18-build-feedback-infrastructure/design.md` | Module boundaries, source identity, process/allocation evidence and retention design. |
| `.trellis/tasks/10-18-build-feedback-infrastructure/implement.md` | Historical checkpoints and current coordinated implementation ownership. |
| `.trellis/tasks/10-18-build-feedback-infrastructure/task.json` | Stale task checkpoint; it still records 0/5 and pre-leaf refusal, so it must be updated only by the main session after verification. |
| `.trellis/spec/agentsessions-cli/backend/index.md:1006-1250` | Executable build-feedback scenario and its validation/error rules. |
| `scripts/evidence/build_feedback_runner.py` | Protocol, admission, Job Object capture, full capacity and Pilot orchestration. |
| `scripts/evidence/build_feedback_retention.py` | Lease, seal, recycle, exact-match purge and disposition validation. |
| `scripts/evidence/build_feedback_coordinator.py` | Missing production coordinator added for full arms and reservation evidence. |
| `scripts/evidence/build_feedback_benchmark.py` | Inventory/preview/initial-leaf CLI and comparison entrypoint. |
| `.trellis/.runtime/build-feedback-infrastructure-20261018/capacity-domain-check-v2-001/campaign.json` | Actual successful leaf campaign, currently unsealed. |
| `.trellis/.runtime/build-feedback-infrastructure-20261018/capacity-domain-check-v2-001/capacity/capacity.json` | Actual leaf capacity, artifact and process proof. |
| `.trellis/.runtime/build-feedback-infrastructure-20261018/capacity-domain-check-v2-001-control/retention-approval.json` | Old leaf approval with now-stale verification-tool hashes. |

### 16. Related specs

- `.trellis/workflow.md` — research persistence and active-task authority.
- `.trellis/spec/agentsessions-cli/backend/index.md` — Windows build-feedback evidence contract.
- `.trellis/spec/guides/cross-layer-thinking-guide.md` — distinguish source, capture, derived evidence and decision boundaries.
- `.trellis/spec/guides/code-reuse-thinking-guide.md` — reuse the existing runner, retention and coordinator APIs instead of adding parallel mechanisms.

## Caveats / Not Found

- The coordinator is actively changing during this research. The details above describe the `2026-10-21 00:54` snapshot and must be revalidated after all writers stop.
- The current coordinator has only three unit tests and no end-to-end callback integration test in the current test file. A green three-test run is not enough to freeze the tool hash.
- The previous final 277-test checkpoint predates the current coordinator and the current runner/statistics hashes. It must not be reused as the final validation count.
- No real native fixture disposal, full capacity arm, Pilot or formal campaign was run by this research role. The only real Cargo observation found is the existing successful leaf; it is capacity evidence and cannot support performance adoption.
- No source, existing build cache, Recycle Bin item, product file, task metadata or Git state was changed by this research role.
- The process-tree tail observed in the leaf is not yet attributed to a named executable. Future captures should retain the active descendant identity if the same tail recurs.
