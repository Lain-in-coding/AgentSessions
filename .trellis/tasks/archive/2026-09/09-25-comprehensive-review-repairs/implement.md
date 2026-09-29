# Implementation Plan

- [x] Phase 0: Confirm baseline, read applicable specs, and establish failing regression tests from the review reproductions.
- [x] Phase 1: Repair Unicode/time/native-ID/hook/Grok boundaries.
- [x] Phase 2: Make read opens read-only and migrate only under writer lease.
- [x] Phase 3: Implement logical SQLite snapshots and true multi-session staging for OpenCode/Cursor.
- [x] Phase 4: Repair semantic readiness, filtering, finite scores, pagination, rank fusion, and cursor state.
- [x] Phase 5: Wire MCP to the shared semantic application path and unify provider filter registry behavior.
- [x] Phase 6: Complete Web subset parameters and provider canonicalization.
- [x] Phase 7: Validate no-op batches and provider SQL error propagation.
- [x] Phase 8: Bound embedding rebuild and session metadata SQL; add representative memory/plan evidence.
- [x] Phase 9: Upgrade dependencies, synchronize contracts/specs/changelog, and run all final quality gates.
- [x] Phase 10: Run targeted mutations and record the final review, residual risks and follow-up task links.
- [x] Phase 11: Close embedded Web UI functional gaps: expose existing API filters, invalidate stale cursors on search-state changes, and prevent out-of-order search/context responses from overwriting current state; verify with focused interaction tests.

## Finding Disposition

The source report is `.trellis/tasks/09-25-comprehensive-project-review/research/review-report.md` (baseline `6cd1e6f`). P2/P3 numbers below follow that report's numbered list.

| Findings | Disposition and regression coverage |
| --- | --- |
| P1-1 WAL snapshots | Read-only logical Backup snapshots observe WAL-only updates, survive external checkpoints without false changes, and leave provider data unchanged. `source_fs` WAL regression and killed WAL mutation. |
| P1-2 multi-session sources | Message-level observations preserve separate placements, context, usage events and resume claims. Shared Message projections retain every Session. CLI multi-session WAL regression and killed session mutation. |
| P1-3 semantic filters | Shared metadata/facet/system predicates run before top-k and paging. Application/SQLite and CLI/MCP/Web positive filter tests; killed semantic-filter mutation. |
| P1-4 MCP semantic wiring | Shared query embedding and semantic App composition keep indexed MCP semantic/hybrid searches effective. Positive CLI/MCP parity tests and killed NoSemanticIndex mutation. |
| P1-5 provider filters | Capability registry owns canonical searchable providers and aliases for every entrypoint and MCP schema. Registry parity tests and killed registry mutation. |
| P1-6 native identity collisions | Checked message/parent IDs reject invalid input atomically before normalization. Ingress regression and killed native-ID mutation; historical `ses_v1` unchanged. |
| P1-7 Unicode redaction | Multibyte AWS-prefix text cannot trigger byte-boundary panic. Regression and killed UTF-8 mutation. |
| P1-8 request time | Signed clock fields, impossible dates and checked-arithmetic overflow are rejected. Strict time regressions and killed signed-time/extreme-year mutations. |
| P1-9 read lifecycle | Missing catalogs are not created by reads; old schemas need an explicit leased write path; concurrent read/write behavior is covered. Hook no-query/disabled paths return before opening a catalog. |
| P2-1 cursor binding | Mode/model/vector/ranking/filter context is bound; recency clock and original expiry stay fixed across pages. Mode/model, clock and expiry mutations were killed. |
| P2-2/3/4 semantic paging, readiness and scores | Visibility is applied before the sentinel; readiness errors propagate; non-finite vectors/scores fail explicitly. Application/SQLite regressions cover each boundary. |
| P2-5 FTS rank fusion | Message/session corpus ranks merge by deterministic RRF with canonical-ID ties; relevance signals are scaled to RRF. Killed raw-BM25 mutation. |
| P2-6 no-op validation | Duplicate identities, placements and resume claims are validated before no-op returns. Killed no-op and duplicate-claim mutations. |
| P2-7 provider SQL failures | OpenCode prepare/row/schema errors fail closed; skipped malformed roles prevent missing-record tombstones. Provider/schema regressions. |
| P2-8 calendar ordering | Domain ordering rejects impossible month/day/leap-year combinations. Killed impossible-date mutation. |
| P2-9 relocation identity | Explicitly deferred to the registered additive alias/migration task below. Ordinary sync does not rekey existing Sessions. |
| P3-10 Grok rewind | Checked integer conversion prevents truncation; oversized rewind regression includes a simulated 32-bit truncation mutation. This is not a native 32-bit runtime test. |
| P3-11 hook budget | Detection and truncation share the existing Unicode character estimator. Unicode regression and killed budget mutation. |
| P3-12 Web subset | API declares/validates all supported search parameters; UI exposes them, resets pagination and rejects obsolete async results. Server tests and 11 Node interaction regressions. |
| Optimization work | Keyset embedding batches replace vectors atomically; semantic scan retains bounded top-k; metadata exclusion SQL has bounded shape. Evidence below. Ratatui/Crossterm upgrade removes both lru advisories; optional paste follow-up remains. |

## Validation Evidence

All raw logs are local, gitignored files under `evidence-output/`; they are not shared source artifacts. The results below record executed commands, not inferred CI status.

