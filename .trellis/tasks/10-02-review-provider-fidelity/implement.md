# Execution
- [x] Read curated specs and parent baseline.
- [ ] Recheck P1-2/P1-3/P1-4/P1-5/P1-6/P1-7/P2-5/P2-6 at 2edf2dc; add failing regression.
- [ ] Implement smallest root-cause fix in exclusive scope.
- [ ] Run package tests/Clippy and report exact commands/results.
- [ ] Independent check and shared-boundary integration.

No child commit, push or recursive agents. Coordinate shared Cargo target builds.

## Timestamp/BOM slice (base df9746f)
- Cursor P1-2 and Cline P1-3 are implemented as disjoint provider slices with serialized Cargo gates. No dedicated Cursor/Cline spec indexes exist; actual generic ports/testkit specs and committed provenance guide this work, not Codex-only field rules.
- Cursor must cover legacy ItemTable chatdata/prompts and current disk-kv. Cline must keep single-document BOM handling local to its reader/probe/parser, not duplicate the existing JSONL reader's first-line handling.
- Main owns CLI time-boundary/reparse tests and parser semantic version 4 -> 5. Existing unchanged sources must be reparsed once; schema version stays 19.
- Conversion must use demonstrated field/variant units, retain equivalent instants, preserve native identity and source bytes, and keep the existing inclusive-since/exclusive-until contract. Cross-source and reconstructed-ID upgrade implications must be checked explicitly.
- Temp DB ownership, other provider fidelity and maturity changes are not part of this slice.


## Timestamp/BOM integration evidence
- Existing Cursor/Cline fixtures and provider tests reproduce the original unit,
  absent/empty-time and leading-BOM defects; evidence is pinned in each provider's
  golden PROVENANCE. Cline fixture revision is 2; all other providers remain 1.
- SQLite's original two-copy ItemTable test failed on the decimal/RFC timestamp
  conflict, then passed with the proven aggregate-only alias. Expanded tests
  exposed missing batch-scoped Document proof for an unscanned claimant; loading
  only referenced missing proofs fixes that without broadening aggregate IDs.
- The five `cursor_timestamp_upgrade_*` storage regressions now pass: both orders,
  1ns differences, three-source null folding, per-claimant document proof, complete
  removal versus incomplete retention, exact raw evidence, no-op and other stable
  field conflicts. The final CLI tests also exercise actual identical snapshots,
  both rolling upgrade orders, exact raw observations, source bytes, stable IDs,
  half-open millisecond/nanosecond filters and unchanged-source no-op behavior.
- Independent final read-only review covered production, specs and all new CLI
  regression assertions; no blocking finding remained. The reviewer independently
  ran rustfmt/diff checks, not Cargo; Cargo results below were executed by main.
- Final Windows all-feature workspace gate: **1966 passed, 0 failed, 23 ignored**.
  The additional ignored case is a manual Cline golden-output printer, not a
  disabled regression; the new golden assertion and all timestamp tests ran.
  All-target/all-feature Clippy with -D warnings, fmt and Rust 1.90 locked/offline
  all-target/all-feature checks passed. Python 21/11/54 (two skips), Node 11,
  privacy/diff/context checks and actual debug-binary smoke 10/10 passed.
- P1-2/P1-3 scoped implementation is verified locally. Remote checks must bind the
  pushed SHA; prior df9746f results do not certify this slice. Other provider work
  and the parent remediation task remain open; no archive, release or promotion.

### Root-cause prevention
- Categories: cross-layer representation contract, test coverage gap, and implicit
  numeric-unit assumptions. Provider-only green tests cannot prove catalog rolling
  upgrade safety; copied sources share IDs even when no native ID is adopted.
- A pairwise null-converging fold can conceal disagreements among non-null original
  timestamps. Prove units per claimant and compare originals before folding.
- Float-token decoding can destroy the evidence needed to reject fractional values.
  Test raw numeric lexemes, not already rounded Values. Shared ports/storage specs
  now pin these contracts; no broader provider or parser fallback is introduced.


Implementation checkpoint: `9a36ad6` (provider code, storage compatibility, CLI
regressions, specifications and CHANGELOG).

