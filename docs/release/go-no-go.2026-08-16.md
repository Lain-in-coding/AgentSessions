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
| Claude Code | experimental | golden + byte-assert round-trip (243d7d5); observed `emit_activity` emissions | certified target, not reached |
| Codex | experimental | incremental resync/tombstone e2e (432744f); observed `emit_activity` emissions | certified target, not reached |
| 12 others | experimental | golden fixture + `PROVENANCE.md` (`fixture_revision=1`) each; real probe/parse/search/incremental behavior asserted per provider (see notes) | `known_limitations` non-empty for all 14 (guarded); randomized property tests still only Claude/Codex |
| deepseek-harness, zcode | unsupported | deferred, no transcript evidence | — |

All 14 implemented providers now carry executable claim-vs-behavior evidence, not
just manifest entries: every capability column has a registered behavior guard
(`CAPABILITY_BEHAVIOR_GUARDS` in `crates/agent-session-grep-cli/tests/provider_matrix.rs`),
and `every_capability_column_has_a_behavior_guard` fails if a column is added
without one. What the 12 still lack relative to Claude/Codex is randomized
property coverage, not baseline evidence.

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
| 13 | pi and openclaw transcripts are the *same* v3 JSONL format with no in-content discriminator, so both adapters probe `Confirmed` on the same bytes and whole-registry selection always hit the ambiguity tie — every source under `~/.pi` or `~/.openclaw` was unindexable, including pi's own golden fixture | P1 | closed | 82f0f26 |
| 14 | Row 13's fix only reached `sync --discover`, which carries a scanned provider identity. Explicit `sync <file>` and `ingest <file>` still probed the whole registry, so naming a pi or openclaw transcript by path — the natural first move for a new user, and the only way to index a source outside a canonical root — still failed with the same `ambiguous provider selection` error | P1 | closed | this task |
| 15 | `qoder`'s probe counted any record whose top-level `type` is `session_meta` as its own header, but every Codex rollout record is a `{timestamp, type, payload}` envelope whose first line is exactly that. Codex degrades `Confirmed`→`High` on a tolerated broken line (its own golden fixture has one), so both adapters returned `High` and a real Codex rollout with any damaged line was rejected as ambiguous rather than indexed as Codex | P1 | closed | this task |
| 16 | `codex` declared `context: Native` while its adapter hard-codes `parent_native_id: None` — Codex rollout is a linear sequence with no threading edges, so no `message_edges` row can ever exist for it and `context` had nothing to walk. Found by the guard added for it: `context` was the last column falsifiable from pinned golden output that had none | P1 | closed | this task |
| 17 | `handoff` was declared `Unsupported` for all 14 implemented providers and `incremental` was declared `Native` for two and `Unsupported` for the other twelve — but neither is a per-provider capability. `handoff_pack::generate_deterministic` reads no provider identity, and incremental judgement lives entirely in the composition root plus the store's fingerprint cache. Both columns had only a doc↔`capability.rs` consistency guard, never a claim-vs-behavior one, so twelve providers under-claimed a shipped command (`asg handoff`) and a shipped behavior (no-op resync) while two mislabeled a store-layer capability as provider-native | P1 | closed | this task |
| 18 | Three release-facing documents cited implementation evidence that nothing verified, so each could silently decay into a false claim: the MCP CONTRACT §8 tool list was pinned only against a hand-copied array in the test (editing the doc failed nothing, editing the catalog forced no doc update); ADR-0010 §1's evidence table still described golden fixtures as a Claude/Codex-only path and `AdapterManifest` as unimplemented, though all 14 providers carry goldens at `fixture_revision=1` and `manifest_for` has shipped; and this ledger's provider row still called `known_limitations` incomplete. All three now read their cited source through `include_str!` and fail on drift in either direction | P1 | closed | this task |
| 19 | The `tool_activity` guard was one-directional: it rejected a provider claiming support with no anchorable messages, but never rejected one that really emits activities while declaring `Unsupported`. Every other capability column already had both directions, so adding `emit_activity` to any of the twelve `Unsupported` adapters would have gone unnoticed — the exact asymmetry that let rows 16 and 17 through | P1 | closed | this task |
| 20 | `SECURITY.md` promised redaction of five secret shapes (AWS keys, GitHub PATs, OpenAI/Anthropic/xAI keys, Bearer tokens, PEM private keys) while the shared detector implements eleven, and pointed at the CLI applier as if it were the ruleset. An under-claimed security boundary is still a false boundary statement, and it is the first one an external researcher reads. The policy now names all eleven kinds, cites the ruleset version and the real detector path, and `security_policy_lists_every_real_redaction_kind` parses the kinds out of `redact.rs`'s production region so a new pattern cannot ship without the promise following it | P1 | closed | this task |
| 21 | Two rot classes remained in the documents an outside contributor actually follows. Every repo path cited in the root documents (NOTICE's attribution targets, SECURITY.md's detector location, CHANGELOG's guard files) could be invalidated by any rename with nothing failing, turning verifiable evidence into dead references. And RFC-0002's contract vocabulary — the §5 error labels and §6 capability tiers that `PROVIDER-ADAPTER-CONTRIBUTOR-GUIDE.md` instructs new adapters to implement — had no tie to `ProviderError` or `CapabilityLevel` at all, so renaming a variant would leave the specification describing an API that no longer exists | P1 | closed | this task |

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

Closed 2026-08-25 (§6 rows 14–15, this task): row 13's fix was incomplete, and
finding the rest of it required a guard rather than another accident.

Row 14 is that incompleteness. The hint reached staging only through
`sync --discover`, which owns a scanned path→provider table; explicit
`sync <file>` and `ingest <file>` passed `None` and still went to the whole
registry, so pointing either command at a real transcript under `~/.pi` failed
with the same `ambiguous provider selection` the discovery path had just been
taught to avoid. The earlier reasoning — that explicit invocation has no path
fact — was simply wrong: the path is right there in the argument, and
`PROVIDER_DISCOVERY_ROOTS` already knows which provider owns it. The new
`provider_for_source_path` reads that same table in reverse (path → provider),
so both entry points now resolve one file to one provider and the two cannot
diverge. Matching is per path segment after `source_path_identity` normalization,
so `.pi/agent/sessions-backup` does not match `.pi/agent/sessions`; a path that
*is* a registered root, rather than under one, is not a source and does not
match. The reverse lookup deliberately does **not** write
`source_scans.provider_id`: that column's tombstone diff means "this row came
from a complete root+extension enumeration", and populating it from an explicit
sync would let the next complete discovery scan synthesize an empty batch for,
say, a hand-renamed `foo.jsonl.bak` and erase it. Verified end to end against a
real pi transcript and a real openclaw transcript: both index under their
canonical roots, while the same bytes copied outside every registered root still
receive the honest tie rejection.

Row 15 came from the guard added alongside it. Rather than wait for the next
format twin to surface as a user-visible failure,
`ambiguous_formats_are_always_separable_by_a_registered_root` probes each
implemented provider's real golden fixture against the whole registry and
requires that either exactly one variant holds top confidence, or every tied
claimant has a registered canonical root. On its first run it failed on the
codex fixture, claimed jointly by `codex` and `qoder`: qoder counted any record
whose top-level `type` is `session_meta` as its own header, and every Codex
rollout line is a `{timestamp, type, payload}` envelope whose first line is
exactly that. Codex's own fixture contains a deliberately broken line that
degrades it from `Confirmed` to `High`, qoder also reported `High`, and the tie
branch would have rejected the source. Since `qoder` has no registered
discovery root, no path fact could have rescued it — a genuine shippable defect,
found by the guard within seconds of its existence. The discriminator is
structural: a Qoder header carries identity at the top level or in a
`session_meta` object and never has `payload`, so a `session_meta` record
carrying `payload` is now counted as a Codex envelope and recorded in
`unmatched_evidence`. The paired test
`pi_and_openclaw_fixtures_are_mutually_indistinguishable_by_content` anchors the
premise the root-based fix rests on, and will fail if either format ever gains a
discriminator — which would be welcome, but must be reflected in the reasoning
that justifies the narrowing. A coverage test keeps `TWIN_FIXTURES` in step with
the capability matrix, so a new provider cannot be added without being checked
against the existing ones; whole-file SQLite sources are excluded by name, since
their probes key on magic bytes and table structure rather than competing for
the same JSONL records.

Closed 2026-08-25 (§6 row 16, this task): `context` was the last capability
column falsifiable from pinned golden output that still had no guard tying its
claim to that output, and adding one immediately found an over-claim.
`capability_context_claim_matches_pinned_golden_parent_links`
parses each implemented provider's pinned golden `expected.json` and requires
that a `context: Native` claim be backed by at least one message carrying a
`parent_native_id` — because a context graph is assembled by walking
`message_edges`, and no code path synthesizes sequence-based edges. On its first
run it failed on `codex`: the adapter hard-codes `parent_native_id: None`
(`crates/agent-session-grep-provider-codex/src/lib.rs:708`, asserted at
`lib.rs:1018`), and its module doc already said why — a Codex rollout is a linear
sequence that publishes no explicit threading edge, so inventing one would be
dishonest. The golden confirms it: codex's three messages all carry an empty
parent, while claude-code's carries a real parent on four of five. So the claim
was wrong, not the test, and codex's `context` is now `Unsupported` in
`capability.rs`, in the Beta-readiness ledger, and in the maturity matrix. The
same read surfaced the mirror-image under-claim in the matrix prose, which
described `context` as "unsupported 或 unknown" across the board while
claude-code genuinely supports it; that line now states the per-provider truth.
Mutation-verified in both directions: restoring `Native` for codex re-fails the
guard, and deleting claude-code's parent links fails it too.

Closed 2026-08-25 (§6 row 17, `main` at `5034838` and `2d23ca3`): auditing the
sentence above — "the last column falsifiable from pinned golden output" — showed
it was true only in its narrow reading. Two columns, `handoff` and `incremental`,
had no claim-vs-behavior guard of any kind; they were covered only by
doc↔`capability.rs` consistency checks, which keep three files agreeing with each
other and say nothing about whether the agreed value is true. Both turned out to
be under-claims of the same shape: neither capability is per-provider at all.

`handoff`'s generator (`application::handoff_pack::generate_deterministic`)
consumes only `SearchHit` plus authoritative source placement; it never reads a
provider identity, and it sets `provenance: None` and
`matched_sessions[].provider_id: None` on purpose, because a search-shaped pack
spans providers and inventing a single origin would be dishonest. So "can this
provider produce a pack with real evidence" reduces to "is the message indexed
with a placement" — i.e. `parse` works. The blanket `Unsupported` was checked
against reality by running `asg handoff` over three structurally unrelated real
goldens (codex JSONL, aider markdown whose native ids are always empty, opencode
SQLite); all three returned `confidence: high` with real evidence.
`handoff_pack_generation_is_provider_independent` now runs every implemented
provider id through the generator and requires the assembled pack to be
field-identical, so the moment a per-provider branch appears the test fails.

`incremental` is the same story one layer down: the decision lives entirely in
the composition root and the store. `sync` reads the cached
`(len_bytes, fingerprint)` from `source_scans`, compares it against the fresh
snapshot's BLAKE3 fingerprint, and on a match skips parse outright (reporting
`unchanged` = stored message count); `commit_source_batches_if_changed` then does
content-level no-op detection and leaves `generation` alone. No adapter
participates and no per-provider branch exists.
`capability_incremental_claim_matches_real_resync_for_every_provider` syncs each
provider's pinned golden *source* twice — staging pi and openclaw under their
canonical roots, since the two are format twins and identity comes from the
registered root — and requires the second sync to be `committed=0` /
`unchanged=N` / unchanged generation. On its first run it failed on `aider`,
which claimed `Unsupported`; the measured resync was a clean no-op, and the same
held for all twelve providers marked `Unsupported`.

