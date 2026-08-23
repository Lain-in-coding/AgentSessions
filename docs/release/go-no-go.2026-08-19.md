# Go/No-Go Report — agent-session-grep v0.2.0 (release-gate verification, 2026-08-19)

> Release-gate verification record for plan item M5-1: gates G1–G15 verified by
> execution. **In progress — verdicts are appended as each gate is executed.**
> Every verdict below cites the command that was run and its actual output. A
> gate that could not be executed here is recorded `not_verified` or `blocked`
> with the precondition, never `pass` by inference.

- **Date**: 2026-08-19
- **Rehearsal run ids**: (filled per gate below)
- **Operator**: QIN (recorded in task system)
- **Environment manifests**: (filled per gate below)

## Status vocabulary

Per `docs/operations/core-beta-evidence-matrix.md`:

- `locally_verified` — executed evidence present in this repository, naming the
  environment used. **Every green gate below is at most this.**
- `ci_verified` — a named successful run of that exact CI job on a
  GitHub-hosted runner. **No gate below is `ci_verified`;** CI is blocked by
  billing and no local run may be promoted.
- `not_verified` — not executed here; precondition stated.
- `blocked` — the required host, credential, network capture or repository
  visibility is unavailable.

---

## 0. Environment

| Field | Value |
|---|---|
| OS | Windows 11 (10.0.22631), x86_64 |
| Branch | audit branch tip (fast-forwarded before the rehearsal) |
| Verified commit | `d27e8d4` (audit tip) + two commits made during this run |
| Rust binary | `target/release/asg.exe`, default features (no `semantic-candle`) |
| Python | 3.10.11 |
| Isolation | `HOME` / `USERPROFILE` / `LOCALAPPDATA` redirected to a scratch tree per run; every data command run without `--db` so the default `<data-dir>/asg.db` path is exercised |

No gate below is `ci_verified`. Every green gate is `locally_verified` on the
single Windows host above; Linux/WSL and macOS are covered separately under G14.

---

## 1. Gate results (G1–G15)

| Gate | Subject | Verdict | Status word |
|---|---|---|---|
| G1 | Claude Code + Codex verified on real corpus to the D9 standard, each with a synthetic golden fixture in-repo | partial — golden side executed, real-corpus side not executable here | `locally_verified` (golden) + `not_verified` (real corpus) |
| G2 | ≥5 providers at D9 promoted to Beta; rest honestly labelled | **fail** | — |
| G3 | Four gated actions meet thresholds on the 100k synthetic corpus, thresholds bound into automation | see §2 | mixed |
| G4 | First full index of 1 GiB ≤ 5 min | see §2 | `locally_verified` at real density |
| G5 | Seven Gate D invariants green on the real frozen snapshot | `not_verified` | — |
| G6 | Five-entry-point consistency harness `overall_verdict: consistent` | **pass** | `locally_verified` |
| G7 | Zero-egress static audit; `--offline` fail-closed | **pass** | `locally_verified` |
| G8 | Cross-boundary redaction; synthetic secret fixture never surfaces | **pass** | `locally_verified` |
| G9 | Data lifecycle usable across all three dimensions, with tests | **pass** | `locally_verified` |
| G10 | Public-tree `public`-profile privacy scan, 0 findings | **fail** | — |
| G11 | Documentation completeness | see §4 | partial |
| G12 | In-product UX | **fail** | — |
| G13 | No stale or self-contradictory claims | **fail** | — |
| G14 | Windows + Linux fully verified; macOS honestly labelled | partial — Windows only here | `locally_verified` (Windows) |
| G15 | All quality gates green | **fail** (1 of 7 commands exits non-zero) | — |

Detail and evidence for each gate follows.

### G1 — Claude Code + Codex, D9 standard

D9 = real-transcript parse is lossless (session / message / parent chain /
timestamp / role all correct) + gaps recorded in `known_limitations` + a
synthetic golden fixture committed to guard against regression.

Golden-fixture half, executed:

```
cargo test --workspace
```

Output: 60 test binaries, **936 passed, 0 failed**, exit 0. Golden fixtures
exist for 14 of 14 implemented providers under
`crates/agent-session-grep-provider-*/tests/golden/` (claude 3 files, codex 3
files; inventory taken by directory listing).

Real-corpus half: **`not_verified`**. The D9 real-transcript run is
`real_data_regression.py` against the owner's frozen ~1.18 GB snapshot. That
snapshot is not present in this worktree and provider transcripts are read-only
and must never be copied here, so the run cannot be reproduced. See G5.