### Final verification commands
```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --offline --locked -- -D warnings
cargo test --workspace --all-features --offline --locked --no-fail-fast -- --test-threads=2
cargo +1.90.0 check --workspace --all-targets --all-features --offline --locked
python -m unittest discover -s scripts -p 'test_*.py'
python -m unittest discover -s scripts/release -p 'test_*.py'
python -m unittest discover -s scripts/evidence -p 'test_*.py'
node --test crates/agent-session-grep-cli/tests/web_ui.test.cjs
python scripts/evidence/privacy_scan.py --repo .
python .trellis/scripts/task.py validate 10-02-review-provider-fidelity
python scripts/verify-release.py --asg <built-debug-binary>
git diff --check
```
All source inputs were synthetic and source-byte preservation was asserted.
The existing expected Cursor JSON has only six semantic timestamp differences;
its generated output is now LF to satisfy ordinary diff checks without a
whitespace override. The three existing Cursor binary fixtures remain unchanged.


## Conversation-fidelity execution slice (base 20f30c8)
- [x] Reproduce Grok multi-prompt misclassification and whitespace loss before fixes.
- [x] Reproduce OpenClaw thinking-only zero-loss report without exposing reasoning.
- [x] Preserve chunk/rewind semantics and honest derived identity; verify Resume refusal.
- [x] Keep legitimate empty content distinct from non-indexable content loss.
- [x] Check parser-version upgrade, unchanged-source reparse and legacy source authority.
- [x] Independent review and full local/remote gates; no maturity or schema expansion.
Provider changes require new synthetic fixtures with explicit provenance; never
mutate a real source corpus or use model/CI failure as successful validation.


### Conversation-fidelity implementation evidence (2026-10-04)
- Grok red regressions at parser 5 reproduced `Some("turn-0")` instead of absent
  native identity and `alphaomega` instead of `\talpha \n omega\t` (2 failures).
  Pinned synthetic expected output now preserves whitespace, rewind and seq;
  the 2/3/5-turn table and fixed-seed oracle no longer encode the false identity.
- OpenClaw red run: 20 passed / 7 failed / 2 manual printers ignored. Five failures
  proved missing loss accounting; two guarded fixture revision 2. Green: 27 passed,
  0 failed, 2 manual printers ignored. Missing/null/empty-string/whitespace-string/
  empty-array remain silent; other textless content reports one loss per message.
- Main reran both provider suites: **64 passed, 0 failed, 4 ignored printers**.
  Package all-target/all-feature Clippy passed for both providers. New fixtures
  are entirely synthetic; original source inputs remain unmodified.
- Real compatibility experiment used a disposable catalog produced by the actual
  parser-5 binary before rebuilding, not just lowered scan markers. Two identical
  Grok files under distinct synthetic installation roots yielded two old native
  Sessions and two shared Messages. Reparse source A with parser 6: retain source
  B's exact original evidence and old Session, introduce one document-derived
  Session and correct the text. Reparse B: converge to one reconstructed Session,
  preserve both Message IDs, remove old Session IDs/aliases, keep Resume unavailable.
  Generations were 3 -> 4 -> 4; the last unchanged sync emitted zero. Both input
  files remained byte-identical. This verifies complete replacements; incomplete
  scans deliberately retain prior observations under existing storage policy.
- Python suites 21/11/54 ran successfully (two existing environment skips); Node
  Web UI suite passed 11/11; curated task context validation passed 8 entries each.
  Subsequent CLI/independent/full-workspace gates are recorded below. New-SHA
  remote gates remain required before the parent ledger marks these items Verified.

### Root-cause prevention: conversation fidelity
- Categories B/D/E: turn identifiers were promoted across the provider/session
  boundary without evidence, whitespace was discarded before message composition,
  and a valid record was conflated with successfully indexed content.
- Existing goldens and a mirror-model property oracle had encoded the same wrong
  native-ID assumption. Revision-2 fixtures use explicit independent text/span
  expectations; semantic counterexamples precede fixes. Reference adapters are
  field-shape evidence, not correctness or format-certification authorities.
- The ports spec now pins identity scope, the closed empty-content set, loss vs
  intentional text-only projection, and parser-6/source-replacement consequences.
  CLI assertions and a real old-binary upgrade complement provider-only tests.
  No reasoning/tool capability expansion, path guessing or maturity promotion.


### Independent check and CLI boundary evidence
- Both new CLI regressions pass. Grok checks exact five-message/first-span/seq
  output, reconstructed Session identity, absent native/cwd/Resume authority,
  seeded old body in catalog/source observations/FTS, parser-5 marker backfill and
  subsequent no-op. This automated body test does not claim original legacy-ID
  migration; the actual parser-5 executable experiment above covers that boundary.
