# Benchmark, Install and Open-Source Gate — Implementation

## Preconditions

- Work directly in the repository root (scripts/docs scope only).
- Read `core_beta_benchmark.py`, install scripts, roadmap §5.

## Steps

### 1. Synthetic labeled corpus

Create `scripts/evidence/fixtures/gate/`:
- `claude/session-alpha.jsonl` — 4 messages, token `gateprobe`.
- `claude/session-beta.jsonl` — 4 messages, token `kairostopic`.
- `codex/rollout-gamma.jsonl` — 4 messages, token `zetacode`.
- `labels.json` — query → expected id sets (msg_v1_* and ses_v1_*).

### 2. Gate benchmark harness

Create `scripts/evidence/open_source_gate_benchmark.py`:
- Import helpers from `core_beta_benchmark`.
- Subcommands: `run`, `validate-report`.
- `run`:
  - resolve binary (build or `--binary`).
  - sync the labeled corpus into a temp store.
  - measure search/show/get latency (p50/p95) over the label queries.
  - compute lexical recall@10 from labels vs search hits.
  - compute parse_loss from the sync envelope (skipped / total).
  - emit discovery_coverage and resume_handoff_success as
    `not_applicable` with explicit reasons.
  - write manifest to `scripts/evidence/out/gate-manifest-<profile>.json`.
  - print the manifest path.
- `validate-report`: check schema_version, metric set, pass/threshold
  consistency.

### 3. Install gate runners

Create `scripts/install/gate_smoke.ps1` and `gate_smoke.sh`:
- Accept `--prefix`, `--binary`, `--output-dir`.
- Sequence: install (skip-build if binary given) → smoke → uninstall →
  reinstall → smoke again (idempotency).
- Emit `install-gate-<os>.json` into output dir.
- Best-effort; do not fail the gate if a platform script is not applicable
  (e.g. .sh on Windows).

### 4. Roadmap §5 fix

Edit `docs/product/OPEN-SOURCE-ROADMAP.md` §5: reconcile the 13-vs-14 count.

### 5. Task bookkeeping

- Append `check.jsonl` entry.
- Update `implement.jsonl` with the spec/research files.

## Validation

- `python scripts/evidence/open_source_gate_benchmark.py --help` runs.
- Run the harness end-to-end on the synthetic corpus; attach manifest path.
- PowerShell syntax check the .ps1 (dry parse).
- `python scripts/evidence/open_source_gate_benchmark.py validate-report
  <manifest>` passes.
- Do NOT commit/push.
