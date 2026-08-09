# Implementation plan

No product code may be changed and `task.py start` may not run until the final
planning summary receives fresh user approval.

## 1. Domain and port contracts

- [x] Refactor stable `Message`/`Session` so contextual fields are not intrinsic.
- [x] Add `PlacementId`, `MessagePlacement`, `MessageEdge`, relation enum, graph
      validation, placement-aware `select_mainline`, and placement-aware
      `select_full`.
- [x] Add tests for deterministic placement identity/order, duplicate stable
      Messages, context-specific parents, orphan parents, ambiguity, cycles,
      sidechains, true-leaf selection with missing timestamps, path-independent
      fallback IDs, and invalid spans/ID kinds.
- [x] Add `ContextGraphStore` DTOs/methods and `&T` blanket implementation.
- [x] Update testkit builders/fakes to construct stable messages and placements.

## 2. SQLite v7 and atomic relation commit

- [x] Add the forward v6→v7 migration, indexes, relation-manifest columns, and
      source relation-completeness markers; prove old catalog bytes and nullable
      v6 membership are retained without fabricated completeness.
- [x] Extend source batches with placements/edges and source-placement claims.
- [x] Canonicalize and hash relation upserts/deletes in the durable intent;
      include source entity membership, placement claims, scan completeness,
      relation markers, and empty replacements; verify them under generation
      CAS inside the commit transaction.
- [x] Make no-op compare entities, source membership, placements, edges, and
      source claims; relation-only changes must activate a generation.
- [x] Replace a rescanned source's claims, delete only unshared placements/edges,
      preserve shared entities, and regenerate subtractive compatibility aliases.
- [x] For `skipped > 0`, union observed facts without tombstones, clear any
      relation-complete marker atomically, and keep context disabled until a
      later zero-skipped replacement.
- [x] Keep stable-content conflicts strict while removing contextual fields from
      conflict comparison.
- [x] Implement typed session graph, message-context, and aggregate context-stat
      reads without leaking SQLite types.
- [x] Prove failed batches roll back catalog, FTS, membership, relations,
      generation, and outbox activation together.
- [x] Prove FTS rebuild leaves relation rows/context results unchanged.

## 3. Ingest and Application

- [x] Preserve complete `ParseReport` through staging and report emitted/skipped
      counts honestly from ingest/sync.
- [x] Replace `staged_to_entries` with source staging that de-duplicates stable
      entity payloads while retaining every contextual placement and edge.
- [x] Derive placement IDs without source paths and preserve exact document/span.
- [x] Replace path+seq fallback Message IDs with provider/variant/document/ordinal
      `Unstable` IDs and prove identical document bytes at different paths agree.
- [x] Rewrite `handle_context` to load one typed graph, run Domain selection, and
      assemble evidence solely from selected placements.
- [x] Add placement identity to context message/leaf/evidence responses and apply
      existing response budgets to occurrences; preserve `messages[].id` as the
      Message-ID compatibility alias.
- [x] Add message-context candidate ADT; remove the TUI `Show`/`session` alias
      bypass, group candidates by distinct session, and key TUI evidence by
      placement/occurrence ID.
- [x] Keep CLI, Robot, Human, MCP, and TUI rendering on shared App responses; no
      frontend SQL or branch logic.

## 4. Synthetic coverage and compatibility

- [x] Add SQLite tests for same-session multi-document slices, shared Messages,
      per-document spans, divergent contextual parents, sequential chunks,
      re-sync no-op, relation-only change, shared-source survival, and tombstones.
- [x] Add v6→v7 migration tests: legacy show/list/get remain readable; context
      fails with `schema_incompatible`/re-ingest-required until every known
      contributing source is relation-complete; complete re-ingest populates
      relations and then enables context; injected migration failure leaves a
      clean v6 schema/version.
- [x] Add CLI/MCP/TUI/e2e fixtures combining all four real-data shapes; assert
      per-session branch chains and exact evidence placement/document/span.
- [x] Keep stable-content conflict and privacy/error/atomicity assertions strict.
- [x] Update the Python harness fixture to expose shared identity and re-parenting.
- [x] Change `INV-NO-PARSE-LOSS` to emitted source records = persisted
      source-placement claims and skipped = 0; keep all six invariant IDs.
- [x] Add tests that legal Message de-duplication does not look like loss and real
      skipped records cannot look green, cannot derive tombstones, and revoke a
      previous completeness marker.
- [x] Add compatibility tests for `id`, `parent_native_id`, `is_sidechain`,
      zero-message session documents, and mixed v6/v7 projections.

## 5. Documentation and evidence

- [x] Update affected crate specs after code behavior is verified.
- [x] Add v6→v7 migration/operator guidance and compatibility/re-ingest behavior.
- [x] Run the authorized real-data harness only after all code gates; do not read,
      copy, alter, upload, or commit transcript content. The green full-corpus
      run (generated `2026-08-09T21:10:16Z`) passes all six invariants with
      harness exit 0 over 1,242 sources and 1,177,479,794 bytes: 164,136
      emitted, 0 skipped; 231 sessions all context-successful; 659/659
      byte-precision spans; rebuild stable (catalog 151,562 -> 151,562). The
      earlier 2026-07-31 run (exit 5, recorded as-is) and the 2026-08-10
      subset run remain in the evidence record.
- [x] Update real-data evidence, provider maturity, and core evidence matrix from
      the latest actual run. (Committed 2026-08-10: green full-corpus run
      recorded in real-data-regression.md, core-beta-evidence-matrix.md
      IB-REAL-DATA-REGRESSION-001/IB-SESSION-CROSS-SOURCE-001, and
      PROVIDER-MATURITY-MATRIX gap 4 closed.) Both providers remain
      Experimental pending independent review and owner decision; the failed
      2026-07-31 run is not presented as green.
- [x] Review the parent task's remaining integration criteria without repeating
      archived children 1–8. The review is complete; parent acceptance items
      that depended on the corpus-wide full-root run with all six invariants
      green are now met by the 2026-08-09 run.

## 6. Validation gates

Run in this order and stop to fix root causes on any failure:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo build --locked --release -p agentsessions-cli
python -m unittest scripts/evidence/test_real_data_regression.py
```

Then run the authorized local regression against both configured provider roots:

```text
python scripts/evidence/real_data_regression.py \
  --binary target/release/agentsessions.exe \
  --sources "$HOME/.claude/projects" \
  --sources "$HOME/.codex/sessions" \
  --out evidence-output/real-data-regression.json
```

Green requires harness exit 0 and all six invariants passed. Anything else is a
failed run and must be recorded as such. Verify the report contains only the
closed aggregate field set and no transcript text, prompt, native ID,
fingerprint, hostname, username, or personal path.

## 7. Review and rollback points

- Gate A: Domain/port tests before persistence changes.
- Gate B: migration + atomic/no-op/tombstone tests before Application wiring.
- Gate C: shared ADT/e2e tests before harness changes.
- Gate D: all code gates before real-data access.
- Gate E: evidence documents only after the latest run result is known.

Do not commit or push. After implementation, checks, real-data evidence, and
independent review are complete, present a logical split-commit plan and wait
for explicit commit authorization; push requires separate authorization.