- OpenClaw CLI covers fresh import and replacement after indexed history. Two
  non-indexable messages produce two skips plus the existing retention diagnostic;
  genuine empty content contributes no loss. Negative searches and machine
  diagnostics exclude reasoning; incomplete replacement retains old history.
- Independent Trellis check reviewed every changed file and all four new fixture
  files, including final CLI additions. The only finding was a stale "current
  parser 5" sentence in the SQLite spec; it now names 6 and retains historical
  parser-5 notes. No Rust/test defect remained. The reviewer independently ran
  scoped rustfmt/diff and Python fixture field/span/line/byte checks; Cargo and the
  real old-binary migration experiment were run by main, not independently rerun.
- Final binary verification passed 10/10; privacy scan includes the four staged
  intent-to-add fixtures and passed. No source bytes or Cargo.lock were modified.


### Conversation-fidelity final local gate
- Main ran the same complete command matrix listed under Final verification
  commands above. Windows all-feature workspace result: **1979 passed, 0 failed,
  24 ignored** across 86 test targets. Versus the prior 1966/23 checkpoint, all 13
  added regression tests ran; the one new ignored test is an OpenClaw manual
  golden printer, not a disabled regression.
- Workspace fmt, all-target/all-feature Clippy with `-D warnings`, and Rust 1.90.0
  all-target/all-feature locked/offline check passed. Python 21/11/54 (two skips),
  Node 11, real binary smoke 10/10, fixture-inclusive privacy scan, context and
  ordinary diff checks passed. No whitespace-check override was used.
- Independent review is complete and its sole documentation finding is fixed.
  Existing Grok basic expected JSON is now LF; its only semantic change is the
  approved native session ID removal. Original fixture inputs remain unchanged.
- P1-4/P1-5/P1-6 are locally verified. The first push and its exact-SHA remote
  checks are the remaining batch delivery gate. Parent and provider tasks stay
  open for other items; no archive, main merge, formal release or promotion.


### Conversation-fidelity remote checkpoint
Implementation: `452c883cc5a46a0dcb0268c28b3e9d49b00be0ef`.
All three completed runs were checked against this exact SHA; PR #21 reported
**14/14 successful checks**, not results from an earlier timestamp checkpoint.

| Workflow | Run ID | Result |
|---|---|---|
| ci | 37173224386 | success |
| security-audit | 37173224639 | success |
| core-beta-evidence | 37173224455 | success |

The scoped P1-4/P1-5/P1-6 acceptance is verified, including independent review
and local/remote integration. A documentation-only follow-up records this result;
its own pushed head must be checked separately, not inferred from these run IDs.
No provider promotion, official release, merge, archive or source cleanup occurred.

### Next confirmed counterexample (P2-5; not fixed in this checkpoint)
While remote CI ran, the current debug binary was exercised with two synthetic
Codex sources in disposable catalogs. Both contain one `session_meta` and one
valid `response_item/message`; only the session field name differs:
- `session_meta.payload.id` plus cwd: message is indexed, but native session ID
  and original working directory are both missing and Resume is unavailable.
- The same value under `session_meta.payload.session_id` plus the same cwd:
  native ID and the same-record cwd are observed, and Resume metadata is available.
This confirms the type-scoped alias gap at `452c883`. Do not turn arbitrary
id-only records into messages; same-record alias conflicts still need explicit
regressions in the next slice. P2-5 remains open; no code change was made for it.


## Metadata/probe execution slice (base 57529db)
- [x] Reproduce Codex current/root identity and type boundaries, Kimi prompt/steer probe
  mismatch and Qoder cross-record/body SID/cwd pairing before each fix.
- [x] Add synthetic revision-2 fixture evidence and minimal provider changes.
- [x] Verify CLI provider selection, current/root identity scope, no invented Resume
  authority and unchanged-source rolling upgrade.
- [x] Update parser/version and shared executable specs without capability promotion.
- [x] Independent review; full local gates; commits/push and exact-SHA remote gates.
Previous 57529db delivery is confirmed: ci 37173862774, security-audit 37173862769,
core-beta-evidence 37173862760 all succeeded; PR #21 had 14/14 successful checks.
This does not validate the new metadata/probe code before its own gates run.


### Metadata/probe evidence correction (2026-10-04)
- The original P2-5 alias-conflict rejection was a misdiagnosis, not a format
  invariant. Official openai/codex protocol.rs at fixed commit
  `c5d242fa7907bff1b7a7e26e95febc548c0a6963` (2026-10-02) defines `id: ThreadId`
  and `session_id: SessionId` with root-thread semantics (lines 3130-3132).
  SessionMetaLine fills absent session_id from id (3259-3265); rollout metadata.rs
  builds thread identity from meta.id (45) and cwd from meta.cwd (59).
