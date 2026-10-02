# Execution
- [x] Read approved revised plan; preserve dirty original checkout.
- [x] Fetch main and create isolated branch at 2edf2dc.
- [x] Translate approved scope into child tasks and context.
- [ ] Establish immutable baseline tests and classify upstream changes.
- [ ] Implement/check boundaries, model/handoff and provider fixes.
- [ ] Implement/check storage authority, vectors, ownership and snapshots.
- [ ] Implement/check read surfaces and immutable release gates.
- [ ] Reconcile all IDs/C/A dispositions, CHANGELOG and specs.
- [ ] Run full quality gates, build/smoke/packaging.
- [ ] Commit risk-focused batches, push branch, inspect remote CI.

## Gates
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --offline --locked -- -D warnings
cargo test --workspace --all-features --offline --locked --no-fail-fast
cargo +1.90.0 check --workspace --all-features --offline --locked
Python unittest discovery in scripts, scripts/release, scripts/evidence
node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs
python scripts/evidence/privacy_scan.py --repo .
git diff --check

Share a target directory and serialize Cargo builds. All tests use synthetic inputs. Full final validation is not inferred from child tests.

## Item ledger

These are tracking items, not a count of confirmed defects or completed work.

| ID | Current state |
|---|---|
| P0-1 | Implemented; checked native-ID and leading-option rejection; integration gate pending |
| P0-2 | Implemented; Human-only execution, machine refusal; integration gate pending |
| P0-3 | Partial: normalized unknown command, secret-safe diagnostic/progress/MCP errors. Full private-path and adversarial identity audit remains open |
| P0-4 | Verified: matching-placement ownership across lexical/semantic/hybrid/fallback, metadata and grouping; current-repo boost uses the same owner. Old search cursors explicitly invalidated. Full 1904-test integration passed |
| P0-5 | Pending current-baseline revalidation and implementation |
| P0-6 | Pending current-baseline revalidation and implementation |
| P0-7 | Pending current-baseline revalidation and implementation |
| P0-8 | Pending current-baseline revalidation and implementation |
| P0-9 | Implemented and focused tests passed; complete required manifest coverage and unique safe names |
| P0-10 | Implemented and focused tests passed; hard content budget, final confidence and labeled identity |
| P0-11 | Upstream MSRV fix revalidated on 2edf2dc; real MSRV CI gate added. Release/remote gates pending |
| P1-1 | Keep approved fail-closed baseline; upstream tail/partial changes retained; recovery matrix pending |
| P1-2 | Pending current-baseline revalidation and implementation |
| P1-3 | Pending current-baseline revalidation and implementation |
| P1-4 | Pending current-baseline revalidation and implementation |
| P1-5 | Pending current-baseline revalidation and implementation |
| P1-6 | Pending current-baseline revalidation and implementation |
| P1-7 | Pending current-baseline revalidation and implementation |
| P1-8 | Withdrawn diagnosis; current linear Pi behavior retained; tree enhancement deferred |
| P1-9 | Pending current-baseline revalidation and implementation |
| P1-10 | Partial: explicit restart semantics and missing/corrupt model diagnostics implemented; Context activity error propagation verified red/green and in full integration. Readiness/dimension remains open |
| P2-1 | Implemented; actual GET-ack regression failed then passed; POST remains unsupported |
| P2-2 | Pending current-baseline revalidation and implementation |
| P2-3 | Pending current-baseline revalidation and implementation |
| P2-4 | Pending current-baseline revalidation and implementation |
| P2-5 | Pending current-baseline revalidation and implementation |
| P2-6 | Pending current-baseline revalidation and implementation |
| P2-7 | Withdrawn; no mapping from historical tool errors to current request failure |
| P2-8 | Deferred product decision; existing ranking policy preserved |
| P2-9 | Conditional platform/hint work pending reproduction |
| P2-10 | Partial: immutable quality/build/assembly and static regressions implemented. Controlled mismatch rehearsal and privacy policy expansion pending |
| P3-1 | Retain upstream batch-scoped optimization; new performance baseline and correctness cases pending |
| P3-2 | Pending current-baseline revalidation and implementation |
| P3-3 | No cleanup: target-by-target inventory/backup/approval not completed |
| P3-4 | Final advisory and release governance pending; no automatic ignore or feature removal |
| P3-5 | Existing Experimental matrix retained; evidence/variant updates pending |
| P3-6 | No deletion authorized by target; do not infer cleanup permission from general Git permission |
| C1 | Symlink/tombstone scenario not reproduced in this batch |
| C2 | Implemented after two failing short-read/error regressions; all 12 source_fs tests passed |
| A1 | Deferred or pending; do not silently implement broad product/architecture changes |
| A2 | Deferred or pending; do not silently implement broad product/architecture changes |
| A3 | Deferred or pending; do not silently implement broad product/architecture changes |
| A4 | Deferred or pending; do not silently implement broad product/architecture changes |
| A5 | Deferred or pending; do not silently implement broad product/architecture changes |
| A6 | Deferred or pending; do not silently implement broad product/architecture changes |
| A7 | Deferred or pending; do not silently implement broad product/architecture changes |
| A8 | Risk-focused commit/review discipline being applied; no public history rewrite |

