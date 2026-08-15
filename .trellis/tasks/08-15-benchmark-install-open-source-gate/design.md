# Benchmark, Install and Open-Source Gate — Design

> Slice 1 (this task): benchmark harness extension + gate evidence manifest +
> synthetic labeled corpus + install gate runner + roadmap §5 count fix.

## Context

`scripts/evidence/core_beta_benchmark.py` already measures startup/sync/
search/show/get latency, index_size, and peak RSS for a synthetic Claude Code
corpus. The open-source gate (PRD Q55) additionally needs:

- **discovery_coverage** — what fraction of provider-root JSONL sources a
  `sync --discover` run surfaces (currently the CLI has no `--discover` flag;
  the task `08-14-provider-source-auto-discovery` adds it). This slice records
  the metric scaffolding so the gate can populate it once that flag lands.
- **parse_loss** — records committed vs skipped per source (the CLI `sync`
  envelope already reports `messages` and the provider `ParseReport` carries
  `skipped`).
- **lexical recall** — on a **labeled** synthetic fixture corpus: queries with
  expected message/session ids, so recall@10 is deterministic.
- **p50/p95 latency aggregation** across the existing latency metrics (the
  core harness already emits per-metric nearest-rank percentiles; the gate
  manifest re-projects them into a single p50/p95 block for cross-metric
  comparison).
- **resume/handoff success counters** — recorded as `not_applicable` until the
  Resume-execution feature (task `08-15-resume-metadata-execution`) lands.
  Never silently skipped.

## Decisions

### D1 — New sibling script, not a fork of the core harness

Create `scripts/evidence/open_source_gate_benchmark.py` as a **separate** file
that **imports** helpers from `core_beta_benchmark` (`nearest_rank`,
`rounded_summary`, `sha256_file`, `tree_hash`, `environment`, `run_process`,
`parse_frame`, `cli`). The core harness stays the authoritative latency/index
evidence source; the gate harness adds the labeled-corpus metrics and the
gate manifest. Keeping them separate preserves the core harness's frozen
`schema_version` and validator.

### D2 — Synthetic labeled corpus (static fixtures, two providers)

A small static fixture set under `scripts/evidence/fixtures/gate/`:

- `claude/session-alpha.jsonl`, `claude/session-beta.jsonl` — Claude Code
  transcript shape (root → reply → sidechain), synthetic tokens only.
- `codex/rollout-gamma.jsonl` — Codex rollout shape (session_meta →
  response_item messages).
- `labels.json` — query → expected `msg_v1_*` / `ses_v1_*` id sets.

No real paths/hostnames/identities. Tokens are invented nonsense
(`gateprobe`, `kairostopic`) to avoid collision with any real store.

### D3 — Gate evidence manifest format

A JSON file under `scripts/evidence/out/gate-manifest-<profile>.json`:

```json
{
  "schema_version": "agent-session-grep.open-source-gate/v1",
  "generated_at_utc": "...",
  "commit": "<40-char sha>",
  "environment": { "os": "...", "arch": "...", "build": "release",
                   "commit": "...", "provider_fixture_set_id": "..." },
  "metrics": [
    { "name": "lexical_recall_at_10", "unit": "ratio", "value": 1.0,
      "threshold": 0.95, "pass": true },
    { "name": "parse_loss_ratio", "unit": "ratio", "value": 0.0,
      "threshold": 0.05, "pass": true },
    { "name": "discovery_coverage", "unit": "ratio", "value": null,
      "state": "not_applicable",
      "threshold": 0.95, "pass": null,
      "reason": "sync --discover not yet implemented (task 08-14)" },
    { "name": "resume_handoff_success", "unit": "count", "value": null,
      "state": "not_applicable", "threshold": 1, "pass": null,
      "reason": "resume execution not yet landed (task 08-15-...)" }
  ],
  "latency_p50_p95_ms": {
    "search": { "p50": ..., "p95": ... },
    "show": { ... }, "get": { ... }, "initial_sync": { ... }
  },
  "index_size_ratio": ...,
  "gate": { "pass": true, "failures": [], "deferred": [...] }
}
```

A metric with `state: "not_applicable"` and `pass: null` is recorded
explicitly — never silently omitted. The overall `gate.pass` is true only if
every applicable metric passes; deferred metrics are listed in
`gate.deferred` and do not fail the gate.

### D4 — Install gate runner (best-effort wrappers)

`scripts/install/gate_smoke.ps1` and `gate_smoke.sh` run the
install → smoke → uninstall → reinstall sequence locally, wrapping the
existing install/uninstall/smoke scripts into a throwaway prefix. They emit a
JSON result fragment into `scripts/evidence/out/` for the manifest. They do
**not** modify CI workflows (out of scope this slice).

### D5 — Roadmap §5 count reconciliation

`docs/product/OPEN-SOURCE-ROADMAP.md` §5 says "13 external projects" but lists
14 names (cass included). Fix: state that 13 are free-to-reference while cass
is clean-room-only due to its restricted-party license rider, so the "13
external projects" count refers to the freely-referenceable set and cass is
called out separately. Minimal edit, no structural change.

## Boundaries

- **Read-only on provider sources**: the harness syncs synthetic fixtures only.
- **No real data**: all fixtures are synthetic; no real transcript paths.
- **No new Python deps**: stdlib only + helpers reused from the core harness.
- **No CI workflow edits** this slice.
- **Windows Defender hazard**: verify each written file persists; rewrite if
  missing (per project memory).
