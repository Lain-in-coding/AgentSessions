# Final local verification record

Date: 2026-10-07. Platform: Windows. User data was not used or modified.
Implementation and local acceptance are complete. The user explicitly approved the two work commits and push. Linux/macOS execution and remote CI have not been run.

## Coordinator-executed final gates
- `cargo fmt --all -- --check`: PASS.
- `cargo check --workspace --all-targets`: PASS.
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS.
- `cargo test --workspace`: PASS, **1,978 passed**, 23 ignored, 95 test-suite summaries. The enlarged maintenance measurement is one intentional ignore and was explicitly run below; do not describe ignored tests as executed.
- `cargo test -p agent-session-grep-application --features semantic-candle`: PASS, 318 passed, no ignored tests across 3 suite summaries. These overlap default Application tests; do not add them to the workspace count as unique cases.
- `cargo check -p agent-session-grep-cli --all-targets --features semantic-candle`: PASS.
- `node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs`: 11 passed.
- `python -m unittest discover -s scripts -p "test_*.py" -v`: 19 tests, OK, one existing skip.
- `python -m unittest discover -s scripts/release -p "test_*.py" -v`: 18 passed.
- `python -m unittest discover -s scripts/evidence -p "test_*.py" -v`: 58 passed.
- `git diff --check`: PASS.
- All four task context manifests validate.

Detailed machine-local logs remain ignored under target/journal-maintenance-evidence; this checked-in record is the human-readable evidence summary, not a claim of remote CI.

## Real CLI physical evidence
Coordinator reran BOTH sizes on the final source snapshot:

```text
cargo test -p agent-session-grep-cli --test journal_maintenance physical_reclamation -- --include-ignored --nocapture --test-threads=1
```

Both passed. Each fixture has 21 revisions and uses the default 30-second budget, not a raised override. The CLI process detaches and completion preserves generation/search results; no owned backup remains.

| Messages | DB bytes before -> after | Whole root bytes before -> after | Whole root reclaimed bytes | Elapsed ms | Max writer lease ms | Process peak memory bytes | Sampled peak root bytes |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 200 | 3,346,432 -> 794,624 | 3,346,508 -> 839,756 | 2,506,752 | 1,030 | 508 | 16,257,024 | 10,117,564 |
| 2000 | 29,937,664 -> 4,050,944 | 29,937,740 -> 4,096,076 | 25,841,664 | 5,185 | 3,182 | 22,626,304 | 89,891,260 |

### Measurement scope
- Whole-root measurements bracket submission/completion and include queue, WAL/SHM, owned backups, temporary files and lock files visible to the test. They demonstrate net physical savings, not only logical compaction.
- Job `metrics.before/after` are phase measurement points and can include an already-created queue or an open SQLite SHM. They intentionally differ from the external whole-root submission-to-completion comparison above; do not conflate their deltas.
- Disk peak is sampled every 40 ms and is an observed lower bound, not an exact instantaneous allocation peak. Open/deleted SQLite temporary files or shorter spikes may not be visible to the sampler.
- Peak memory is the worker process lifetime high-water mark on supported platforms, not per-job incremental allocation. These fixture workers process one measured job each. Unsupported platforms expose unavailable as zero; no cross-platform memory result is asserted here.
- This is a reproducible debug-build synthetic benchmark, not a real-user-data or release-build performance promise. Search acceleration is not claimed.

## Safety and compatibility evidence
- Ports 72, Application 299 default tests (15 maintenance), adapter 334 tests (19 new storage, 14 queue, legacy B2 13 and relocation 42), CLI 359 unit tests and 13 normal maintenance process tests passed in owner/final workspace runs. Counts overlap workspace totals.
- Real process coverage: writer busy then autonomous progress with no additional request; detached survival and crash wake; single executor; 30-second idle-exit/enqueue race; persistent pause/read-only no-wake; long-reader checkpoint deferral; spawn failure; writer broken pipe; fixed target and fixed selection.
- Independent bounded safety review is in maintenance-safety-review.md. Original findings and follow-up regressions were fixed and independently verified; no unresolved high-risk issue was found in that scope.
- The explicit cleanup cancellation cutoff, nullable unreconciled logical commit, early destructive-phase invariant verification, no-catalog cleanup recovery and Windows parent-pipe inheritance fix are documented in code/specs.

## Remaining delivery work
- Execute the user-approved work commits, archive only this task tree, then push the feature branch and verify local/remote equality.
- No PR or Linux/macOS CI run is represented by this local evidence. Remote push is delivery of the feature branch, not proof of cross-platform CI.
- This feature does not complete the earlier 15-project full audit and introduces no semantic-model change.

## Final pre-commit review
A reused independent checker inspected the final module/test/dependency inventory, Unix portability paths, privacy and documentation alignment; no new blocker was found. Its review made no product edits. Newly added source/test files are included explicitly rather than relying on tracked-only diff statistics.

## Full-tree privacy baseline (pre-commit audit)
The extra actual-repository run of `python scripts/evidence/privacy_scan.py --repo .` FAILED with 1,177 path-pattern findings in 228 historical files, all below `.trellis/tasks`. This is separate from the passing scanner unit tests above. A Git-baseline comparison against b1216c976e3cd25661b4dd2f871b7832ef0fb818 confirmed every finding is in an unchanged baseline file. Both current changed/staged files and all new untracked task/product files had zero findings; the prior versions of changed files also had zero. No allowlist was widened, no scanner error was suppressed, and historical reports were not mass-rewritten in this maintenance task. The full-tree privacy gate therefore remains an explicit pre-existing limitation, not a passing gate.