Verdict: the in-repo half is `locally_verified`; the real-corpus half is
`not_verified`. **G1 is therefore not fully verified.**

### G2 — five providers at Beta

```
target/release/asg.exe providers        # 16 rows
```

Output, maturity column: **14 `experimental`, 2 `unsupported`, 0 `beta`,
0 `ga`, 0 `certified`**. The `target` column declares Beta for 9 providers and
Certified for `claude-code` / `codex`, but a target is a roadmap value, not a
current state.

This is not a bookkeeping lag — it is enforced. `crates/agent-session-grep-ports/src/capability.rs`
carries a test named `implemented_providers_are_experimental` that asserts every
non-`unsupported` row equals `Experimental`. Promoting any provider to Beta
requires deleting or rewriting that assertion, which is a deliberate decision
with its own evidence bar.

Verdict: **fail**. 0 of the required 5. Honest labelling is satisfied; the
promotion count is not.

### G6 — five-entry-point consistency

```
python scripts/rehearsal/compare_entrypoints.py --binary target/release/asg.exe \
  --out evidence-output/g6-entrypoints.json
```

Exit 0. Report contents:

- `overall_verdict: consistent`
- `entry_points.declared` = `[cli, mcp, robot, web, tui]`;
  `implemented` = the same five; `pending` = `[]`
- two operations compared — `search` and `recent_sessions` — each
  `verdict: consistent`, with `skipped: []`, `unimplemented: []`,
  `divergences: []`
- the Web leg is a real loopback `serve` request, and the TUI leg a headless
  `tui --snapshot-json`; neither is a skip-as-pass

Verdict: **pass**, `locally_verified`. (The `recent_sessions` operation also
answers G17 across all five entry points.)

### G7 — zero egress and `--offline`

```
cargo test -p agent-session-grep-cli --test network_egress
```

Output:

```
test default_build_has_no_http_client_dependency ... ok
test only_socket_is_serve_loopback_tcp_listener ... ok
test result: ok. 2 passed; 0 failed
```

Exit 0. This is a **static** audit: it asserts no HTTP-client crate is in the
default dependency graph and that the only socket construction in the tree is
`serve`'s loopback listener. A clean-environment packet capture has still never
been run — recorded as a residual risk, not folded into this verdict.

`--offline` is an inert standing gate: no capability currently in the product
needs the network, so the flag has nothing to refuse today. It is a guard
against a future capability, and its value is that it fails closed rather than
degrading silently.

Verdict: **pass** for the stated static claim, `locally_verified`.

### G8 — cross-boundary redaction

Two kinds of evidence, both executed.

Unit and e2e:

```
cargo test -p agent-session-grep-ports redact          # 16 passed
cargo test -p agent-session-grep-cli --bin asg -- redact   # 20 passed
cargo test -p agent-session-grep-cli --test e2e -- handoff_redacts   # 1 passed
cargo test -p agent-session-grep-cli --bin asg -- integration_unsupported_post_echoes  # 1 passed
```

End-to-end boundary probe, written for this verification and run against a
synthetic-secret corpus (four hand-authored secret shapes with no valid
checksum; no provider transcript involved), in an isolated `HOME`:

| Boundary | Command | Result |
|---|---|---|
| CLI robot JSON | `search <probe> --output json` | no leak |
| CLI human | `search <probe>` | no leak |
| Robot envelope | `--robot search <probe>` | no leak |
| MCP JSON-RPC | `tools/call search_sessions` + `get_session_context` | no leak |
| Handoff pack | `handoff <ses> --output json` | no leak |
| Session context | `context <ses> --output json` | no leak |
| Resume projection | `get-session-resume <ses> --output json` | no leak |
| Web / HTTP | `GET /api/projection/search` over real loopback `serve` + bearer token | no leak |
| TUI automated | `tui --snapshot-json <probe>` | no leak |

`boundaries checked: 9; leaking: 0`. The raw catalog projection
(`--robot list`) shows all four shapes replaced in the stored text:
`[redacted:api_key]`, `[redacted:aws_access_key]`, `[redacted:github_token]`,
`[redacted:bearer_token]`, with the envelope reporting
`redaction: {mode: default, redacted_count: 2, ruleset_version: v1.0, status: applied}`.

Verdict: **pass**, `locally_verified`.

Honest caveat found while probing, recorded rather than smoothed over: the first
run of this probe used a **malformed** AWS access key (16 characters instead of
the required 20) and it passed through unredacted at the Robot and Web
boundaries. That is the documented behaviour of a deliberately conservative
ruleset — it matches only high-confidence structured shapes and declines
near-misses to avoid false positives — but it means the ruleset is
shape-exact, not fuzzy. A truncated, prefixed or otherwise reshaped credential
is not covered. This is a residual risk, not a defect against G8's wording.