Both columns are therefore now `Derived` for all 14 implemented providers, and
`Derived` rather than `Native` deliberately: the behavior is derived from
indexed content and source bytes, not natively provided by the agent's format.
That also corrected an over-claim hiding inside the under-claim — claude-code and
codex had `incremental: Native`, which recorded a store-layer capability as a
provider-native one. Their dedicated append/shrink/empty-source tombstone resync
e2e tests remain, and the matrix now says plainly that those represent deeper
test coverage, not a higher capability tier. Mutation-verified in both
directions on the incremental guard: restoring `Unsupported` for aider fails it
(exit 101), and downgrading a row to `Unknown` fails it too, with `capability.rs`
restored byte-identical (hash-checked) after each.

Closed 2026-08-25 (§6 row 18, `main` at `5cc6f4d` through `f1214cd`): with row 17
closed, the remaining question was not "which column is wrong" but "which claim
could go wrong without failing anything". Three columns — `probe`, `parse`,
`search` — were `Native` across all 14 providers with no behavioral guard, so
`capability_probe_claim_matches_real_probe_on_own_golden`,
`capability_parse_claim_matches_real_parse_on_own_golden`, and
`capability_search_claim_matches_real_retrieval_for_every_provider` now run each
adapter against its own golden bytes. Probe requires `Ok`, a non-`Ambiguous`
confidence (RFC-0002 §3 makes ambiguity a refusal, so an ambiguous probe is a
probe that failed), a `variant_id` matching `capability.rs` verbatim — a probe
reporting a different variant means registry selection would mount the wrong
adapter — and non-empty `matched_evidence`. Parse requires `Ok`, `committed > 0`,
`sink.messages.len() == report.committed`, and non-empty text on every message,
since a message counted as committed but carrying nothing would claim to have
indexed something unretrievable. Search queries a token taken from each
provider's own golden text rather than a fixed keyword, which would otherwise
fail on the non-English fixtures. All three claims held; each was
mutation-verified by faking an `Unsupported` on aider and watching the guard
report the real contradicting behavior.