- New distinct-root regression failed against the uncommitted alias-rejection
  implementation with StructuralFatal, then passed after selecting current id.
  Both byte and bounded-source entry points cover legal differing fields, with
  and without an earlier Message. A separate matrix covers ten field shapes;
  same-root/different-current headers still report multi-session ambiguity.
  The original id-only omission remains a real bug; root-only identity chosen by
  parser 6 for child rollouts is also incorrect. No real transcript was copied.
- Kimi's resumed implement worker failed with an upstream cross-resource session
  error before execution; main finished the reviewed golden and package checks.
  Provider all-target tests and all-target/all-feature Clippy passed for Codex,
  Kimi and Qoder. Original fixtures are unchanged. Qoder/Kimi property suites
  pass unchanged; the Qoder random oracle never asserted cwd authority, which is
  independently covered by the new fourteen-case same-body table and golden.
- Parser 7, CLI integration, actual old-catalog migration and full/remote gates
  still require verification below; this is not yet a completed delivery.


### Actual parser-6 to parser-7 replacement evidence
- Preserved the actual pre-rebuild parser-6 executable (SHA256
  `680b4579f512b1744887a609c324334358c968f25761ea116e7e1ef480fec347`). Its
  synthetic catalogs recorded parser_version=6 and schema=19. Tests copied these
  databases through SQLite backup, never by lowering new parser markers.
- Codex id-only, two independent noncanonical installation roots: old metadata
  was Missing and one document-derived Session was shared. Rolling replacement
  retained the unscanned source's exact resume claims and original entity
  projections, preserved the native Message ID, then removed the old Session and
  all stale message aliases. Session counts were 1 -> 2 -> 2: each installation
  correctly owns its own native Session after metadata recovery. Generations were
  2 -> 3 -> 4 -> 4; final unchanged scan emitted zero for both sources.
- Codex current/root IDs, two copies under one canonical installation: parser 6
  chose the root as Resume identity. Parser 7 selected current-thread identity;
  Session counts were 1 -> 2 -> 1, generations 1 -> 2 -> 3 -> 3. The unscanned
  source's old root claim remained byte-identical until its own complete reparse;
  afterward the old root Session and stale aliases disappeared. Message IDs,
  message text and source bytes were unchanged.
- Qoder, two same-installation copies with separate SID-only and cwd-only records:
  parser 6 incorrectly set pair=1 and a cwd. The first replacement made only that
  source's cwd Missing/pair=0; unscanned original observations were untouched.
  The second replacement converged both to Missing/pair=0 without changing Session
  or Message IDs. Generations 1 -> 2 -> 3 -> 3; final unchanged scan emitted zero.
- All three experiments asserted exact source bytes, native/message identity,
  original unscanned projection bytes, schema 19 and per-source 6/7 markers. Real
  search and get-session-resume commands succeeded after convergence. These are
  complete-scan cases, not a claim that incomplete scans discard retained evidence.
- An initial experimental assertion wrongly expected one final Session across
  two independent installation namespaces. Inspecting claims/identities corrected
  that test expectation; no production cross-namespace merge was introduced.
- Provider gate totals: **130 passed, 0 failed, 4 ignored** across nine targets;
  three manual golden printers and one timing microbenchmark remain ignored. Python 21/11/54 (two existing
  environment skips), Node 11/11 and real parser-7 binary smoke 10/10 passed.
  Privacy scan includes all six intent-to-add fixture files and passed. Full Rust
  workspace and remote-SHA gates remain pending until recorded below.


### Metadata/probe independent check and final local gate
- Independent Trellis checker reviewed all changed providers, the six new fixture
  files, final CLI additions, parser/schema constants, provenance and shared specs.
  It independently fetched fixed upstream Codex/Kimi evidence and checked fixture
  fields/order, exact byte spans, UTF-8/LF/BOM/CRLF, original fixture immutability,
  Git -text attributes and task references. Scoped rustfmt/diff checks passed.
- Review found two stale identity comments, not runtime defects. The checker fixed
  CodexAdapter's current/root description; main corrected ParseReport's old Codex
  example in ports. No blocking code/test defect remained. The checker did not
  independently run Cargo, old-catalog experiments or BLAKE3; those checks belong
  to main and the implementation worker, not to the independent review.
