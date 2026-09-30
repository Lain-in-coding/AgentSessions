# Execution plan

## Approved sequence
- [x] Re-fetch main, verify PRIVATE, and create an isolated implementation branch/worktree.
- [x] Persist the reviewed owner decisions and curated context before task activation.
- [x] Retain historical run 36665818855 artifacts under ignored target output; record only portable metadata in task research.
- [x] Implement A and C; coordinator check completed for both (independent sub-agent unavailable).
- [x] Implement B after A, including Hermes partial/recovery integration (starts at the same transaction boundary the Hermes accounting requires).
- [x] Coordinator updated the three applicable shared specs (domain ordering, CLI empty-source/record-stream lifecycle, SQLite placeholder repair + parser version 3) where behavior changed.
- [x] Run integrated offline quality gates and the privacy/diff review (evidence in research/check-report.md).
- [ ] Final independent Trellis check: NOT run — every sub-agent spawn failed with HTTP 402 (AgentRouter quota); the coordinator check above carries that explicit gap.
- [ ] Commit only this task's allowlisted code/spec/task changes after local gates pass.
- [ ] Request any needed remote publication/merge authorization; do not change visibility/billing or bypass CI.

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