## First-batch evidence (2026-10-02)
- Fixed baseline 2edf2dc: Rust 1.90 all-feature check passed. Initial all-workspace test was interrupted because it saturated local resources; it is not a passing baseline result.
- Actual red regressions: manifest coverage; three handoff budget/identity cases; two short-read/error cases; two release/source workflow contracts; HTTP GET acknowledgement.
- Actual green focused results: model/handoff module cases, 12 source_fs tests, 5 workflow tests, stateless HTTP preview/CLI gate regression.
- Shared application run at one intermediate point: 306 passed, one still-red resume test from the concurrent boundary work. Do not misreport this intermediate run as all green.
- Final all-feature Clippy passed with -D warnings. Low-concurrency workspace/MSRV integration is running; final totals belong below after it finishes.
- Boundary implementation was delegated. Other implementation/check delegates were rate-limited; main continued and inspected changes. No completed independent multi-agent review is claimed.
- Scope correction: source_fs is in adapters-sqlite, not CLI. Cache retry remains explicit process restart; no automatic 30-second reload was added.
- Original dirty checkout is unchanged. No commit/push/tag/release or cleanup has happened yet at this checkpoint.

## Continuation evidence (2026-10-03)
- Reconfirmed remediation branch at 2edf2dc and the original checkout's eight dirty entries; original checkout remains untouched.
- Formatting, all-target/all-feature Clippy, the complete all-feature workspace test run and Rust 1.90 all-target/all-feature check passed (offline/locked, two Cargo jobs and two test threads). This run predates the diagnostic-redaction review correction, which requires fresh checks.
- Python discovery passed: scripts 18 tests (1 skipped), scripts/release 11 tests, scripts/evidence 54 tests (1 skipped). Node Web tests: 11 passed. All seven task context manifests validated.
- Privacy scan initially found 22 machine/personal path occurrences in six archived throughput evidence files already present at HEAD. A bounded repair replaced only path roots and made the trace script resolve the repository from its own location. Metrics/hashes were preserved; three JSON artifacts parse and differ only in nine path/command fields. Scanner and allowlist were not changed. Main re-ran the scan successfully. Historical benchmarks were not rerun.
- Independent first-batch review completed. It identified and fixed a diagnostic-redaction bookkeeping regression: pre-redaction discarded counts and changed Human rendering. Six diagnostic regressions passed after a red-to-green run; machine output now redacts before truncation at the output boundary, keeps accounting, and leaves Human diagnostics raw. Reviewer reran fmt, all-feature/all-target Clippy and Rust 1.90 CLI check successfully. Main reviewed the fix and started another complete workspace/MSRV run.
- Debug-binary synthetic release smoke initially returned 8/10. Both failures were verifier defects: platform model discovery was not isolated by --db, and disabled hooks use empty stdout rather than a JSON envelope. Child-process-only platform paths now isolate synthetic state; the hook check rejects any stdout or nonzero exit. Nine verifier tests passed, including parent-environment preservation. Main independently reran real smoke: 10/10 passed. Final scripts discovery: 21 tests (1 skipped); release 11 and evidence 54 (1 skipped); Node 11 passed. This is debug-binary smoke, not release packaging, real-model quality or remote CI evidence.
- Final post-review workspace run: **1895 passed, 0 failed, 22 ignored**; Rust 1.90 all-target/all-feature workspace check passed. Both used offline/locked dependencies. Ignored cases remain unverified; no cross-platform result is implied by local Windows execution.
- Remaining storage/provider/read-surface ledger items stay open. This batch is ready for risk-focused checkpoint commits; it does not close or archive the parent task. Remote CI, packaging and the controlled mismatch rehearsal remain pending; no tag/release is authorized by this checkpoint.

## Second-batch evidence (2026-10-03)
- P0-4 and the Context activity part of P1-10 implemented and independently reviewed; related current-repo ownership mismatch corrected without changing ranking weights.
- Final Windows all-feature workspace gate: **1904 passed, 0 failed, 22 ignored**. Formatting, all-target/all-feature Clippy, and Rust 1.90 all-target/all-feature locked/offline check passed.
- Python suites: scripts 21 (1 skipped), release 11, evidence 54 (1 skipped); Node 11 passed. Real debug-binary synthetic smoke: 10/10. Privacy scan, diff checks and storage context manifests passed.
- Old search cursors must restart because the digest now binds matched-owner ranking. List cursors and persisted schema are unchanged.
- Original checkout's eight dirty entries remain untouched. The storage task remains in progress: snapshots, per-source projections, claimant removal and vector invalidation are not implemented by this batch.