- CLI worker added four metadata_probe_* regressions and updated the exact provider
  revision map. All four and the complete 35-test provider matrix passed, as did
  all-target/all-feature CLI Clippy. Its lowered-marker test only proves reparse
  triggering; the actual old-executable experiments above prove migration effects.
- Main's all-feature Windows workspace suite: **2001 passed, 0 failed, 24 ignored**
  across 86 test targets, with all 22 added regressions executed and no new ignored
  tests. Workspace fmt, all-target/all-feature Clippy with -D warnings and Rust
  1.90.0 all-target/all-feature locked/offline check passed. The final remaining
  edit is the two-line ports documentation correction, not executable behavior.
- Python 21/11/54 (two environment skips), Node 11, binary smoke 10/10, fixture-
  inclusive privacy scan and task-context validation passed. Original worktrees,
  Cargo.lock, sources, provider maturity and schema 19 are unchanged. Commit/push
  and exact new-SHA remote gates still must be recorded; no main merge or release.


### Metadata/probe remote checkpoint
Implementation `83528d944182919d8c503d9de0851bacc0ff9a2a` passed:
- ci `37200315199` (all three OS tests/installers, supply chain and Rust 1.90);
- security-audit `37200315202`;
- core-beta-evidence `37200315200` (Windows, Linux, macOS Intel and Apple Silicon).
All three workflow run records name that exact head SHA, and PR #21 reports
14/14 successful checks. The repository remains public; the PR remains draft.
GitHub reports the existing repository's move to its canonical owner/name; no
remote configuration, visibility or repository ownership was changed by this work.
A transient Git TLS failure succeeded on an unchanged read retry. gh run watch
also hit a transient annotations-request EOF; workflow run/job APIs, not that
watch exit code, confirmed success. No TLS validation or failed check was bypassed.
The final ports comment correction passed a fresh fmt and ports doc-test command;
there are no ports doctests, so it adds no functional test count. No merge,
release, archive or unrelated worktree cleanup occurred. Later documentation
commits have separate SHAs and require their own remote check, not an inferred pass.


## Temp SQLite ownership execution slice (P1-7; base 83528d9)
- [x] Read source-backed research for Cursor, OpenCode and Hermes; include sidecar
  ownership rather than treating main-file create_new as protection for the group.
- [x] Add and run baseline counterexamples through the actual lifecycle helpers.
- [x] Implement exclusive per-copy directory and exclusive/private DB creation;
  cleanup ownership must follow successful creation and close handles first.
- [x] Verify conflict preservation, injected write/sync errors, SQLite query/open
  failures, normal/sidecar cleanup, concurrent copies and Unix access bits.
- [x] Independent check; scoped and full local gates; new-SHA remote checks.
P1-7 red/green evidence, Windows lifecycle checks and the Ubuntu-only permission
and symlink results are recorded below; metadata/probe closure above is not used
as evidence for this slice.


Metadata documentation/planning head `88484ed0c8186519c549eb589e4dc7b3ca8cb8ab`
also passed its own ci `37201508669`, security-audit `37201508667` and
core-beta-evidence `37201508665`; PR #21 had 14/14 successful checks. The later
P1-7 slice is commit `5ca05e2` and carries its own remote checks; the earlier
SHA's results are not reused for it.


### P1-7 Cursor implementation checkpoint
- Cursor worker first ran three regressions through the same production lifecycle:
  **0 passed / 3 failed** for pre-existing DB truncation, failed creation deleting
  foreign sidecars, and sidecar-only namespace deletion. The same three became
  green after the ownership correction. No copied fake legacy implementation was
  used as the tested path.
- Completed package result: **78 passed, 0 failed, 2 existing ignored**; package
  all-target/all-feature Clippy -D warnings, scoped rustfmt and diff checks passed.
  Main has not yet rerun the complete P1-7 workspace or granted full-slice closure.
- Cursor now separately owns the exclusive directory and successfully created DB;
  the write handle is moved inside the DB guard lifetime, while returned TempDb
  keeps conn first. Fixed owned names are removed before nonrecursive empty-dir
  removal; unknown entries survive. Both SQLite variants/fixtures stay unchanged.
- Controlled write/sync faults are injection, not OS disk-failure experiments.
  Real Windows non-delete-sharing handles exercise close-before-cleanup; three
  Unix permission/symlink cases await Unix CI. ACL inheritance and best-effort
  cleanup limitations remain explicit. OpenCode/Hermes and independent full-slice
  review are still pending, so this is not a P1-7 Verified claim.