Two structural gaps closed alongside them. First, the discipline itself was
unenforceable: nothing stopped a fifteenth capability column from shipping with a
declaration and no behavioral guard, which is exactly how rows 16 and 17 became
possible. `every_capability_column_has_a_behavior_guard` parses the
`CapabilityLevel` fields out of `capability.rs` source (rather than a hand-copied
list, which would drift the same way) and requires each one to appear in
`CAPABILITY_BEHAVIOR_GUARDS` — column, guard test name, and the `include_str!`'d
file that must actually contain it. A new column now fails the build until it has
executable counter-evidence. Second, the `tool_activity` guard was one-directional:
it caught a provider claiming support it could not anchor, but would never fail a
provider that really emits activities while declaring `Unsupported` — the precise
asymmetry behind rows 16 and 17.
`capability_tool_activity_unsupported_claim_is_not_an_under_claim` closes it by
parsing each `Unsupported` provider's own golden and requiring zero anchorable
activities; mutating claude-code (a real emitter) to `Unsupported` now fails.
`resume`, checked during the same sweep, was already bidirectional — it asserts
`(resume == Derived) == builder_supports`, an iff that fails either way.

The same audit found three documentation claims that had drifted stale rather
than wrong-at-birth, all in the over-cautious direction. ADR-0010 §1's evidence
table still described golden fixtures as a Claude/Codex-only path and
`AdapterManifest` as unimplemented, when all 14 providers carry goldens with
`fixture_revision=1` and shipped manifests declaring non-empty
`known_limitations`; §5 of this report described the other twelve as having "no
external golden" and incomplete limitations for the same reason. Both now state
the verified position. Because that ADR cites specific test and constant names as
its evidence, `adr_0010_cited_evidence_exists_in_source_and_is_actually_cited`
now checks both directions — each cited name must exist in the source file it
claims, and must still be cited by the ADR — so a rename cannot silently turn a
governance record into an assertion. The same treatment went to the MCP tool
catalog, whose nine-tool contract had been pinned to a hand-copied array:
`contract_declared_mcp_tools_match_the_real_catalog` parses the CONTRACT document
itself and compares it against the live registry.

