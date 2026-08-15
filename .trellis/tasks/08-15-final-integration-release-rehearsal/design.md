# Final Integration Release Rehearsal — Design (framework slice)

> Framework skeleton only. The actual rehearsal cannot run yet: most 08-15
> features (Web UI, handoff pack, resume execution, offline privacy hooks,
> semantic search) are being built in parallel by sibling agents. This design
> covers the harness, runbook, and templates those features will slot into.

## Context

The parent task (`08-15-open-source-product-roadmap`) requires a full
fresh-environment rehearsal (install → index → search → context → resume →
handoff → Web UI → uninstall) plus a five-entry-point consistency final check,
privacy/performance/materials final checks, and a Go/No-Go report. The
rehearsal is gated on features that do not exist yet; the framework must be
landable today and grow without churn.

## Decisions

### D1 — Rehearsal steps are declarative, feature-gated

The runbook (`docs/release/rehearsal-runbook.md`) defines every step of the
procedure. Steps depending on unlanded features carry an explicit
`[pending feature <subtask>]` marker and a defined expected-evidence contract.
This keeps the runbook authoritative from day one while the feature slices
land in parallel — the rehearsal can run in partial mode today (steps 0-4, 8-9,
13-14) and full mode after the tree converges.

### D2 — Consistency script is stdlib-only, adapter-based

`scripts/rehearsal/compare_entrypoints.py` models each of the five entry
points as an adapter. Today only `cli` and `mcp` are implemented; `robot` is
recorded as an alias of the CLI binary (same envelope by construction); `web`
and `tui` are explicit `{"status": "skipped", "reason": "not implemented"}`
records — never silent absence, so a future addition of an entry point cannot
masquerade as a passing check without the report updating.

Comparison is over a **closed canonical field set** per operation (ids,
booleans, counts — never message text, absolute paths, provider-native ids,
fingerprints, usernames, hostnames). Transport envelopes (Robot v1 envelope,
JSON-RPC frame) are stripped before diffing; non-semantic ordering fields
(`request_id`, `meta.duration_ms`) never enter the view.

### D3 — Rust smoke test is fixtures-only and green today

`crates/agent-session-grep-cli/tests/e2e_consistency.rs` shells the script
against a synthetic fixture catalog in a temp dir (via `CARGO_BIN_EXE_`), and
asserts the report contract: schema version, `overall_verdict == consistent`,
all five entry points present as compared/aliased/skipped, pending entry
points `status == skipped` with `reason == not implemented`, and privacy
assertions. It is a separate test target (`e2e_consistency`) so the existing
suite is untouched.

### D4 — Environment manifest and Go/No-Go are templates

`docs/release/environment-manifest.template.json` fixes the per-platform
environment evidence (OS/build, clean image id, installer hash, toolchain,
fixture license + redaction status). `docs/release/go-no-go.template.md` fixes
the owner decision record (privacy/performance/materials final checks,
five-entry consistency, provider evidence, residual risks, sign-off). Both are
templates until the rehearsal actually runs.

## Boundaries

- Framework files only: runbook, templates, consistency script, Rust smoke
  test. No production code changes.
- No real personal paths, hostnames, identities, or provider transcripts —
  synthetic fixtures only; provider transcripts stay read-only.
- No new Python dependencies beyond the standard library.

## Compatibility

- The consistency script's report schema
  (`agent-session-grep.entrypoint-consistency/v1`) is versioned; adding an
  entry point adapter or a canonical operation is additive.
- The Rust smoke test is a new test target; existing tests are untouched.