| Gate | Result | Local evidence |
| --- | --- | --- |
| `cargo fmt --all --check` | Exit 0 | `repair-20260927-final-fmt.log` |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | Exit 0 | `repair-20260927-final-clippy.log` |
| `cargo clippy -p agent-session-grep-cli --all-targets --features semantic-candle --offline -- -D warnings` | Exit 0 | `repair-20260927-final-clippy-semantic.log` |
| `cargo test --workspace --offline --no-fail-fast` | 80 suites; 1693 passed, 0 failed, 20 existing ignored | `repair-20260927-final-workspace-debug.log` |
| `cargo test --workspace --offline --release` | 80 suites; 1693 passed, 0 failed, 20 existing ignored | `repair-20260927-final-workspace-release.log` |
| `cargo test -p agent-session-grep-application --features semantic-candle --offline` | 266 passed, 0 failed | `repair-20260927-application-semantic.log` |
| `node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs` | 11 passed, 0 failed | `repair-20260927-web-ui.log` |
| Python scripts / release / evidence suites | 18 / 9 / 58 tests; 84 passed and 1 existing expected skip in total | `check-python-{scripts,release,evidence}-current.log` |
| `cargo deny check` | Advisories, bans, licenses and sources pass; duplicate warnings remain visible | `repair-20260927-cargo-deny.log` |
| `cargo audit --file Cargo.lock` | Exit 0; one disclosed optional `paste` unmaintained warning | `repair-20260927-cargo-audit.log` |

The 2026-09-27 Cargo/Node gates also have sibling `.exit` files containing their actual process status. The earlier lost test handle was not counted as evidence. Post-cleanup workspace gates, final source review and specification updates are complete.

### Mutation evidence

Eighteen deliberate regressions failed their intended test assertions, rather than failing compilation. Restored code passed the paired checks and the full integration gates:

- Twelve `check-mutant-*.log` / `check-restored-*.log` pairs: UTF-8 redaction, original cursor expiry, cursor recency clock, duplicate resume claims, extreme year, simulated 32-bit Grok rewind, Unicode hook budget, impossible calendar date, MCP semantic App, native ingress, provider registry and signed time.
- `repair-ingest-mutation-{wal,sessions,noop}.log`, restored in `repair-ingest-restored.log` and the full workspace gates.
- `repair-retrieval-mutation-{semantic_filters,cursor_model,fts_rrf}.log`, restored in `repair-retrieval-final.log` and the full workspace gates.

### Performance and interaction evidence

- `repair-retrieval-topk-evidence.log`: exact top-k agrees over 2048 synthetic two-dimensional vectors with 17 retained results; measured debug scan 6.5389 ms. Candidate heap is bounded to k+1; CPU remains an exact O(N*d + N*log(k)) scan. This is not a production p95/RSS or model-quality claim.
- `repair-retrieval-sql-plan-evidence.log`: 1100 message exclusions produce 1786 SQL bytes, one `json_each` input and indexed Session/catalog probes. SQL shape does not grow one correlated expression per hit.
- Embedding regressions assert bounded keyset batches, complete rollback on encoder/vector failure, previous vectors and generation preservation, and successful generation advance.
- Local synthetic desktop browser check passed search, pagination and context at 1029x666 without horizontal overflow. The screenshot stays outside the repository. Node tests additionally exercise stale successes/errors, input invalidation, duplicate page loads, context close/policy changes, token rotation, typing and language switches.
- On 2026-09-27 the old synthetic serve process was absent; Web Access doctor reported zero active tasks/managed targets and no running proxy. No user browser process was terminated.

## Final Review and Spec Sync (2026-09-27)

- The Trellis checker reviewed task/spec alignment and the identity, snapshot,
  retrieval and Web changes, and identified the private SQLite sidecar leak.
  Repeated service rate limits prevented the check agents from finishing;
  the main session completed the remaining SQLite query/transaction inspection,
  implemented the cleanup and ran the final full-workspace gates.
- `repair-20260927-temp-cleanup-red.log`: four real WAL cleanup regressions
  fail against the previous guards on both success and query-error paths.
  `repair-20260927-temp-cleanup-green.log`: both provider suites pass after
  the fix (57 passed, 2 existing ignored). Tests check real sidecar creation
  before asserting their removal, and clean only their own failure artifacts.
- Final inspection confirms metadata exclusions use one bound JSON set,
  non-default facets cannot receive unfiltered metadata hits, semantic scans
  retain bounded top-k, and embedding replacement/generation share one
  rollback-capable transaction. Existing mutation and integration evidence
  covers the corresponding data-flow contracts.
- Phase 3.3 used `trellis-update-spec`: the five affected layer indexes now
  capture checked IDs/calendar parsing, shared provider/search contracts,
  SQLite read/write and temporary-file lifetimes, atomic rebuilds, and the
  embedded Web request/controller lifecycle. The Web scenario includes the
  required signatures, validation matrix, cases, tests and wrong/correct pair.
- Final privacy and diff checks cover the staged source and task artifacts;
  raw logs/screenshots, runtime pointers and developer journals remain local.

## Registered Follow-up and Limits

- Review P2-9 (session relocation): `.trellis/tasks/09-25-session-relocation-aliases/`. Existing `ses_v1` remains unchanged; explicit installation mapping and additive aliases require their own migration design. Relocation is not claimed as repaired by this task.
- Optional semantic dependency warning: `.trellis/tasks/09-25-semantic-dependency-advisories/`. Ratatui 0.30.2 / Crossterm 0.29 remove both lru unsound advisories. `paste` remains in the optional Candle/gemm/tokenizers tree; no audit ignore was added. The follow-up records the dated upstream investigation.
- Default bigram-hash is experimental fuzzy lexical vectorization. These synthetic tests do not establish natural-language model quality, real-corpus performance, or native Linux/macOS/32-bit runtime behavior.
- The final parser temporary-database sidecar leak is fixed. Connection-first destruction now removes the owned main database, WAL and SHM; guard registration also precedes byte writes. No provider source or unrelated temporary file is a cleanup target.
- No push, merge or publication is part of this task.
