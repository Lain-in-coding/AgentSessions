# Wake reuse study and isolated verification

## Goal

Decide what Wake can contribute to agent-session-grep through a pinned source study and reproducible isolated experiments, without changing the product. The user approved the research-plus-isolated-verification plan and explicitly requested implementation on 2026-09-29.

## Requirements

- Fix Wake at v0.8.5, commit `71aeca67ec80f8645d1f9d5199290c2c732036ce`; preserve its MIT license and Git provenance. The external checkout belongs under the user-requested `Github_src/Wake`, never the main repository index or Cargo workspace.
- Fix the product comparison at `2b8f89562e43bd1f68ad3c8e5cf8ccc9281f9af2`. Work on the independent `research/wake-reuse-study` branch. Do not change the relocation CI task, PR #12, or pre-existing entry-worktree files.
- Deliver a Chinese report, machine-readable four-tier reuse matrix, exact source coordinates, provenance, runnable experiments, and raw JSON results.
- Distinguish direct-copy, adapt, idea-only, and reject. Existing product features are not new Wake contributions. Source inspection is not upstream provider certification.
- Experiment A compares prefix snippets with match-centered Unicode-safe snippets, including final serialized-byte limits and no fabricated literal matches.
- Experiment B measures 10k, 100k, and 1m synthetic messages: unmodified product baseline; separate CJK/FTS versus trigram/short-LIKE strategy experiments; and prepare versus prepare_cached with identical SQL/results and short/long connection lifetimes. Record dependencies, artifact size, memory availability, query plans, qrels, and raw samples. Microbenchmarks are not end-to-end Wake or product speed claims.
- Experiment C uses minimal synthetic Cursor IDE cursorDiskKV and Hermes SQLite probes with ordering, exact native identifiers, malformed/missing rows, bounded reads, profile isolation, and source immutability checks. DSH/ZCode receive evidence/risk assessment only, not full adapters.
- All datasets are generated. Never read user transcripts, copy external transcript fixtures, start a real provider, invoke shell/clipboard resume, run Wake GUI, scan default home roots, or access remote mirrors.
- No product source, public API/wire/schema, main Cargo manifests/lockfile, default feature, product spec, release promise, or SLO change. Real semantic models remain optional/offline.

## Acceptance Criteria

- [ ] External checkout HEAD and license verified; checkout clean and ignored by the entry repository.
- [ ] Report/matrix have pinned file/line evidence, local mappings, compatibility obligations, honest confidence, and rejected patterns.
- [ ] Snippet and provider probes pass substantive positive/negative synthetic tests; JSON reports and fixture provenance are present.
- [ ] Search experiments attempt all three scales and preserve successful samples or explicit failure/timeout/resource evidence without inventing results.
- [ ] Baseline/experiments record reproducible dataset hashes and actual runtime metadata, not Python SQLite as product SQLite or new-process timing as cold-disk timing.
- [ ] Existing statistics/dataset tests and new experiment gates pass; result validators reject tampering and enforce recorded sample counts/status.
- [ ] Independent Trellis review passes, source checkout remains unchanged, no product-scope diff, and git diff --check passes.
- [ ] Final Chinese conclusions rank follow-up candidates without implementing or certifying them.

## Notes

Negative or inconclusive reuse findings are valid results. No numerical performance threshold is frozen by this study. Large generated datasets and build/database artifacts remain untracked.
