# Go/No-Go Report — agent-session-grep v0.1.0 (draft, 2026-08-16)

> Non-template draft. Conclusion is **No-Go**: local P0 gaps and external
> release gates remain open. This draft records the rehearsal evidence and the
> residual risks; it is submitted for the owner's final decision, not as a
> release authorization.

- **Date**: 2026-08-16
- **Rehearsal run ids**: Windows Round 1 (`install-windows-2026-08-16`,
  `ingest-windows-2026-08-16`, `search-windows-2026-08-16`,
  `context-windows-2026-08-16`, `resume-windows-2026-08-16`,
  `handoff-windows-2026-08-16`, `consistency-windows-2026-08-16`,
  `uninstall-windows-2026-08-16`, `reinstall-windows-2026-08-16`);
  WSL Linux side (fmt/clippy/test, release build, verify-release) recorded in
  aggregate; macOS rehearsal **not run** (external CI-billing blocker).
- **Operator**: QIN (recorded in task system)
- **Environment manifests**: `docs/release/environment-manifest.template.json`
  (template); local Windows/WSL evidence recorded in aggregate — OS/build,
  commit, and hashes only, no personal paths.

---

## 1. Privacy final check

| Check | Status | Evidence |
|---|---|---|
| Zero outbound network during supported runtime workflows | **pass (static)** | global `--offline` landed (fail-closed `capability_not_supported`); `tests/network_egress.rs` + `security-audit` step assert zero-egress default build; clean-env Wireshark capture drill still pending |
| Cross-boundary redaction (Web/Handoff/MCP/Robot) | **pass** | shared `redact.rs` engine; Round 3 redaction fix + synthetic secret fixture e2e (b1060d4); Web/JSON boundary redacted |
| Secret fixture never surfaces | **pass** | synthetic secret fixture verified; embedded-prose shapes covered |
| No transcript leak in logs/diagnostics | **pass** | diagnostic audit clean |

**Residual privacy risks**:
- Public tree/history scrub landed (P0-1 closed): `scripts/evidence/privacy_scan.py`
  scans tracked text for personal/machine absolute paths (0 findings on current
  HEAD); the history-rewrite decision
  (`docs/operations/PUBLIC-HISTORY-SCRUB.md`) remains an owner decision.
- Redaction ruleset is a conservative subset; enterprise/custom token formats
  are not covered.

---

## 2. Performance final check

| Check | Status | Evidence |
|---|---|---|
| Gate D invariants (all seven) | **pass** | v10 authorized full-corpus run `2026-08-24T17:22:18Z`: 1,900 sources / 2,222,889,692 bytes, 326,100 emitted, 0 skipped, all seven invariants pass, harness exit 0 (`docs/evidence/integration-beta/real-data-regression.md`) |

**Residual performance risks**: the Gate D correctness invariants now pass on
the full authorized corpus, including `INV-SOURCES-UNCHANGED`. Throughput and
latency benchmarking against the rehearsal corpus is still not run, so no
performance number is claimed.

---

## 3. Release materials final check