Continuing the same sweep outward from `capability.rs` to every release-facing
document, four more hand-written claims were tied to their authoritative source.
The README's Format column describes each provider's real source format, which
is exactly the sort of prose that decays when an adapter changes storage:
`readme_provider_format_column_matches_manifest_source_consumption` classifies
each row as whole-source or record-stream from its wording and requires it to
agree with that provider's `AdapterManifest.streaming_support`, the value
`manifest.rs` derives from the format itself. Describing OpenCode (SQLite) as
JSONL or Kimi (JSONL) as SQLite now fails. The README's eleventh line claims the
planned version matches the Cargo workspace version, and its "honest maturity"
bullet lists the grading tiers; `readme_release_status_and_maturity_tiers_match_authoritative_sources`
reads the version out of `[workspace.package]` and the tier names out of
`ProviderMaturity::as_str`, so a release bump that misses the README, a dropped
tier, or a fabricated one like "stable" all fail. The CHANGELOG's `[Unreleased]`
section names all 16 providers and both counts;
`changelog_provider_claims_match_capability_matrix` derives those from the matrix
and requires every implemented id to be named and both deferred ids to be marked
deferred. Seven documents are now read through `include_str!` and fail on drift:
the maturity matrix, the Beta-readiness ledger, README, CHANGELOG, the MCP
CONTRACT, ADR-0010, and `capability.rs` itself as the authority they answer to.

