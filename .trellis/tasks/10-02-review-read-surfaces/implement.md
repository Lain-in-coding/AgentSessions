# Execution: owner-approved experience phase (2026-10-05)

Baseline: f0831af. The previous provider phase is not reopened. P2-1 GET-ack and the historical 1.0/active 1.1 schema split already exist. Pre-implementation local probes: Node Web suite 11 passed; published_envelope_schema_contains_runtime_contract 1 passed. These do not cover the new gaps.

## Ordered batches
- [x] Resolve current worktree and owner approval; replace stale planning templates with the approved scope.
- [x] 1. Reconcile finding ledger; reproduce and repair bounded P0-3 identity/diagnostic gaps.
- [x] 2. HTTP parameter/budget fidelity and Web accepted-search Handoff inheritance.
- [x] 3. Web raw/talks/sessions rendering, truncation and truthful empty/error/partial states.
- [x] 4. TUI literal input, modified facets and shared clock/current_repo injection.
- [x] 5. Real-frame schema validation, negative controls and browser/terminal acceptance.
- [x] Independent scoped and integration review; resolve findings with regressions.
- [x] Final exact-tree local gates, then commit/push and exact-SHA remote gates (verified implementation checkpoint a867a01).
- [x] Update parent/child evidence, relevant specs and the existing owner-facing combined plan. Do not archive the parent/children or mark deferred items closed.

## Gate commands
Use the shared Cargo build cache with CARGO_BUILD_JOBS=2 and RUST_TEST_THREADS=2; coordinate builds. Cargo commands use --offline --locked locally.
- cargo test -p agent-session-grep-cli --all-targets --all-features
- node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs
- Targeted real CLI/MCP/HTTP red-green counterexamples and 1.1 schema validation (test dependency pinned).
- cargo fmt --all --check
- cargo clippy --workspace --all-targets --all-features -- -D warnings
- cargo test --workspace --all-features
- cargo +1.90.0 check --workspace --all-targets --all-features
- Existing scripts, release and evidence Python unittest suites, privacy scanner, task.py validate, git diff --check.
- Real synthetic-data browser and terminal journeys; isolated user home/config/cache, no real transcripts or models.
- Remote required workflows on the final committed SHA; earlier green runs do not certify later edits.

## Evidence / limits
This file will record exact command results and remaining gaps per batch. The planning-only boundary follow-up agent failed with quota exhaustion; it did not provide an independent implementation review. No product change or new full-workspace verification is claimed at activation.

## Robot schema checkpoint (local, 2026-10-05)
- Confirmed the old version-only diagnosis is stale: active 1.1 already exists and historical 1.0 stays frozen.
- Actual synthetic search output failed the 1.1 validator because data.retrieval_mode was omitted from the closed searchData schema. The positive four-mode unit case failed before the minimal enum addition. No runtime field or version changed.
- After schema correction: 13 Python unit tests passed; explicit-binary verifier passed 10 cases / 11 real frames (9 response, 1 error, 1 progress), covering success/partial/failure. Diagnostic remains fixture-only.
- In-memory mutation disabled Draft202012Validator.iter_errors: all five schema-negative subcases failed, proving validation is load-bearing; no source mutation remained.
- Wired pinned test dependency and explicit current-build binary into CI, and documented contributor setup. Final executable rebuild/exact-SHA evidence and independent reviewer verdict are pending; shared-cache binary execution alone does not certify source identity.

## TUI worker checkpoint (local, 2026-10-05)
- Changed only tui/core.rs and tui/mod.rs. Search literal m/k, explicit Alt facet actions, visible help and shared resume_app clock/current_repo are implemented. Results m/k, Unicode and existing exit/navigation keys are retained.
- Actual baseline RED after a corrected test import: 44 passed / 4 failed. Focused TUI GREEN: 53 passed / 0 failed, including all-features. The clock/repo test isolates hooks in a child process and includes negative reference controls for both signals.
- File-level rustfmt/diff checks and production-library all-features Clippy passed. Test Clippy found an in-flight HTTP err_expect lint and was reported to its owner, not suppressed.
- Main reviewed the production delta; independent TUI review, native terminal acceptance and final workspace/exact-SHA integration remain pending.

## HTTP worker checkpoint (local, 2026-10-05)
- serve.rs now forwards Context max_bytes and Handoff max_tokens/max_bytes plus every repeated provider. Shared strict query validation preserves argument-injection guards and rejects unknown/empty/duplicate-scalar/malformed percent/UTF-8 input.
- Actual baseline RED: 34 passed / 12 failed. Focused all-feature HTTP GREEN: 46 passed / 0 failed. Added synthetic route_request versus independently constructed CLI dispatch checks, default/zero/boundary cases and content-budget assertions.
- all-feature --tests Clippy passed after replacing err().expect() with expect_err; no suppression. Existing GET-ack, POST no-exec, auth/CSRF tests stayed green.
- Main reviewed the production delta. The redacted_json call site intentionally remains for main's shared identity-helper integration after the boundary worker completes. Final browser, independent integration and full-workspace/exact-SHA gates remain pending.

