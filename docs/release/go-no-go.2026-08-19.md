
## 7. Current-state addendum after subsequent fixes

The original sections above are point-in-time evidence from the first rehearsal
pass. This addendum records reruns after the fixes made during the same release
rehearsal. It does not overwrite the historical measurements.

- **Current verified tip**: `a584ae3` plus the merge commits after it. The working
  branch is `worktree-public-release-audit`; no public-visibility change was made.
- **G10 re-run**: `python scripts/release/export_public_tree.py --destination <tmp>`
  exited 0, exported 489 files, and reported 0 privacy findings. Independent
  checks found no `.trellis`, dated go/no-go record, release template,
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