Local gate green throughout, ending at 1511 tests, with
`cargo fmt --all --check` and
`cargo clippy --workspace --all-targets -D warnings` both clean.

Closed 2026-08-25 (§6 rows 19–20, `main` at `f1214cd` and `8cfd95e`): auditing
the guards themselves rather than the claims turned up one that was only half a
guard. Every capability column was checked in both directions except
`tool_activity`, which rejected a provider claiming support it could not anchor
but would have said nothing about a provider that really emits activities while
declaring `Unsupported` — the same one-way asymmetry that produced rows 16 and
17. `capability_tool_activity_unsupported_claim_is_not_an_under_claim` now parses
each `Unsupported` provider's own golden through a `CapturingSink` and requires
zero anchorable activities; mutating claude-code, a real emitter, to
`Unsupported` fails it. The current claims were already honest — only claude-code
and codex call `emit_activity`, and only they declare `Partial` — so this closes
a latent gap rather than a live defect. `resume`, checked in the same pass, was
already bidirectional: it asserts `(resume == Derived) == builder_supports`, an
iff that fails either way.

The same audit found the sweep's one genuinely outward-facing under-claim.
SECURITY.md's boundary table named five secret families (AWS keys, GitHub PATs,
OpenAI/Anthropic/xAI keys, Bearer tokens, PEM private keys) while the shared
detector in `crates/agent-session-grep-ports/src/redact.rs` implements eleven,
having gained GitLab PATs, Slack tokens, Google API keys, Stripe keys, bare JWTs,
and a separate AWS secret-key shape since the policy was written. Under-claiming
a security control is not the safe direction it looks like: a researcher reading
that table decides what to treat as protected, and the file reference pointed at
the CLI applier rather than the detector, so following it would not have shown
the real rules either. The table now names all eleven kinds and cites both files
plus ruleset `v1.1`, and `security_policy_lists_every_real_redaction_kind`
parses the `[redacted:<kind>]` markers out of the detector's production region —
not a hand-copied list — requiring every kind to appear in the policy, no
fabricated kind to appear, and the cited ruleset version to match the constant.
Adding a pattern without updating the policy now fails the build. One rebuild
hazard is worth recording: cargo does not treat `include_str!` targets as
dependencies, so editing a guarded document without touching the including source
can leave a stale snapshot compiled into the test binary — the guard reported a
missing `v1.1` that was present on disk until the source was touched. Any future
document guard should be re-run after a `touch` of its host file before its
result is believed. Local gate green at 1512 tests.

Closed 2026-08-25 (§6 row 21, `main` at `b1686a1` and `a8a7c76`): the last two
rot classes were about references rather than claims. Root documents cite repo
paths in backticks as the evidence a reader is supposed to follow — NOTICE points
at the file retaining an upstream copyright line, SECURITY.md at the detector,
the CHANGELOG at the test enforcing a guarantee — and a refactor that moves any
of them silently turns a verifiable citation into a dead end with nothing failing.
`root_docs_cited_repo_paths_all_resolve` extracts every backticked fragment that
contains a slash and ends in a source or document extension from the six root
documents, then requires it to resolve either at the repository root or under
some crate (the CHANGELOG writes `tests/network_egress.rs` crate-relative).
Fragments that look like paths but are not — the `handoff-pack/v1` schema name,
the `intfloat/multilingual-e5-small` model id, the ACP `session/update` method,
gitignored files, and release-time generated inventories — sit in an explicit
`NON_PATH_IDENTIFIERS` list with a stated reason each, rather than being guessed
at by a regex. Sixteen citations are checked; inventing one that does not exist
fails.

