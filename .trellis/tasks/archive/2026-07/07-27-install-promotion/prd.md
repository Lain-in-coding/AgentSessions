# PRD: Installation, smoke evidence, runbook, promotion

Parent: `07-24-advance-integration-beta` child 7 (final child before parent
integration review). Closes the two remaining Beta gaps in
`docs/product/PROVIDER-MATURITY-MATRIX.md` (§晋级缺口 4-5) to the extent
authorized: NO publishing/signing/notarization; real user data never leaves
this machine and its content is never committed.

## Requirements

- R1 Install scripts `scripts/install.ps1` + `scripts/install.sh`: wrap
  `cargo install --path crates/agentsessions-cli --locked`, then verify
  `agentsessions --version` and `agentsessions doctor` succeed. Local
  install only; clear failure messages; no network beyond cargo itself.
- R2 Smoke scripts `scripts/smoke.ps1` + `scripts/smoke.sh`: build, then on
  a throwaway temp dir with a SYNTHETIC fixture drive the real binary:
  doctor → ingest → search `--robot` (assert envelope fields + exit 0) →
  context (assert evidence present) → get/status → error paths (not-found →
  exit 4, garbage cursor → exit 2) → MCP handshake (initialize → tools/list
  → one tools/call over stdio; assert pure JSON-RPC + exit 0). Any assertion
  failure exits non-zero. No personal paths; runs offline after build.
- R3 CI: extend `.github/workflows/ci.yml` with a smoke step running R2 on
  all three OS targets — this is the repeatable clean-machine evidence
  (matrix gap 5 evidence carrier via PR #1 checks).
- R4 Runbook `docs/operations/rebuild-and-migration-runbook.md`: operator
  procedures — schema upgrade path (linking `migration-v5-to-v6.md`), full
  FTS rebuild (`index rebuild`), store reconstruction by re-ingest,
  writer-lease contention and interrupted-batch recovery (doctor signals),
  generation/cursor invalidation semantics. English, honest about limits.
- R5 Real-data regression `scripts/real-data-regression.ps1` (Windows-first,
  documented): with explicit user-authorized source globs, ingest ALL real
  Claude Code + Codex transcripts into a throwaway temp store and report
  AGGREGATE-ONLY results (source/message/session counts, parse failures,
  duration). Evidence file `docs/evidence/integration-beta/real-data-regression.md`
  records aggregates + method ONLY (no content, no personal paths). One
  authorized local run executed and recorded this round.
- R6 Update `PROVIDER-MATURITY-MATRIX.md`: gap 4 → repeatable script +
  recorded run; gap 5 → state exactly what CI has/hasn't certified at time
  of writing. Providers REMAIN Experimental unless every documented gate is
  green — do not promote by implication.

## Acceptance criteria

- [ ] `scripts/smoke.ps1` passes locally end-to-end (recorded exit 0).
- [ ] `scripts/install.ps1` verified locally; sh variants lint-clean and
      mirror the ps1 logic (full sh verification happens in CI).
- [ ] ci.yml smoke step added for all 3 OS; workflow YAML valid.
- [ ] Runbook exists, covers the 5 listed procedures, no invented commands —
      every command it names exists in the current CLI.
- [ ] Real-data regression executed once locally; evidence file contains
      aggregates only (reviewed for privacy before commit).
- [ ] Matrix updated truthfully; both providers still Experimental with
      precisely stated remaining blockers.
- [ ] Full gates green; no new dependencies.

## Out of scope

- crates.io publish, signing, notarization, release binaries/tags.
- Marking providers Beta (owner decision after PR CI + review).
- Parent integration review (separate wrap-up).
