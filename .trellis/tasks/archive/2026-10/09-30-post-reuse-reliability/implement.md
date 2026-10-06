# Execution plan

## Approved sequence
- [x] Re-fetch main, verify PRIVATE, and create an isolated implementation branch/worktree.
- [x] Persist the reviewed owner decisions and curated context before task activation.
- [x] Retain historical run 36665818855 artifacts under ignored target output; record only portable metadata in task research.
- [x] Implement A and C; coordinator check completed for both (independent sub-agent unavailable).
- [x] Implement B after A, including Hermes partial/recovery integration (starts at the same transaction boundary the Hermes accounting requires).
- [x] Coordinator updated the three applicable shared specs (domain ordering, CLI empty-source/record-stream lifecycle, SQLite placeholder repair + parser version 3) where behavior changed.
- [x] Run integrated offline quality gates and the privacy/diff review (evidence in research/check-report.md).
- [x] Final independent Trellis source review completed on 2026-10-01. Two product defects were repaired, missing proof/matrix coverage added, and three diagnostic assertions propagated. The independent handoff and fresh coordinator acceptance are recorded separately in the two check reports.
- [x] Commit only this task's allowlisted code/spec/task changes after local gates pass (five local commits including subtask archival at the 2026-10-01 handover; nothing had been pushed).
- [x] Owner separately authorized continued task-scoped execution and remote publication on 2026-10-01; do not change visibility/billing or bypass CI.
- [x] Accept and commit independent-review fixes plus three spec updates as `107578d90e4711d931d677cfbdb56d9829fcdc94`; record the final stable-tree offline gates in portable task evidence.
- [x] Publish PR #17 at `a51a60e`; verify all 12 current-head checks succeed. No old or local run was substituted for PR CI.
- [x] Merge PR #17 normally with an exact head guard as `124fb57`; verify identical tested/merge trees and successful main-push `ci` 36874887357 (7/7) plus `core-beta-evidence` 36874887468 (4/4).
- [x] Archive the accepted parent on 2026-10-01, relocate its 18 owned context references across eight manifests, and validate all four task directories. Archive publication uses a separate normal closeout PR; the final publication/CI receipt is recorded there.

## Local validation
All Cargo invocations use --offline; dependency-resolving checks also use --locked.
- cargo --offline fmt --all --check
- cargo --offline --locked clippy --workspace --all-targets -- -D warnings
- cargo --offline --locked test --workspace --no-fail-fast
- cargo --offline --locked test --workspace --release --no-fail-fast
- cargo --offline --locked test -p agent-session-grep-application --features semantic-candle
- cargo --offline --locked check -p agent-session-grep-cli --all-targets --features semantic-candle
- cargo +1.90.0 --offline --locked check --workspace --all-targets --all-features
- node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs
- python -B -m unittest discover -s scripts -p "test_*.py"
- python -B -m unittest discover -s scripts/release -p "test_*.py"
- python -B -m unittest discover -s scripts/evidence -p "test_*.py"
- git diff --check

## Gate accounting
Do not equate local implementation, task completion, PR checks, main checks, provider promotion, or publication. If private CI is billing-blocked, retain an explicit external pending gate. Never reopen the old completed phase to record this work.