The second is the contract vocabulary itself. RFC-0002 §5 grades failures with
snake_case labels (`source_changed_during_read`, `structural_fatal`,
`ambiguous_variant`) and §6 lists the field capability tiers, and the contributor
guide instructs new adapter authors to report exactly those labels — but nothing
connected either list to the code. Renaming a `ProviderError` variant, or writing
a label the enum never had, would have left the specification intact and wrong.
`rfc_0002_error_vocabulary_maps_to_real_provider_error_variants` requires each
registered label to still appear in the RFC and its variant to exist in the
production region of `ports/src/lib.rs`, and requires every `CapabilityLevel`
tier to appear in §6's vocabulary. The two labels that are handling strategies
rather than error types (`record_recoverable`, which lands in
`ParseReport.skipped`, and `incomplete_tail`, which is a caller obligation not to
commit a partial tail) are documented as deliberately out of the table.
Mutation-verified in three directions each. Local gate green at 1514 unique
tests (1548 as reported, since the CLI's unit tests run once per bin and the
crate builds two).

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

Closed 2026-08-25 (release-hardening wave, `main` at `8f5a6b1`): three code
review Mediums closed — robot/JSON error envelopes now run the shared
redaction engine and `ProviderError::Io` masks source paths per the R4.3
rule; the entry-point consistency e2e skips when no Python interpreter
exists instead of panicking; CI gains a dedicated
`--features semantic-candle` test step so the candle module is compiled and
tested on every run. Seeded randomized property suites
(`tests/properties.rs`) now cover **all 14 implemented providers**,
mirroring the previous claude-code/codex-only coverage — §5's "randomized
property tests still only Claude/Codex" statement is superseded; the Beta
readiness ledger gains a `property` column guarded in both directions
against each suite's existence. The resume command matrix was extended with
upstream-evidenced commands for `antigravity` (`agy --conversation <id>`),
`opencode` (`opencode <directory> --session <id>`), `kimi-code`
(`kimi --session <id>`), and `tencent-codebuddy`
(`codebuddy --resume <id>`); `hermes`/`qoder`/`cursor` stay unknown with
recorded reasons. A seven-provider tool_activity honesty review found four
formats carrying structured tool records but no per-message native id to
anchor and three carrying none — all seven honestly stay `Unsupported`,
pinned by golden-corpus drift tests. Local gate green throughout,
`verify-release.py` 10/10, `cargo deny` advisories/bans/licenses/sources all
ok. §5's provider-evidence table and §7.2's 0-Beta statement remain
historical records of the 08-16 state; current per-provider status is
tracked in `docs/product/PROVIDER-BETA-READINESS.md`.

Closed 2026-08-26 (borrowed-feature waves, `main` at `c4b2a54`): seven
competitor-borrowed improvements landed, each with failure-test-first
coverage and the local gate green after every merge. Lexical rank signals
(30-day exponential recency decay with a 0.3 floor, fixed sidechain score
penalty, injected-clock determinism — `application::ranking`); pseudo-user
noise filtering in the claude-code/codex parse layers (envelope-shape
whitelist only, twelve formats pinned for verbatim passthrough) plus
string-shaped Codex content parsing; `list_sessions` Peek previews charged
to the response byte gate; session display titles derived via
custom-title > ai-title > first-user chain (schema v13) with the parse-layer
noise filter feeding the derivation; CJK single-character query recall via
unigram FTS tokens (no schema change; ADR-0007 updated); parser-semantic
versioning (schema v14) so parse upgrades force a targeted backfill of
unchanged sources instead of silently retaining stale projections; bounded
FTS projections (16,000 chars per message, schema-consistent across the
write/rebuild/current paths); sidechain parent edges now classify as
`Subagent` relations instead of `Reply`; token usage tracking (schema v15)
extracting observed usage facts for claude-code and delta-derived Codex
totals (Recall 98% stale-regression rule), with seven formats recording
their evidence and staying honestly unsupported; and privacy-safe repo
slugs (schema v16, `--repo` filter + `status` aggregation, git-detection
failures degrade to null). Privacy scan stays at 0 findings,
`verify-release.py` 10/10 against a fresh release build, and
`cargo deny` advisories/bans/licenses/sources all ok (`cargo audit` is not
runnable from this network — the RustSec advisory database fetch fails —
so deny remains the advisory gate). The three gates this wave cannot close
remain exactly the ones named in §7.2/§7.3: CI billing, owner sign-off, and
provider Beta promotion (all 14 stay Experimental with the ledger recording
the remaining local gaps).

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
