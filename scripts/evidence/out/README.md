# Generated evidence output

Files in this directory are **generated** by local evidence harnesses:

- `gate-manifest-<profile>.json` — from
  `scripts/evidence/open_source_gate_benchmark.py run`
- `install-gate-<os>.json` — from
  `scripts/install/gate_smoke.ps1` / `gate_smoke.sh`

Do not commit these files: they contain local machine details and are
reproducible by re-running the harnesses.