### G9 — data lifecycle, three dimensions

All three deletion dimensions exist as real subcommands:

- **by session / by project**: `forget <ses-id>`, `forget --project <dir>`,
  plus `forget --list` / `forget --readmit` for the suppression list
- **by time**: `prune --before <YYYY-MM-DD>`
- **by provider**: `prune --provider <id>`

```
cargo test -p agent-session-grep-cli --test e2e -- forget prune
```

Output: 6 passed, 0 failed —
`forget_session_is_dry_run_until_yes_and_then_removes_the_content`,
`forget_project_matches_the_directory_and_its_children_only`,
`forget_and_prune_reject_malformed_scopes`,
`forget_human_output_names_the_sessions_and_the_undo_path`,
`prune_before_removes_only_older_sessions`,
`prune_provider_removes_only_that_providers_sessions`.

```
cargo test -p agent-session-grep-cli --test e2e -- after_deletion
```

Output: `after_deletion_doctor_is_consistent_and_rebuild_does_not_resurrect
... ok`. This is the important one: it asserts that after `forget --yes`,
`index compact` succeeds, `doctor` reports a consistent store, and
`index rebuild` — which reprojects from the authoritative catalog — does **not**
resurrect the deleted text, while an unrelated session survives.

The `index compact` requirement is stated in the product, not just the plan:
`index --help` says FTS5 deletion only writes a delete marker, so deleted terms
remain readable in the database file until `compact` rewrites it, and that
`compact` is deliberately not folded into `sync` because it takes an exclusive
lock and needs temp space roughly the size of the store. `forget`'s human
output also names it.

Provider source transcripts are never modified: `forget --help` states the
source files stay read-only and deletion only affects the index.

Verdict: **pass**, `locally_verified`.

### G10 — public-tree privacy scan

```
python scripts/release/export_public_tree.py --repo . \
  --destination evidence-output/public-tree \
  --manifest evidence-output/public-tree-MANIFEST.json
```

Exit 1. Output:

```
CONTRIBUTING.md:64: [internal-tracker] .trellis
README.md:157: [internal-tracker] .trellis
```

Both are the same regression, introduced by the language-policy text: the
sentence explaining which existing Chinese text is not mass-translated names the
internal task-tracker directory as one of the places it lives. The exporter's
own `public` profile rejects internal task-tracker references, so the export
aborts and no clean public tree can be produced from this commit.

Verdict: **fail**. Precondition to clear: remove the two `.trellis` mentions
from `README.md` and `CONTRIBUTING.md` (the sentences work without naming the
directory), then re-run the exporter to exit 0.

Separately, this verification found and fixed a **new** instance of the M0-4
defect class: this very report, `docs/release/go-no-go.2026-08-19.md`, was being
copied into the exported public tree because the exporter excludes internal
release-decision drafts by exact path and had never heard of it. Confirmed by
listing the exported `docs/release/` directory before and after. Fixed in this
branch; after the fix the exported directory contains only
`environment-manifest.template.json` and `rehearsal-runbook.md`.

---

## 2. Performance final check (G3 and G4)

Both gates read the same harness. Two corpora were generated and each was
measured twice, per the plan's "two consistent runs" rule.

| Corpus | bytes/message | `fixture_hash` matches frozen manifest | Harness verdict |
|---|---:|---|---|
| Frozen gated corpus, `--body-scale 1.0` | 509 | yes (`65cc08e9…`) | scored |
| Real-density corpus, `--body-scale 25.66` | 7,002 | no, by design (`bb9f31f9…`) | `gate.pass = null` |

Corpus generation and integrity:

```
python scripts/evidence/synthetic_corpus.py generate --output-dir evidence-output/synthetic-corpus-100k
python scripts/evidence/synthetic_corpus.py verify   --output-dir evidence-output/synthetic-corpus-100k
```

`verify` exit 0, with `fixture_hash`, `manifest_fixture_hash` and
`frozen_manifest_fixture_hash` all equal to
`65cc08e90dbf7fd406c96ee51cb658d3c36a909ca5c229a308da0b58b4d8c457` and
`checked_against_frozen_manifest: true`. The gated corpus is bit-reproducible on
this host.

### Measured results

