# Generated evidence output

Files in this directory are **generated** by local evidence harnesses:

- `gate-manifest-<profile>.json` — from
  `scripts/evidence/open_source_gate_benchmark.py run`
- `install-gate-<os>.json` — from
  `scripts/install/gate_smoke.ps1` / `gate_smoke.sh`
- `semantic-benchmark-<profile>.json` — from
  `scripts/evidence/semantic_benchmark.py run` (status
  `locally_verified` or `model_not_imported`)

Do not commit these files: they contain local machine details and are
reproducible by re-running the harnesses.
