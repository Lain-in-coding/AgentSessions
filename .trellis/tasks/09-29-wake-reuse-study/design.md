# Design: isolated Wake reuse study

## Boundaries and provenance

The only shared writes are under this task directory. External Wake sources are read-only. The root Cargo workspace is unchanged. Each Rust experiment is an independent crate with an explicit `[workspace]`; build products go to ignored `target/` directories. No Git path dependency on Wake and no GPUI build.

Use portable, repository-relative paths in tracked reports. Accept external checkout/workspace/output paths as explicit runtime arguments; do not persist user names, home paths, credentials, or local task pointers. Reference Wake by upstream URL, full commit, blob hash, and line range. Keep generated database/corpus files out of version control.

Research prototypes are independent adaptations, not imports of Wake modules. Acknowledge the MIT source and copyright where deriving implementation, retain the full MIT notice locally to the experiment, and never copy transcript fixtures. Product reuse approval remains separate.

## Experiment ownership and output

- Snippet worker owns `research/experiments/snippet/` and `research/results/snippet.json`.
- Search worker owns `research/experiments/search/` and its smoke results. It supplies a portable full-run interface; the coordinator runs large measurements serially, without concurrent builds.
- Provider worker owns `research/experiments/providers/` and `research/results/providers.json`.
- Coordinator owns task artifacts, `research/report.md`, `research/reuse-matrix.json`, `research/sources.json`, shared provenance/environment, integration commands and final full-run results.

Do not edit another owner's files while work is in progress. Workers must not spawn implement/check children or change task state, commit, push, update product specs, or mutate the source checkout.

## A: snippets

Use Rust Unicode scalar semantics to compare the existing prefix behavior with a bounded hit-centered candidate. Derive offsets through lower-case expansion to original character boundaries; never use transformed byte offsets on the source. Treat input as literal terms, with an explicitly tested final-star prefix boundary. Empty/nonmatching/semantic-only terms retain an honest no-match/prefix outcome. Output remains plain text (no markup). Test character and final JSON byte limits, escaped/control characters, expansion characters, CJK, emoji, long-tail and multiple hits. This prototype changes no product contract/ranking/cursor.

## B: search and connection lifetime

Use the same runtime SQLite library as the baseline dependency selection (`rusqlite 0.40.2`, bundled) in an isolated Rust harness. Compare independently constructed FTS tables on the identical deterministic beacon corpus. Preserve CJK unigram/bigram semantics in the baseline; quote plain tokens and handle the current final-star behavior. The Wake-inspired branch uses trigram MATCH for long terms and explicit LIKE for short terms. Record semantic differences, wildcard escaping, query plans, result IDs and relevance judgements rather than assuming equivalent retrieval.

Cache comparison must reuse identical SQL/data/order: ordinary prepare versus feature-gated prepare_cached; persistent connection and reopen-per-request modes. Cache builds/locks are isolated and root dependencies remain unchanged. Record dependency/artifact differences as prototype-only. No performance assertions in ordinary tests.

Measure an unmodified release product binary separately through explicit synthetic source paths and explicit scratch DBs. Reuse the existing benchmark statistics, deterministic generation approach and report obligations where compatible; do not edit the existing harness. CLI startup 20 samples, query 100 samples, no-op sync 3 samples. Record actual statuses, subprocess errors, timeout/resource limits, artifact SHA-256, linked SQLite evidence, raw nearest-rank percentiles, dataset hash, OS/CPU/RAM/disk/filesystem/antivirus and sampling limitations. Use bounded source batches to avoid Windows command-line limits. Do not call sync --discover. A fixed runtime timeout must produce explicit incomplete evidence, never an invented zero or an infinite wait.

Full scales: 10,000 / 100,000 / 1,000,000 messages, executed in order. Do not compare timing under simultaneous experiment/build load. A smoke run only validates the pipeline and is labeled as such. No OS cache flushing or claims of a cold disk. No Wake-product comparative benchmark claim.

## C: provider structure probes

Implement only enough Cursor IDE/Hermes schema knowledge to test format assumptions against generated DBs. Preserve exact native text/IDs and report unknown/missing/malformed data. Cursor message order follows fullConversationHeadersOnly, not KV sorting. Hermes profiles namespace observations; ambiguous call/result matching is not authoritative. Reads must be bounded before materialization; never flatten away SQL row errors. Snapshot tests cover synthetic DB/WAL/SHM; use safe read-only access or an explicitly consistent private snapshot, not a copied three-file race. A successful probe does not certify a provider, resume support, or canonical IDs.

## Reporting and rollback

Each result states synthetic-only and experimental status; failed/inconclusive runs retain reasons. The Chinese report separates confirmed implementation behavior, experimental evidence, and unverified upstream assumptions. No product API/schema change means no product migration or rollback. Rollback is simply not adopting prototypes; do not delete unrelated directories or reset existing worktrees.