| Check | Status | Evidence |
|---|---|---|
| README ↔ quickstart consistency | pass | installer/docs sync (919b48e, 9014546) |
| Comparison table ↔ benchmark numbers | pass | `--offline`/semantic over-claims removed (e506288) |
| Demo dataset ↔ quickstart commands | pass | fixture provenance documented |
| LICENSE present and correct | pass | MIT + Apache-2.0, REUSE audit pending owner signing |
| Third-party attributions complete | **pass (factual NOTICE landed)** | `NOTICE` now ships in every release archive; approval of the reuse matrix remains owner-signed (REUSE audit Draft) |
| Fixture provenance documented | pass | synthetic gate fixtures, license + redaction recorded |
| Community health files complete | **pass** | `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1, CC BY 4.0 attribution recorded in `NOTICE`) landed at `9932df9`; GitHub community profile now resolves code_of_conduct, contributing, license, readme, pull_request_template. `issue_template` reports `null` because both templates use the newer issue-**forms** schema (`.github/ISSUE_TEMPLATE/*.yml`), which that API field does not report; both forms parse and carry name/description/labels/title/body |
| Public-tree export dry run | **pass** | `scripts/release/export_public_tree.py` on `9932df9`: 484 files exported, 0 `.trellis/` internal records (406 tracked internally, all excluded), every community-health/license file present, exported-tree privacy scan clean |

**Residual materials risks**: factual `NOTICE` + machine-readable third-party
inventory now land in every archive; the REUSE reuse-matrix approval and any
formal SBOM certification decision remain owner-signed (Draft).

---

## 4. Five-entry-point consistency

| Entry point | Status | Notes |
|---|---|---|
| CLI robot JSON | **pass** | `--output json` canonical projection |
| MCP JSON-RPC | **pass** | `search_sessions` structuredContent, same render projection |
| Robot envelope | **pass** | explicit `--robot` invocation, same canonical payload |
| Web/HTTP | **pass** | real loopback `serve` + bearer token, `GET /api/projection/search` |
| TUI automated | **pass** | headless `tui --snapshot-json <query>` shared Application projection |

**Consistency report**: `scripts/rehearsal/compare_entrypoints.py` output —
`overall_verdict: consistent`, all five entry points compared, no skipped /
aliased / unimplemented surfaces.
**Overall verdict**: **consistent** (this check).

Note: this check covers the fixed canonical search operation. Web/TUI do not
expose a comparable list-sessions projection; the harness deliberately scopes
the comparison to the search contract rather than weakening it to skipped.

---

## 5. Provider evidence

| Provider | Tier | Evidence | Notes |
|---|---|---|---|
| Claude Code | experimental | golden + byte-assert round-trip (243d7d5) | certified target, not reached |
| Codex | experimental | incremental resync/tombstone e2e (432744f) | certified target, not reached |
| 12 others | experimental | manifest entries, no external golden | `known_limitations` incomplete |
| deepseek-harness, zcode | unsupported | deferred, no transcript evidence | — |

Provider matrix `providers` reports 16 rows; PRD requires ≥5 Beta + Claude/Codex
certified — currently 0 Beta. **Not release-ready per provider gate.**

---

## 6. Defects found during rehearsal

| # | Description | Severity | Status | Fix ref |
|---|---|---|---|---|
| 1 | verify-release.py ran 5 checks but docstring claimed full flow | P0-5 | closed | this task |
| 2 | Web/TUI allowed skip-as-pass in the consistency harness | P0-5 | closed | this task |
| 3 | handoff subcommand missing (round 1) | P0 | closed | 4f98fc2 |
| 4 | serve not wired to Application ADT (round 1) | P0 | closed | 355186a + 46e4c5f |
| 5 | Web/JSON boundary leaked secrets (round 3) | P0 | closed | b1060d4 |
| 6 | handoff created_at invalid timestamp | P0 | closed | 8cc165e |
| 7 | serve token generator not CSPRNG | P0 | closed | 0630971 |
| 8 | aider declared `tool_activity: Partial` but the adapter never calls `emit_activity` and emits empty `native_id` (fail-closed anchor drop) | P1 | closed | 8704ac9 |
| 9 | antigravity declared `source_span: Unsupported` while the adapter emits byte-exact spans asserted by its golden test | P1 | closed | 7434092 |
| 10 | openclaw / tencent-codebuddy / antigravity declared `discover: Unsupported` while `provider_data_root` wires a discovery root for each — the same under-claim class as row 9, on the last unguarded capability column | P1 | closed | 68665a7 |
| 11 | Both SQLite adapters leaked their temp database copy on every probe/parse: cursor returned a bare `Connection` with no cleanup guard, and opencode returned the guard beside the connection in a tuple (which drops the guard *first*, so the unlink ran while the file was still open and Windows refused it) | P1 | closed | 2a1f062 |
| 12 | `sync --discover` hard-coded a `.jsonl` extension filter, so a SQLite-sourced provider could never be auto-discovered even with a real data root — and registering one anyway would have made a *complete* scan return zero paths and tombstone that provider's previously indexed sources | P1 | closed | this task |
| 13 | `pi` and `openclaw` share one byte-identical v3 JSONL format, so both probes returned `Confirmed` and whole-registry selection always hit the tie branch: every source under either root was rejected as `ambiguous provider selection` and could not be indexed at all — pi's own golden fixture included | P1 | closed | this task |
| 13 | pi and openclaw transcripts are the *same* v3 JSONL format with no in-content discriminator, so both adapters probe `Confirmed` on the same bytes and whole-registry selection always hit the ambiguity tie — every source under `~/.pi` or `~/.openclaw` was unindexable, including pi's own golden fixture | P1 | closed | this task |

---

## 7. Residual risk summary

1. **P0-4**: release pipeline configured but never a named successful run; CI
   billing blocks all jobs; factual NOTICE landed and ships in archives;
   REUSE reuse-matrix approval + SBOM certification decision remain owner-signed.
   Observed 2026-08-24: every workflow on the three most recent `main` heads
   (`3388344`, `0edd4e7`, `13b879c`) reports `failure` within ~3 s with an empty
   `runner_name` and zero executed steps — no runner is ever assigned, and no
   job log exists to download. That signature is account-level (Actions
   spend/billing), not a repository or code defect: on the same tree the local
   equivalents of every blocked job are green (`cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets -D warnings`,
   `cargo test --workspace` 1500 passed / 0 failed / 15 ignored, and
   `cargo deny check` advisories+bans+licenses+sources all ok).
   Re-confirmed 2026-08-25 on the newest run (`32767597854`,
   `2026-08-24T19:20:34Z`): all four cross-target jobs (Windows MSVC,
   Ubuntu 22.04 GNU x64, macOS Intel x64, macOS Apple Silicon ARM64) report
   `failure` with `runner_name: ""` and `steps: 0`, each completing 4–5 s
   after start — no step ever executes. Only the owner can lift the billing
   block, so `last_certified_targets` stays empty and the cross-target
   evidence rows stay `ci_configured_only`.
2. **Provider maturity**: 0 Beta; Claude/Codex not certified against the PRD
   gate — remains below the ≥5 Beta requirement.
3. **External**: GitHub Actions billing; PRIVATE→public switch (Option-A public
   tree exporter `scripts/release/export_public_tree.py` is ready and current-tree
   privacy gates are green), tag, GitHub Release; Authenticode/notarization/cosign;
   branch protection; ADR signing/owner decisions.
4. **macOS**: rehearsal not run (external CI-billing blocker).
5. **Known deferred**: real local semantic model **weights delivery + recall
   benchmark gate** (the optional `semantic-candle` runtime and the offline
   `model import` / `model status` path are now implemented; default builds
   stay bigram-hash / lexical-only and must not be marketed as semantic).
   Robot capability UI and ToolActivity retention/cleanup policy remain
   explicitly deferred. The current default `bigram-hash-v1` vectorizer
   remains honestly labeled fuzzy-lexical, not semantic.

Closed since this draft's original date: P0-1 privacy scrub, P0-6 bounded
ingestion, handoff determinism/budget/redaction, resume first-run preview,
`--offline` + hook provider/time filters, ToolActivity search facets + schema
v12, serve hardening, P0-5 release rehearsal (verify-release 10/10, five-entry
consistency harness all-direct).

Closed 2026-08-25 (capability-claim honesty wave, `main` at `2a32ada`): the two
capability over/under-claims in §6 rows 8–9, plus the drift-guard gap that let
them land. `capability.rs` claims were previously cross-checked only against
documentation (three doc↔matrix tests) and, for the `resume` column, against the
real command builder; `source_span` and `tool_activity` had no code-side guard at
all. `crates/agent-session-grep-cli/tests/provider_matrix.rs` now asserts both
columns against each provider's pinned golden output: a declared span capability
must match span presence in `basic.expected.json` (all 14 providers), and a
`tool_activity` claim is rejected when every pinned message carries an empty
`native_id`, since `main.rs:3517` fail-closed discards activities whose anchor is
empty. Both new tests were mutation-verified by re-introducing each original bug
and confirming a targeted failure. Note `codex` declares `tool_activity: Partial`
on adapter evidence (three `emit_activity` call sites) while its golden fixture
contains no tool records — the claim is real but fixture-unexercised, so the
anchoring test is the binding guard rather than an emission count.

Closed 2026-08-25 (`discover` column, `main` at `68665a7`): §6 row 10 plus the
last unguarded capability column. `discover` was the only column whose claims
had no code-side guard after the `source_span` / `tool_activity` wave above, and
it held three under-claims. The provider→root wiring is now a single named table
(`PROVIDER_DISCOVERY_ROOTS` in `crates/agent-session-grep-cli/src/main.rs`) and
`discover_roots_match_capability_discover_claims` asserts the equivalence in both
directions: registered in the table ⟺ `capability.rs` declares a usable
`discover` level. Mutation-verified in both directions (re-injected under-claim
and a fabricated over-claim each produced a targeted failure naming the provider).

Auditing that column also surfaced the hazard behind §6 row 12:
`discover_provider_sources` collected only `.jsonl`, while `sync_discover`
synthesizes empty tombstone batches for prior paths a *complete* scan did not
rediscover. Registering a SQLite-sourced provider without extending the filter
would therefore make one `sync --discover` scan complete-but-empty and tombstone
that provider's previously indexed sources. The wave above deliberately left
`opencode` unregistered for that reason and pinned the hazard in a test; row 12
closes the underlying gap so the capability no longer has to be withheld.

Closed 2026-08-25 (§6 row 11, `main` at `2a1f062`): auditing that same SQLite
read path surfaced a resource leak in both SQLite adapters. Each writes the
source bytes to a temp copy because rusqlite needs a path, and neither deleted it:
`cursor` returned a bare `Connection` with no guard at all, and `opencode` had a
guard but handed it back beside the connection as `let (conn, _temp_db) = ...` —
Rust drops the *later* tuple binding first, so the unlink ran while SQLite still
held the handle, Windows refused the delete, and `let _ = remove_file(...)`
discarded the error. Measured on the development machine before the fix: 2115
orphaned files, 65.9 MB, accumulating since 2026-08-16. Both adapters now return a
single `TempDb` struct whose field order (`conn` before `_guard`) makes the
sequence correct, since struct fields drop in declaration order; the load-bearing
ordering is stated at the definition. Verified by re-running both suites and
observing zero new temp files, where the previous run added one per parse. A third
defect found in the same pass: `cursor`'s `temp_db_path` documented "process id +
atomic counter" but interpolated only the counter, so two concurrent processes
would both pick `asg-cursor-0-parse.db` and `File::create` would truncate the
other's database mid-parse — the same flake `opencode` fixed for itself in
`861240e`. Each adapter now has a unit test pinning the unlink and, for cursor,
the pid scoping.

Closed 2026-08-25 (§6 rows 12–13, this task): extending `sync --discover` beyond
`.jsonl` surfaced two further defects in the same path.

Row 12 was the extension filter itself. `discover_provider_sources` matched a
hard-coded `.jsonl`, so a provider whose source is a database could never be
auto-discovered — and registering its root anyway would have been actively
destructive, because `sync_discover` synthesizes empty tombstone batches for
prior paths a *complete* scan did not rediscover, so a complete-but-empty scan
erases that provider's index. The extension is now a per-provider column in
`PROVIDER_DISCOVERY_ROOTS` returned from the same lookup as the root, so a caller
structurally cannot pair a root with the wrong extension. `opencode`
(`~/.local/share/opencode`, `db`) and `pi` (`~/.pi/agent/sessions`, `jsonl`) are
now registered on verified local evidence; `cursor` stays unregistered because
its `workspaceStorage` layout is unverified on any development machine (R4: do
not guess paths, and here guessing costs the index). Exact-extension matching
also excludes the `-wal`/`-shm` sidecars by construction, since
`Path::extension()` yields `db-wal`/`db-shm` for those. Ignoring the WAL loses
no rows: measured `session 10/10, message 261/261, part 886/886` against the
live database with and without the sidecars. Mutation-verified in both
directions — a wrong extension and an unregistered row each produced a targeted
failure, and the wrong-extension run reproduced the tombstone hazard exactly
(`complete: true` with `found: 0`).

Row 13 was found by that registration and is the more serious of the two:
`pi` and `openclaw` transcripts are the *same* format. Both are v3 session JSONL
(`{type:session,...}` header plus `{type:message,message:{role,content}}`), which
the openclaw adapter's own module documentation states outright ("the same v3
JSONL shape as the Pi adapter"). Both probes return `Confirmed` on identical
bytes and no discriminator exists in the content, so whole-registry probing
always hit the tie branch in `select_and_stage_source` and rejected the source
with `ambiguous provider selection`. Every source under `~/.pi` and
`~/.openclaw` was therefore unindexable — including pi's own golden fixture,
confirmed by running it through the CLI. This predates the discovery work: the
openclaw root was already registered, so `sync --discover` would have failed the
same way on any machine that had one. Content cannot resolve this ambiguity, so
the canonical root does: `sync --discover` now passes the root-derived provider
as a hint and staging narrows the candidate set to that provider's adapter,
while the probe still runs — identity comes from the path, format judgement
still from the bytes. The hint is deliberately unavailable to explicit
`sync <file>` / `ingest`, where no path fact exists and the tie rejection is the
honest answer; an empty candidate set is reported as "no provider recognized
this source" rather than silently falling back to the whole registry. The
regression test pins the invariant in its strongest form: identical bytes
written under both roots must each index to their own provider, as two distinct
messages in two distinct sessions. Mutation-verified by disabling the narrowing,
which reproduced the original `ambiguous provider selection` failure verbatim.

Closed by the 2026-08-17 release-gap wave (post-draft audit fixes, pushed to
`main` at `ed57a9a`): Robot v1.1 `searchData.facets` schema echo + protocol
flag-skip parity; Web/MCP canonical provider ids (`claude-code`/`codex`); root
installer scripts delegate to canonical `scripts/install/`; MCP facets echo +
`list_providers` 16-row matrix projection + Web `/api/providers` standard
envelope; gate-smoke PowerShell flag style + self-copy hazard; CI runs Python
release/evidence suites and gates release on green CI; workflow actions pinned
to SHAs; `security-audit.yml` gains `pull_request` trigger; redaction covers
fine-grained GitHub PATs + embedded AWS secret keys + MCP error frames + hook
headers; serve token compare made constant-time + `frame-ancestors 'none'` CSP;
`deny.toml` bans HTTP-client crates; spikes get standalone `[workspace]`
markers; privacy scanner drops hardcoded operator username and scans all
tracked paths (0 findings); 16-row capability-matrix drift test; tracked
generated gate manifest untracked per out/README contract; THREAT-MODEL gains
serve-LAN/hook/model-download/embedding-API attack surfaces; roadmap phase
snapshot refreshed to `NOT_READY_EXTERNAL_BLOCKERS`.

Closed by the post-release-gap wave (pushed to `main` at `f7e2a49`): optional
`semantic-candle` backend + offline `model import`/`model status` (default
build stays lexical-only, never downloads); handoff packs project catalog
`tool_activity` and authoritative `role`/`is_sidechain` facts; TUI search
facet controls (`m` sidechain / `k` tool-kind); context responses project
`tool_activities`; provider Beta readiness ledger
(`docs/product/PROVIDER-BETA-READINESS.md`) separates local from external
promotion blockers.

---

## 8. Recommendation

- [ ] **Go** — proceed with public release
- [x] **No-Go** — address residual risks first

**Rationale**: The five-entry-point consistency harness and the release
verification script are now green on the local Windows/WSL rehearsal evidence
(P0-5 closed), and the local P0/P1 feature waves have all landed (privacy
scrub, bounded ingestion, offline, resume/handoff, ToolActivity facets,
serve hardening). Release readiness is not equivalent to those checks: P0-4
(release pipeline has no named successful run and CI is blocked by billing;
SBOM/NOTICE/REUSE audit open), the provider maturity gate (0 Beta,
Claude/Codex not certified), and the macOS clean-environment rehearsal remain
open pending the external billing blocker and owner decisions. The owner
should treat this draft as the evidence summary for a No-Go decision and
re-run the affected sections after the open external items close.

---

## 9. Owner sign-off

| Field | Value |
|---|---|
| Decision | <pending — draft recommends No-Go> |
| Date | <pending> |
| Owner | QIN |
| Signature | <recorded in task system> |
