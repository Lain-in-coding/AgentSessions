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