```
python scripts/evidence/performance_gate_benchmark.py run \
  --binary target/release/asg.exe \
  --corpus-dir evidence-output/synthetic-corpus-100k --output-dir evidence-output/g3-perf
python scripts/evidence/performance_gate_benchmark.py run \
  --binary target/release/asg.exe \
  --corpus-dir evidence-output/synthetic-corpus-dense --output-dir evidence-output/g4-dense --skip-embeddings
```

| Metric | Threshold | Gated corpus run 1 / run 2 | Real-density run 1 / run 2 |
|---|---|---|---|
| `initial_index_throughput_mib_s` | ≥ 3.5 | **1.324 / 1.574 — FAIL** | **7.432 / 7.638 — over threshold, but not scored** |
| `noop_sync_latency_ms` | ≤ 1000 | 836.2 / 693.5 — pass | 769.0 / 796.6 — over-threshold: no |
| `search_latency_p95_ms` | ≤ 50 | 37.5 / 31.4 — pass | **114.6 / 118.3 — 2.3× over threshold** |
| `mcp_single_call_latency_p95_ms` | ≤ 50 | 18.0 / 16.4 — pass | **82.1 / 88.2 — 1.7× over threshold** |
| `initial_index_messages_per_s` | (informational) | 2,726 / 3,240 | 1,113 / 1,144 |
| `store_size_ratio` | (informational) | 12.63 / 9.27 | 3.553 / 3.553 |
| `cli_process_overhead_p50_ms` | (informational) | 23.3 / 22.1 | 21.1 / 23.4 |

Harness verdict lines, verbatim from the manifests:

- gated corpus, both runs:
  `{"pass": false, "failures": ["initial_index_throughput_mib_s"], "deferred": []}`
- real-density corpus, both runs:
  `{"pass": null, "failures": [], "deferred": ["initial_index_throughput_mib_s",
  "mcp_single_call_latency_p95_ms", "noop_sync_latency_ms",
  "search_latency_p95_ms"]}`, every metric carrying
  `state: off_frozen_corpus_contract`

### G3 verdict — **fail**

Three of the four thresholds pass on the frozen gated corpus; the fourth,
`initial_index_throughput_mib_s`, fails at 1.324 and 1.574 MiB/s against 3.5,
reproducibly across two runs. `gate.pass = false` is the harness's own verdict,
not an interpretation. The second half of G3 — "thresholds bound into
automation" — is satisfied: the thresholds live in
`PERFORMANCE_THRESHOLDS` in the harness, each with a recorded origin, and the
validator refuses a manifest that claims threshold scale while carrying a
non-frozen `body_scale`, so the density knob cannot be used to launder a pass.

Note on the sibling correctness gate that G3's evidence column names:

```
python scripts/evidence/open_source_gate_benchmark.py run --binary target/release/asg.exe \
  --output-dir evidence-output/g3-correctness
```

Exit 0, `gate: {"pass": true, "failures": [], "deferred": []}`, with
`lexical_recall_at_10` 1.0 (≥ 0.95), `parse_loss_ratio` 0.0 (≤ 0.05),
`discovery_coverage` 1.0 (≥ 0.95) and `resume_handoff_success` 1.0 (= 1.0). That
gate carries no performance threshold at all, so it does not answer G3's own
wording ("four actions on the 100,000-message corpus"); it is recorded here
because the plan's evidence column names it. Its `discovery_coverage` figure
also still rests on a fixture set seeded for `claude` and `codex` only, so it
cannot fail because of the other twelve adapters.

### G4 verdict — **`locally_verified` at real density; the gated-corpus number does not answer this gate**

G4 asks whether a first full index of 1 GiB completes within 5 minutes. Both
numbers, and which one answers the question:

- **Real transcript density (7,002 bytes/message, 2.4% off the 7,174 measured on
  a real corpus): 7.432 and 7.638 MiB/s → 1 GiB in 2.23–2.30 minutes.**
  This is the number that answers G4, because 1 GiB of real transcript is about
  150,000 messages, not the 2.1 million that 1 GiB of the gated corpus would be.
- Gated synthetic density (509 bytes/message): 1.324 and 1.574 MiB/s → 1 GiB in
  10.85–12.89 minutes. Same binary, same host. The entire difference is corpus
  density: cost was measured to be roughly half per-message and half per-byte, so
  a corpus with 14× more messages per byte does 14× more per-message work for the
  same byte count.

So: the requirement is met at the density real users have, with roughly 2.2×
headroom, and it is missed by 2.5–3.4× on a corpus that is not representative of
real input. **This is not a clean pass and it is not a blocking fail.** It is a
measurement whose verdict depends on a corpus property that the gated corpus gets
wrong, and the gated corpus is what the automation scores.