### P1-7 OpenCode/Hermes implementation checkpoint
- The shared-lifecycle baseline regressions actually produced **12 failures**
  before the semantic fix, then **12 passes**. Expanded ownership coverage ran
  **32 passing tests**. No fixture, field parsing or SQL/transaction change was
  used to obtain the result.
- Windows mutation check deliberately restored the wrong write-handle/guard drop
  order; all four write/sync failure tests failed. Restoring the correct source
  returned them to green. Non-delete-sharing handles provide a real OS ordering
  check; injected write/sync/open errors remain identified as controlled injection.
- Final all-target/all-feature package results: OpenCode **41 passed / 0 failed /
  1 existing ignored**, Hermes **69 passed / 0 failed / 2 existing ignored**.
  Combined package Clippy -D warnings, scoped rustfmt and ordinary diff checks
  passed. Six Unix permission/symlink cases await remote Unix execution.
- Together with Cursor, provider packages report **188 passed / 0 failed /
  5 existing ignored** on Windows. Main is now running full workspace integration;
  the independent checker is extending its Cursor review to all final providers.
  No new dependency, fixture revision or parser/schema bump is introduced.


### P1-7 full local gate evidence (main, commit 5ca05e2)
- Workspace gates on the P1-7 working tree: rustfmt exit 0; workspace Clippy
  -D warnings exit 0; workspace tests exit 0 with **86 targets / 2049 passed /
  0 failed / 24 ignored**; MSRV (dev-profile check of all targets) exit 0.
- Non-Rust gates: Node web UI 11 passed / 0 failed; Python scripts 21 (1 skip),
  release 11, evidence 54 (1 skip inside 54) all OK; privacy scan reported no
  personal path findings; task validation passed.
- Real non-delete-sharing handle ordering tests ran on Windows: while such a
  handle is alive `remove_file` must fail; after drop the four fixed names and
  the owned directory are asserted gone.

### P1-7 independent check and remote checkpoint (commit 5ca05e2)
- A fresh independent checker (read-only, separate session) re-derived the slice
  from the diff and reported: exclusive-directory/exclusive-DB ordering, conflict
  preservation, cleanup authority with close-before-remove ordering, and the
  no-new-dependency / no parser-schema bump / no global TEMP-umask mutation
  constraints were all VERIFIED with no counterexample found. The only
  documentary defect was the stale 'no P1-7 red/green obtained yet' sentence,
  removed in this commit; it also listed three non-blocking test-strength
  weaknesses, handled below. It could not reconstruct the historical RED counts
  or the workspace/remote totals from the commit alone and did not claim them.
- The checker independently reproduced package results (Cursor 78/0/2, OpenCode
  41/0/1, Hermes 69/0/2 = 188 passed, 0 failed, 5 pre-existing ignored), package
  Clippy -D warnings and rustfmt --check. Its cross-target type-check attempt
  stopped at a missing x86_64-linux-gnu-gcc toolchain, not at a code failure.
- Ubuntu CI executed all nine Unix-only cases and each passed: Cursor permissions,
  namespace symlink and DB/dangling symlinks; OpenCode and Hermes each permissions,
  directory symlinks and post-acquisition file symlinks. The Windows, macOS and
  Ubuntu test jobs were all green on this commit.
- Remote checks bound to this exact commit: ci `37254129995`, security-audit
  `37254130043`, core-beta-evidence `37254130014` all succeeded; PR #21 reported
  14/14 successful checks at head `5ca05e2`. Remaining platform limits: Windows
  symlink/ACL behavior is not exercised (the slice does not claim owner-only DACL
  or same-user tamper resistance) and owned-directory removal stays best-effort.
- Reviewer-identified non-blocking weaknesses, kept rather than dropped:
  (a) Cursor's existing-main and sidecar-only cases are gated at the directory
  layer, with file-layer collisions separately covered by injected create_new
  tests; (b) OpenCode/Hermes existing-empty-namespace cases previously asserted
  only is_err, and this commit pins them to the directory layer with panic
  closures so a future file-first implementation cannot silently lose the oracle;
  (c) Cursor's injected open-failure case covers sidecar and directory cleanup,
  while guard deletion of the main file stays covered by write/sync,
  same-candidate, WAL and query-failure cases.


## UTF-8 tail/mid-file decision and matrix slice (P1-1; base 428f535)
- [x] Record the D3 decision: keep the existing safety baseline (an indexed
  record-stream source whose JSON tail is cut at EOF is Retained without parse,
  fingerprint or generation advance) plus the existing recoverable valid-prefix
  path for never-indexed sources; no --strict switch, no change to which bytes
  become searchable, no silent tombstone, no permanent fingerprint for unfinished
  sources.
