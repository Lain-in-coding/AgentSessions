# Design
Follow parent decisions and revised finding semantics.

## Exclusive ownership
SQLite crate; ports/lib.rs; application/lib.rs and coupled tests. Main owns docs, task artifacts, commit and push; never revert another worker.

## Approach
Repo/facet-filtered ownership uses matching placement/session deterministically. Generation/search/payload/ownership use one connection/read transaction; no process-only lock. Same-source latest replaces old text; cross-source deterministic merge/removal uses live projections. Preserve full/incomplete claimant semantics. Alias candidates include entities losing this source but retained by another. Invalidate vectors on final content change; remove orphans; model/dimension readiness matches query. Context activity errors propagate. Preserve upstream batch scope. Additive migration cannot invent historical per-source content.

## Compatibility
Retain contracts except specified corrections. Document migration/cache changes. Revert isolated commits only after accounting for new data; never remove validation to roll back.

## Main-session baseline design review
Verified on 2edf2dc: commit_source_batches_if_changed still merges stored payload into incoming text regardless of source authority; alias regeneration collects candidates only after membership replacement; semantic readiness has no dimension condition. These are not fixed merely by upstream batch-scoping.

Persisted per-source projection evidence is necessary for deterministic source removal; the present merged payload cannot reconstruct it. Prefer an additive v18-to-next migration with an explicit missing-evidence legacy state, not fabricated copies of catalog content per source. Force one source reparse with PARSER_SEMANTIC_VERSION 3 -> 4 (also needed by provider corrections), then let current-source observations become authoritative. Keep conservative legacy/incomplete behavior until every needed live claimant has current evidence; document this limit and ensure no stale same-source merge after its replacement. Integrate projection persistence with verified durable batch/outbox transaction, no out-of-band write.

Snapshot scope should start before generation and cover all read orchestration including payload/ownership/resume metadata. A port-owned RAII read unit can share SQLite's connection without holding its RefCell borrow across calls; consider nested use and release on errors. BEGIN alone does not pin until the first read. In production every data port must use that same store. Use a two-connection WAL test with an injected writer between generation/query/payload; test next request sees new data and failed requests release snapshot. Do not hold snapshot across model loading, prompts or network waits.

All-request consistency, source authority, alias cleanup and vector changes may be split internally but must retain one correct final integration. Notify main before public schema changes beyond these approved corrections. Ports/app lib are owned here; CLI owns loader diagnostics, model worker owns candle/handoff.