Two constraints on how far this evidence can be taken:

1. The real-density measurement carries `gate.pass = null` and every metric
   `state: off_frozen_corpus_contract`. Per the harness's own contract that is
   **`not_verified`, not `pass`** — an unscored measurement, deliberately, so
   that a density variant can never be presented as a gate result.
2. It is a *projection*: 7.43 MiB/s measured over a 668 MiB corpus, extrapolated
   linearly to 1 GiB. No 1 GiB corpus was indexed end to end in this run.

### New finding: the density argument cuts both ways

The plan's §1.3.6 examined density only for throughput. Measuring all four
thresholds at real density shows something it does not record:

**At real transcript density, `search_latency_p95_ms` is 114.6/118.3 ms against
a 50 ms threshold and `mcp_single_call_latency_p95_ms` is 82.1/88.2 ms against
50 ms.** Both pass comfortably on the gated corpus (31–37 ms and 16–18 ms) and
both are 1.7–2.3× over threshold at the density that G4's own reasoning says is
the representative one. Roughly 21–23 ms of the search figure is process launch
(`cli_process_overhead_p50_ms`), so the query work itself is around 90 ms.

This matters for the pending decision to re-cut the gated corpus at real
density: doing so would move `initial_index_throughput_mib_s` from fail to pass
and simultaneously move `search_latency_p95_ms` and
`mcp_single_call_latency_p95_ms` from pass to fail. The number of failing
thresholds would go from one to two. Whichever corpus is chosen, the gate is not
clean on it, and choosing the corpus that makes throughput pass is not a net
improvement in the release position. Recorded here so that re-cutting the corpus
is not mistaken for closing the performance gate.

**Residual performance risks**:

1. `initial_index_throughput_mib_s` fails the gated corpus reproducibly; no
   scored corpus exists on which all four thresholds pass.
2. Search and MCP p95 exceed their thresholds at real density — measured here,
   not previously recorded.
3. `store_size_ratio` is 9.3–12.6× on the gated corpus and 3.55× at real
   density. Real users see roughly 3.6×, but there is no stated ceiling, so
   neither figure gates anything.
4. `embeddings_index_build_ms` was 326,734 ms (5.4 min) for 100k messages. It is
   informational and semantic retrieval is opt-in, but it is a real cost for
   anyone who enables it.
5. All of the above is one Windows host. No Linux or macOS performance figure was
   produced.

---



## 7. Current-state addendum after subsequent fixes

The original sections above are point-in-time evidence from the first rehearsal
pass. This addendum records reruns after the fixes made during the same release
rehearsal. It does not overwrite the historical measurements.

- **Current verified tip**: `a584ae3` plus the merge commits after it. The working
  verified source branch is the audit branch tip; no public-visibility change was made.
- **G10 re-run**: `python scripts/release/export_public_tree.py --destination <tmp>`
  exited 0, exported 489 files, and reported 0 privacy findings. Independent
  checks found no `.trellis`, dated go-no-go record, release template,
  public-history scrub runbook, architecture review, or release-tooling file in
  the export. The exported tree built, passed `cargo test --workspace`, and had
  no broken documentation links.
- **G15 re-run**: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `python -m unittest discover -s scripts/evidence
  -p "test_*.py"`, and `python -m unittest discover -s scripts/release
  -p "test_*.py"` were rerun. Rust and Python tests are green **after** the
  snippet-budget regression was fixed; the earlier `cargo test` failure was a
  real regression introduced by the match-centred snippet (ellipsis markers
  exceeded `max_snippet_chars`), not a stale verdict. The implementation now
  reserves marker budget and the e2e budget test passes.
- **Handoff schema/privacy re-validation**: a real handoff pack now validates
  against `schemas/handoff/v1/pack.schema.json`; native UUID-like message ids,
  raw BM25-relative relevance scores above 1, integer token budgets, and a Read
  activity with an absolute path were all exercised. The activity target is
  emitted as a basename and the directory chain is absent.
- **Important remaining gate interpretation**: G1/G5 real-corpus evidence is still
  `not_verified`; G2 remains a real fail (0 current Beta providers); G4/G3 remain
  mixed because no scored corpus makes all four performance thresholds pass;
  G11/G12/G13/G14 need the final records below. This addendum is not a go signal.

**Current recommendation remains NO-GO for formal public release.** The code and
export tree are substantially healthier, but the release gates that require real
corpus evidence, provider Beta promotion, and a clean all-threshold performance
position are not satisfied. M5-5 (repository visibility) is intentionally not
executed; the plan explicitly reserves that irreversible action for owner
confirmation.
