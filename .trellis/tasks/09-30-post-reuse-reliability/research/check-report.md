# Coordinator check report (2026-09-30)

Baseline `bbb7c79533c13db584fa5ca2e1e05990d990c8da`; branch `fix/post-reuse-reliability` in
`C:/AgentSessions-worktrees/post-reuse-reliability`. All Cargo invocations used `--offline`,
dependency-resolving ones `--locked`. Logs live under gitignored `target/gate-*.log`.

## Executable evidence

| Gate | Result | Evidence |
|---|---|---|
| `cargo --offline fmt --all --check` | exit 0 | terminal |
| `cargo --offline --locked clippy --workspace --all-targets -- -D warnings` | exit 0 | terminal |
| `cargo --offline --locked test --workspace --no-fail-fast` | 84 test blocks, 0 failed | `target/gate-debug.log` |
| `cargo --offline --locked test --workspace --release --no-fail-fast` | 84 test blocks, 0 failed | `target/gate-release.log` |
| `cargo --offline --locked test -p agent-session-grep-application --features semantic-candle` | 300 passed | `target/gate-semantic.log` |
| `cargo --offline --locked check -p agent-session-grep-cli --all-targets --features semantic-candle` | finished clean | `target/gate-semantic-cli.log` |
| `cargo +1.90.0 --offline --locked check --workspace --all-targets --all-features` | finished clean | `target/gate-rust190.log` |
| `node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs` | 11 passed | terminal |
| `python -B -m unittest discover -s scripts` | 18 run (1 skipped) | terminal |
| `python -B -m unittest discover -s scripts/release` | 9 passed | terminal |
| `python -B -m unittest discover -s scripts/evidence` | 54 run (1 skipped) | terminal |
| `git diff --check` | clean | terminal |
| `cargo --offline --locked test -p agent-session-grep-cli --test e2e --test hermes_state_db` | 150 + 4 passed | terminal |

## Regression coverage added or extended by this patch

- A: `parse_accounts_for_all_orphans_beyond_the_diagnostic_census`,
  `parse_accounts_for_messages_excluded_by_invalid_session_ids`,
  `parse_rejects_duplicate_sessions_even_when_orphans_hide_overcounting`,
  `parse_rejects_coercing_session_matches_even_when_orphans_hide_overcounting`,
  `parse_preserves_binary_session_keys_without_reading_text_orphans`, plus
  exact-limit/cap-overflow cases. CLI integration:
  `partial_orphan_snapshot_counts_skips_and_retains_prior_claims` now also proves a
  complete rescan retires the removed claim.
- B: `first_empty_standalone_source_is_a_warned_noop_until_content_exists`,
  `ingest_empty_replacement_keeps_proven_provider_and_tombstones_messages`,
  `empty_placeholder_binding_is_repaired_only_with_proof` (repair + forged-claim refusal).
  New `whole_source_updates_become_visible_after_rewrite` covers cline and hermes whole
  JSON, aider Markdown, cursor `ItemTable` and opencode SQLite: first sync visible,
  rewrite committed, new token visible and the replaced token no longer hits.
- C: six-permutation and randomized-permutation ordering tests in
  `agentsessions-domain`; `TimestampKey` is the single comparator for both selectors.

## Negative controls actually executed

- The manifest gate was temporarily forced back to the pre-fix shape
  (`provider_is_record_stream` bypassed); `whole_source_updates_become_visible_after_rewrite`
  then failed with `retained=1, committed=0` for cline, reproducing the original defect.
  The source file was restored and its SHA-256 re-verified (`3C3E936A...`), and the gate was
  re-run green afterwards.
- The empty-placeholder proof is fail-closed: a forged `msg_v1_*` claim blocks the rebind and
  the persisted `empty` binding survives the refusal.

## Privacy and diff review

- Diff touches only the seven product files plus three `.trellis/spec/.../index.md` entries and
  the task directories for this phase.
- No personal paths, credentials, runtime pointers, caches, journals, billing or visibility
  changes are included; historical CI payloads stay in gitignored `target/historical-evidence/`.
- No schema, dependency, MSRV, release-gate, SLO or provider-maturity change.

## Known gaps (not claimed)

- The independent Trellis check sub-agent could not run: every AgentRouter spawn in this window
  returned HTTP 402 (quota). This report is a coordinator-performed check, not an independent
  review.
- New PR/main CI for this commit is not run: the repository stays PRIVATE and private-runner
  billing may still be constrained. The external delivery gate stays pending owner
  authorization.
- No performance or maturity claim: the 1M improvement remains ~37.5% (< 50% target), and
  Hermes SQLite / cursor diskKV remain experimental.
- `a544ca2` (macOS parallel temp-dir isolation) was not re-touched.