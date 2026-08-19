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
| Branch | `worktree-agent-a24728f9d894f8c51`, fast-forwarded to the audit branch tip |
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

_Sections for G3/G4 (§2), G5, G11–G15 follow as they are executed._

