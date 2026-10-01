# Independent full-scope check and handoff (2026-10-01)

## Checkpoint and authority

- Reviewer: the dispatched `trellis-check` sub-agent, working independently of the previous coordinator check. No agents were spawned.
- Reviewed baseline: `bbb7c79533c13db584fa5ca2e1e05990d990c8da`.
- Reviewed implementation HEAD: `abcb7d00b0feb6f17757c8638a530b11692c0e2c`.
- HEAD at report creation: `abcb7d00b0feb6f17757c8638a530b11692c0e2c`; branch: `fix/post-reuse-reliability`.
- This is **HEAD plus uncommitted reviewer changes**, not a review of a newly committed SHA. The coordinator separately owns concurrent task/evidence/spec changes. Those changes were not reverted or included as reviewer work.
- Context was loaded from the parent and three archived child check manifests, requirements/design/execution artifacts, applicable specifications, AGENTS.md and workflow. The stale session task breadcrumb was not used as task authority.
- The entire seven-file baseline-to-HEAD product diff was reviewed: Hermes SQLite accounting; Domain ordering; CLI staging/ingest/sync; CLI e2e and Hermes integration tests; SQLite commit activation and installation resolution. The additional Cursor test file was explicitly authorized later for diagnostic assertion propagation only.
- The owner subsequently stopped product/test work and transferred final integration/validation to the coordinator. **This report does not claim a green final debug gate or delivery completion.**

## Findings (fixed)

### F1: Historical placeholder authorization accepted contradictory evidence

File: `crates/agent-session-grep-adapters-sqlite/src/relocation.rs`.

The original predicate checked a `Reconstructed` label but not the complete identity, catalog payload or document attribution. A zero-byte scan whose claimed document described real content was still accepted. The resume check also ignored a resolved working-directory observation when the provider session ID was missing. Both counterexamples failed the new tests before repair.

The proof now requires:

1. The namespace provider is exactly `empty`; the scan has length zero and the empty-byte BLAKE3 fingerprint; scan provider ownership is absent or `empty`.
2. No message, placement, tool-activity or usage membership exists for that source.
3. Each surviving membership is exactly the historical derived empty Document or Session. The complete deserialized sidecar identity (kind, stability and value), catalog payload and membership document attribution must match. Missing/malformed sidecars or catalog rows, foreign IDs, native stability, nonempty content and contradictory container metadata fail closed. Re-derivation is used only to verify the historical placeholder; no native identity is reconstructed or reassigned.
4. A surviving resume row must name the exact empty Session and provider `empty`; both observation states must be `missing`, both values NULL, and `pair_observed` zero. Cwd observations, ambiguous/unknown states, foreign provider/session claims and observed pairs are not empty-only proof. A genuinely missing-only placeholder claim remains allowed.
5. Existing retired-location and relocation guards remain intact. The proof is re-evaluated inside the activation transaction before replacing its evidence; a staging reservation is not authorization.

Tests exercise real historical placeholder entities, full identity/payload contradictions, missing-only versus contradictory resume claims, native/real-provider/retired-alias refusal, shared placeholders and other source bindings, last-claim retirement, changed proof after reservation, and injected activation failure. Failure snapshots compare live tables and generation, excluding only recoverable outbox intent bookkeeping. Shared namespaces are not garbage-collected.

### F2: Partial ingestion did not explain retained history or accepted coexistence

Files: `crates/agent-session-grep-cli/src/lib.rs`, `crates/agent-session-grep-cli/tests/hermes_state_db.rs`.

A valid-plus-orphan Hermes scan committed one row and skipped one, but its warnings only described parser defects. It omitted the accepted consequence: prior claims survive and old/new document-scoped copies can temporarily coexist.

Both ingest and sync now emit the same constant, path-free partial-state warning when skipped rows are present. It appears **once per response**, ahead of provider details, contributes **one** to `diagnostics`, and participates in the existing 16-warning / 512-character bounding path. Provider diagnostics are not discarded to hide the change. No DTO fields, native IDs, content-based deduplication or context-completeness rules changed.

The new real-CLI regression runs both entrypoints with orphan, blank-session and malformed-session cases. It asserts exact committed/skipped counts, visible coexistence warning, accepted temporary copies, retained omitted history, incomplete-context refusal, complete recovery, preservation of another source's claims, and retirement only after the last owner disappears. An additional unchanged-version-2 source regression exercises version-3 completeness reparsing and subsequent no-op convergence.