- [x] Add CLI e2e matrix through the real binary: (a) an indexed source with a
  mid-file invalid row followed by valid rows must scan rather than Retain, keep
  old membership and index the later valid rows; (b) indexed source EOF truncation
  followed by completion must converge without loss or duplication; (c) a
  never-indexed source with an EOF-truncated tail followed by completion must
  converge without duplication while relation_complete stays false only while
  incomplete. Verify actual classification (Clean/Partial/Invalid) before fixing
  expected behavior to a guess.
- [x] Assert retained / skipped / diagnostics / warnings stay individually
  attributable in the single-source result and batch outcome, and that none of
  the matrix cases cache an unfinished source as complete.
- [x] Run CLI/package gates, then the full local gate on the final tree;
  independent check; SHA-bound remote checks. No parser/schema bump expected.

### P1-1 D3 decision
Keep the existing two-path safety baseline. An indexed record-stream source
whose JSON tail is cut at EOF (`JsonlHealth::Invalid` with a cached fingerprint)
stays Retained: no parse, no fingerprint advance, no generation advance, no
commit. A never-indexed source takes the existing recoverable valid-prefix path:
valid prefix committed, truncated row counted as `skipped` plus diagnostics,
`relation_complete=false`. No `--strict` switch is added, which bytes become
searchable is unchanged, no silent tombstone is introduced, and an unfinished
source never leaves a complete relation fingerprint: `source_scans` may cache
its exact-bytes fingerprint as a parse cache, but the completeness marker
`source_relation_scans` is absent for it (deleted on an incomplete commit,
re-inserted only by a complete scan).

### P1-1 matrix e2e evidence (implement worker, 2026-10-05, base 428f535)
Verified classification facts (read-only inspection; no product code changed):
- `crates/agent-session-grep-cli/src/lib.rs:4600-4641`: `jsonl_health` is
  tri-state — `Clean` (no malformed row), `Partial` (malformed row with at least
  one valid row after it), `Invalid` (no valid row, or the last malformed row
  sits at EOF).
- `lib.rs:4842-4852`: the Retain branch requires a cached fingerprint
  (`cached_fp.is_some()`) AND a `RecordStream` provider hint AND
  `health == JsonlHealth::Invalid`; `4860-4870` records `retained`, emits the
  truncated-tail diagnostic and commits nothing; `4871-4893` routes Partial and
  never-indexed sources to `stage_with_source` (recoverable-skip).
- `lib.rs:3314-3324` (`provider_is_record_stream`) derives record-stream status
  from the adapter manifest; the JSONL fixture shape used by the matrix is
  claimed by the `claude-code` adapter.
- `lib.rs:5029-5043`: the single-source sync frame keeps `retained` (source
  count) separate from `emitted`/`messages`/`committed`/`skipped`/`diagnostics`/
  `generation`, so each path stays individually attributable.
- `crates/agent-session-grep-adapters-sqlite/src/lib.rs:3159-3180`:
  `source_fingerprints` reads `source_scans` (content-keyed parse cache);
  `:7528-7548` upserts `source_scans` for every committed replacement;
  `:7549-7562` inserts `source_relation_scans` only when `relation_complete`,
  otherwise deletes it; `:3258-` (`source_paths_requiring_relation_scan`)
  reports scan rows missing the relation marker.

Tests added (pure additions: `git diff --numstat` reports 0 deletions):
- `crates/agent-session-grep-cli/tests/e2e.rs:3485-3518` helpers
  `search_hit_count`, `relation_scan_count`, `stored_source_fingerprint`.
- `sync_indexed_mid_file_invalid_row_scans_without_retain_or_tombstone`
  (`:3524`): indexed source rewritten to "valid row / malformed middle / valid
  row", with both previously indexed rows removed from the new file. Pins
  baseline `source_relation_scans==1` before the rewrite, then
  `retained==0`, `emitted==2`, `committed==2`, `skipped==1`, `diagnostics==2`,
  `generation==2`, both warnings (partial-retention warning first, `line 2 ...
  invalid JSON` diagnostic second), both new needles searchable once, both old
  needles still searchable (no tombstone), status `placements==4` /
  `catalog_count==8`, and `source_relation_scans==0` after the partial rescan
  (the stale complete marker is revoked).