## Integration and native acceptance checkpoint (2026-10-05)
- Main completed trusted-command HTTP redaction integration. New real HTTP identity test first failed with a redacted canonical ID, then passed search/cursor/show/context/handoff after the shared helper was used. Untyped JSON cannot self-select a profile. All 48 HTTP tests passed. Two stale Web source-string tests now follow the actual invalidation/rerender helpers; behavioral guards were not removed.
- Boundary worker: CLI4/MCP3 actual RED -> GREEN, redaction27, full e2e173/MCP48; no protocol/schema version changes. Whole P0-3 remains open for pre-existing non-identity MCP echoes and debug stderr policy.
- Independent TUI review passed 53 default/all-feature tests, check and Clippy. Independent HTTP/Web/privacy review found no new production defect, passed HTTP48/redaction27/Node41 and lint/check; its first real CLI rerun collided with main's concurrently executing shared-cache binary and did not run. A serialized retry is being requested, not counted as passed yet.
- First full main gate: 86 targets, 2084 passed / 0 failed / 24 ignored; fmt, all-target/all-feature Clippy and Rust 1.90 checks passed. This predates the additional Enter fix, so it is not final-tree certification.
- Schema independent review found physical Unicode separator handling, command binding and production-registry coverage gaps. Main reproduced the first two (1 failed +3 errored cases), fixed them, added a real Unicode fixture, and passed 17 tests plus the 10-case live gate. Dropping production Registry wiring causes both HTTPS/file controls to fail without real retrieval. Follow-up review pending.
- Real Windows PTY: literal mk, Alt+M/K facets, query clear, gateprobe search (3 mainline hits), Context open and Ctrl+C exit 0 with terminal/cursor restoration verified. IME input was not claimed.
- Computer Use had no browser surface; used the explicitly identified dedicated Playwright CLI with a new nonpersistent Chrome test session and a copied candidate binary, not a user browser or shared executable. Real raw/talks/sessions views, partial count/reason, explicit zero -> HTTP error, native badInput with empty value, Handoff evidence budget, bilingual controls and 390px no-horizontal-overflow layout were observed. Desktop and narrow screenshots were inspected locally; none contain real transcripts or are committed.
- Real Chrome exposed native Enter -> same-value change cancellation. Web worker reproduced 5 new failures, then passed 47 Node tests with complete-signature + native-validity invalidation. No duplicate global Enter listener was found. Rebuild/native Enter follow-up and final gates are pending.

## Final local verification (2026-10-05; remote publication next)
- All scoped independent checks are resolved. TUI: 53 tests plus clippy/check. HTTP/privacy/Web: HTTP48, redaction27, Node47; serialized CLI4/MCP3 retry passed after the earlier shared-cache contention. Schema reviewer independently verified all three corrections and killed Unicode/command/Registry mutations; 17 tests and the 10-case live gate passed.
- Rebuilt candidate after the Enter fix. Fresh Chrome Enter produced exactly one successful search request and four hits. UI provider/since/until reached Handoff unchanged and returned four matching evidence entries; entity viewing and nonexecuting resume preview worked. Language switching preserved loaded content and filters, close removed selection/preview, and a Unicode/emoji no-match query showed an honest empty state with zero console errors.
- Final Windows workspace: 86 targets / **2084 passed, 0 failed, 24 ignored**. fmt, all-target/all-feature Clippy, Rust 1.90 all-target/all-feature check passed. Node **47/47**; Python scripts **38 tests (1 platform skip)**, release **11**, evidence **58** (binary-backed cases enabled). Real Robot schema **10 cases / 11 frames**; synthetic smoke **10/10**. The test-only candidate is a copied build, so live acceptance does not lock Cargo's shared executable.
- Native Windows PTY and real Chrome verification were completed, with inspected desktop/narrow screenshots. Only the owned test service and dedicated nonpersistent browser were closed. No real source catalog was read or migrated. IME and non-Windows interactive UI were not claimed; platform CI is a separate gate.
- Remaining: publish reviewed commits and bind remote checks to their immutable SHA. No official release, merge, archive or broader P0-3 closure. C1/P2-9 and release/performance work remain outside this verified experience phase.

## Immutable publication checkpoint (2026-10-05)
- Implementation/publication checkpoint: `a867a016d8ce29607e203ccbd2b0c0644e8811b5`. Commits: `77f6db9` TUI, `ee2516d` read surfaces/identity, `0b2e45a` real schema validation, `a867a01` local verification records. All were pushed to the existing remediation branch.
- PR #21 at that exact head passed **14/14 checks**. Runs: CI **37311676480**, security-audit **37311676337**, core-beta-evidence **37311676379**, all `success`, all bound to the same head SHA.
- CI job metadata confirms the pinned validator installation and real Robot schema step actually executed successfully on Ubuntu, Windows and macOS; not skipped and not inferred from an old run. Four evidence targets and all installer/MSRV/supply-chain checks passed.
- The approved experience phase is verified. This follow-up changes evidence/spec text only, not product code, workflow, schema or test behavior. The final documentation head is checked separately before delivery; the immutable source checkpoint above remains the reproducible implementation evidence.
- Parent/task archive remains intentionally open for deferred scope: C1/P2-9, broader P0-3 non-identity diagnostics/debug stderr, release rehearsal/governance and measured performance. No release/tag, main merge, maturity change or unrelated cleanup.

## Shared Cargo validation ownership
All Cargo validators sharing one target must be serialized through test execution, not only compilation. A build lock does not guarantee another Cargo command cannot relink a Windows executable still used by a test child. Long-lived manual services use a copied candidate; never kill unrelated processes to clear a build error. The integration checker's initial access-denied attempt was retried only after main's workspace run completed, and its CLI/MCP 7 tests then passed.