### F3: Empty-source integration coverage did not prove the claimed historical cleanup

File: `crates/agent-session-grep-cli/tests/e2e.rs`.

The historical repair test asserted that old placeholder documents disappeared without actually seeding them. It now seeds the real Document/Session payloads, full sidecars, memberships and relation marker with a version-2 scan, and proves the placeholder exists before repair.

The additional lifecycle matrix covers ingest/sync, canonical/standalone locations and all four native-session/native-message combinations. It verifies first-empty no-op without durable identity, valid ingestion, empty replacement, repeated-empty no-op, refill, exact identity/namespace preservation and absence of invented discovery ownership.

### F4: Shared-warning propagation required three exact expectation updates

Files: `crates/agent-session-grep-cli/tests/e2e.rs`, `crates/agent-session-grep-cli/tests/cursor_disk_kv.rs`.

The first full post-fix workspace run exposed three old diagnostic-count expectations. After explicit authorization, the assertions were changed from 3 to 4 for Cursor and from 1 to 2 for the two JSONL cases. Each affected test now also verifies the original row diagnostic and exactly one bounded, first-position retention/coexistence warning; Cursor additionally retains its identity and slot-accounting diagnostics. No Cursor parser or policy code changed.

These assertion edits were already written before the final stop/handoff request. They remain uncommitted for coordinator inspection/adoption; their final integration acceptance is not claimed here.

## Full-scope review conclusions

- Hermes uses a bounded exact source-row census; visited bad rows and unread rows are accounted once. The diagnostic orphan cap does not masquerade as the skipped total. Duplicate accepted session IDs and coercing ownership matches fail explicitly; BLOB/TEXT key identity remains protected.
- CLI tail retention is manifest-scoped to record streams. Whole-source adapters retain their own validation; no extension-based registry was added. Empty replacement uses existing claims/installation ownership, while unknown first-empty input does not persist fake identity.
- Storage repairs stay within ordinary source replacement, writer-lease/outbox/manifest/CAS boundaries. Shared claims, real/native identity and alias boundaries remain protected by the strengthened proof and tests.
- Domain now compares one cached lexicographic timestamp key: Missing, Invalid original bytes, Valid UTC instant, then existing document/ordinal/placement tie-breaks. Timestamp parsing and raw values are unchanged; existing nanosecond precision is preserved. Six-permutation, randomized and valid/missing reference tests were reviewed.
- Parser semantic version 3 is the existing cache invalidation mechanism. No schema, dependency, MSRV, release/SLO gate, provider maturity or performance-route change was made.

## Commands and observed results

All commands ran from the isolated worktree. Every Cargo invocation used `--offline`; dependency-resolving commands also used `--locked`. Host toolchain observed locally: Rust 1.97.1, `x86_64-pc-windows-msvc`. Logs below are ignored local artifacts, not portable committed test transcripts.

### Independent baseline and focused red/green loops

| Actual command | Observed result | Local log |
|---|---|---|
| `cargo --offline fmt --all --check` | Pass on initial reviewed HEAD | terminal |
| `cargo --offline --locked clippy --workspace --all-targets -- -D warnings` | Pass on initial reviewed HEAD | `target/independent-check-clippy.log` |
| `cargo --offline --locked test --workspace --no-fail-fast` | Pass before reviewer fixes; not final-patch validation | `target/independent-check-debug.log` |
| `cargo --offline --locked test -p agent-session-grep-adapters-sqlite empty_repair_ --lib -- --nocapture` | Red: 2 failed; contradictory document and resolved-cwd evidence were incorrectly authorized | `target/independent-check-empty-red.log` |
| Same adapter command after proof repair | Green: 2 passed | `target/independent-check-empty-green.log` |
| Same adapter command with shared/rollback/identity cases | Green: 5 passed | `target/independent-check-empty-integration.log` |
| `cargo --offline --locked test -p agent-session-grep-cli --test hermes_state_db partial_state_db_rescans_warn_and_preserve_shared_history -- --nocapture` | Red: missing explicit retention/coexistence warning | `target/independent-check-hermes-red.log` |
| Same Hermes command after warning repair | Green: 1 test passed, covering six entrypoint/defect combinations | `target/independent-check-hermes-green.log` |
| `cargo --offline --locked test -p agent-session-grep-cli --test e2e empty_ -- --nocapture` | Green: 7 passed | `target/independent-check-cli-empty.log` |