- `sync_indexed_truncated_tail_completion_converges_without_duplicates`
  (`:3634`): Retain for an indexed EOF-truncated tail (`retained==1`,
  `emitted==0`, `committed==0`, `generation==1`, stored fingerprint unchanged,
  relation marker kept), then completion re-sync (`emitted==3`, `committed==3`,
  `skipped==0`, `generation==2`, fingerprint advanced), new needle searchable
  once, both old needles searchable once, status `placements==3` /
  `catalog_count==5`.
- `sync_new_truncated_source_completion_converges_without_duplicates`
  (`:3752`): never-indexed truncated source keeps the recoverable path
  (`retained==0`, `messages==1`, `committed==1`, `skipped==1`,
  `diagnostics==2`, valid prefix searchable, truncated message _not_ indexed,
  `source_relation_scans==0`), then completion re-sync converges
  (`messages==2`, `skipped==0`, relation marker restored) with both needles
  searchable once and status `placements==2` / `catalog_count==4`.

RED/GREEN record (every product mutation was reverted; final
`git diff --quiet -- crates/agent-session-grep-cli/src/lib.rs` exit 0):
- GREEN: the 3 new tests plus the 2 existing truncation tests each ran alone —
  5 invocations, 1 passed / 0 failed / 0 ignored each, on unmodified source.
- M1 RED (`health == JsonlHealth::Invalid` -> `health != JsonlHealth::Clean`):
  test (a) failed at `retained` (left 1, right 0) with a false truncated-tail
  warning; tests (b)/(c) unaffected. Proves (a)'s file classifies as `Partial`.
- M2 RED (`cached_fp.is_some()` -> `true`, plus the record-stream hint gate ->
  `true`): test (c) failed at `retained` (left 1, right 0, generation 0);
  (a)/(b) unaffected. Honest limit: neutralizing the `cached_fp` gate alone did
  not flip test (c), because that temp-path first sync also has no provider
  hint; only the combined mutation demonstrates the oracle.
- M3 RED (`health == JsonlHealth::Invalid` -> `false`): the new test (b) and the
  pre-existing `sync_truncated_tail_retains_previous_index_without_churn` both
  failed at `retained` (left 0, right 1) with the recoverable-skip frame;
  (a)/(c) unaffected. Proves (b)'s file classifies as `Invalid` and the new pin
  matches the existing one.
- Manual trace probe (not asserted by the tests): a rewritten mid-file fixture
  synced with `ASG_INDEX_TRACE=<temp file>` emitted `cli:sync` with
  `stages_ms.jsonl_health=0.123 > 0`, i.e. the health triage really ran before
  the Partial scan was staged.

Commands and results (env `CARGO_TARGET_DIR` set to the shared build cache,
`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, all with `--offline --locked`):
- `cargo test -p agent-session-grep-cli --test e2e <each of the 5 names>` —
  5 x (1 passed / 0 failed / 0 ignored).
- `cargo test -p agent-session-grep-cli --all-targets --all-features` —
  exit 0, 13 targets: 630 passed / 0 failed / 0 ignored (lib 361, e2e 169,
  mcp_e2e 45, provider_matrix 35, hermes_state_db 6, hook_contract 4,
  cursor_disk_kv 3, e2e_consistency 2, network_egress 2,
  provider_probe_isolation 3, three bin targets 0).
- `cargo test -p agent-session-grep-cli --doc --all-features` — 0 tests, exit 0.
- `cargo fmt --all --check` — exit 0 (rustfmt was applied once to the new test
  block only; no unrelated file changed).
- `cargo clippy -p agent-session-grep-cli --all-targets --all-features -- -D warnings`
  — exit 0.
- No product defect surfaced: zero lines of product code changed. The working
  tree holds only the `tests/e2e.rs` additions plus this task document.

Not verified / limits:
- No `sync --discover` registered-root variant of case (c): the individual
  necessity of the `cached_fp` guard for discovered sources is argued from
  `lib.rs:4842-4852` plus the combined mutation M2, not from a discovery-path
  test. Discovery incomplete/relation-recovery paths (`incomplete_paths`,
  `relation_recovery_paths`) were not exercised here.
- No independent check, no commit/push, no SHA-bound remote gates and no full
  workspace gate; those remain for main/the checker.
- Windows-only local execution; no Linux/macOS run of this slice.
- The `ASG_INDEX_TRACE` observation is a manual probe, not asserted in a test.
- The relation-marker SQL assertions pin local store state after explicit sync;
  they make no claim about multi-source batches.