### First full post-fix gate and propagation handoff

| Actual command | Observed result | Local log |
|---|---|---|
| `cargo --offline fmt --all --check` | Pass | `target/independent-check-final-fmt.log` |
| `cargo --offline --locked clippy --workspace --all-targets -- -D warnings` | Pass | `target/independent-check-final-clippy.log` |
| `cargo --offline --locked check --workspace --all-targets` | Pass | `target/independent-check-final-typecheck.log` |
| `cargo --offline --locked test --workspace --no-fail-fast` | **Fail: 3 exact diagnostic-count expectations in 2 targets**, detailed below | `target/independent-check-final-debug.log` |
| `cargo --offline fmt --all --check` after assertion propagation | Pass | `target/independent-check-final-fmt-r2.log` |
| `cargo --offline --locked clippy --workspace --all-targets -- -D warnings` after assertion propagation | Pass | `target/independent-check-final-clippy-r2.log` |
| `cargo --offline --locked check --workspace --all-targets` after assertion propagation | Pass | `target/independent-check-final-typecheck-r2.log` |

Formatting was applied with `cargo --offline fmt --all` before the formatting checks. `git diff --check` also passed at the earlier four-product-file checkpoint.

The first full post-fix debug failures were:

- `cursor_disk_kv::a_broken_bubble_does_not_discard_its_session`: expected diagnostics 3, observed 4.
- `e2e::golden_broken_line_syncs_with_visible_diagnostic`: expected 1, observed 2.
- `e2e::sync_new_truncated_source_parses_valid_prefix_recoverably`: expected 1, observed 2.

A second `cargo --offline --locked test --workspace --no-fail-fast` was started before the final ownership-transfer request, writing `target/independent-check-final-debug-r2.log`. On attempting to stop the owned session, the process had already finished. **The coordinator must inspect/adopt that log or rerun the gate; this report deliberately does not certify a green final debug run.** No reviewer Cargo job remains active and no further gate was launched after the transfer.

## Coordinator-owned spec synchronization

The coordinator reported updating the CLI/SQLite specs; this reviewer did not edit them. Their final synchronization should retain the precise F1 proof conditions, pre-replacement transactional revalidation, missing-only metadata allowance, shared-claim/rollback coverage, and the F2 one-per-response diagnostic count and cap ordering.

The Domain index previously described microsecond padding. The implementation and tests preserve **nanoseconds**, including equivalence of fractional zero padding and ignoring digits beyond nine; this documentation correction belongs to the coordinator, not a parser behavior change.

## Findings / gates not closed by this handoff

- Final acceptance of the three diagnostic assertion propagation edits and final full-workspace debug validation is coordinator-owned and **pending in this report**. The three recorded failures are not hidden, converted into skips or treated as passing evidence.
- Release, semantic-feature, Rust 1.90, other-platform and remote PR/main gates were not independently rerun for the final reviewer patch. Earlier coordinator results remain historical only. The coordinator owns these follow-up checks.
- Coordinator-run Node/Python checks are separate evidence, not this reviewer's executions.
- No performance rerun, provider promotion, real-corpus claim, remote operation, commit, push, merge or task archival was performed by the reviewer.
- No additional demonstrated product defect was left open when scope was frozen. This is a substantive independent source review with a validation handoff, not a claim that every final delivery gate is complete.

## Reviewer changed paths

The following product/test paths are uncommitted reviewer changes at report creation:

- `crates/agent-session-grep-adapters-sqlite/src/relocation.rs`
- `crates/agent-session-grep-cli/src/lib.rs`
- `crates/agent-session-grep-cli/tests/cursor_disk_kv.rs`
- `crates/agent-session-grep-cli/tests/e2e.rs`
- `crates/agent-session-grep-cli/tests/hermes_state_db.rs`

Additional reviewer artifact:

- `.trellis/tasks/09-30-post-reuse-reliability/research/independent-check.md`

Concurrent shared-spec and task/evidence edits belong to the coordinator and were preserved. No absolute machine paths, credentials, runtime pointers or journal content are included in this report.
