# Relational message placement and cross-source edges

Parent: `07-24-advance-integration-beta` child 9. This task closes the
model-level blocker left by archived child 8; it does not repeat children 1-8.

## Goal

Make a provider-native message a stable identity/content entity while recording
its session, source/document, parent, span, and branch position as contextual
relationships, so one native message can appear in several resumed or forked
transcripts without conflicting projections or ambiguous branch selection.

## Confirmed Facts

- The authorized local real-data regression currently fails `INV-SYNC-OK` with
  `catalog_error` (exit 6) after 7060 committed messages; the other five
  invariants are not evaluated because sync does not complete.
- Real Claude Code data proves four cross-source shapes: sessions span files,
  messages are shared by sessions, spans vary by document, and parent edges vary
  by context. Archived child 8 fixed the first three with multi-value payload
  fields and singular compatibility aliases; the fourth cannot be unioned
  without making mainline selection arbitrary.
- `select_mainline` and `select_full` currently consume one parent value from
  each reconstructed message. Context must instead select relationships within
  the requested session/context before applying either policy.
- Existing v6 payloads cannot be losslessly backfilled into placements: session
  members are de-duplicated by Message ID and parent/span aliases no longer
  preserve their source/session/ordinal correspondence. Migration must retain
  those rows without fabricating missing relationships.
- Provider `ParseReport.skipped` is currently dropped by staging, while
  `INV-NO-PARSE-LOSS` compares emitted source occurrences with de-duplicated
  stable Message entities. The relational model requires an occurrence-aware
  count and an explicit zero-skipped check; this strengthens rather than relaxes
  the no-loss invariant.
- RFC-0001 §3.2 separates message identity from parent-child relations through
  `MessageEdge`; the current implementation has not completed that model.
- Both implemented providers remain Experimental. The contract RFC remains
  Draft, and no status may be promoted by this task without separate evidence
  and owner action.

## Requirements

- R1 Define the stable fields owned by a canonical Message and exclude
  session/source/document-specific placement facts from its intrinsic identity.
- R2 Represent session membership as an explicit contextual relation that
  supports one message in several sessions and one session across several
  documents.
- R3 Represent parent-child edges per context so the same native child can have
  different parents in different resumed/forked transcripts without conflict.
- R4 Associate each evidence span with the exact source document and contextual
  message placement it describes; never select an arbitrary document alias.
- R5 Make context lookup for a requested session load only that session's
  placements/edges, then run `select_mainline` or `select_full` deterministically
  over that graph. Mainline selection must choose a real leaf in the resolved
  contextual graph before applying the deterministic placement order; ordering
  alone may not relabel an internal node as a leaf.
- R6 Extend domain and port contracts only where the Application core needs a
  backend-independent capability; no SQLite types or JSON payload details may
  leak through the port layer.
- R7 If persistence changes, use an additive, forward SQLite migration with
  explicit compatibility behavior for pre-change catalogs. Do not drop and
  recreate user data to avoid migration work. Do not infer relational rows from
  incomplete aliases: until every known contributing source has been re-ingested
  into the new relation schema, context must fail explicitly as
  `schema_incompatible`/re-ingest-required rather than return a partial graph.
  A recoverably skipped record makes that source relation-incomplete: an
  incomplete re-scan may upsert observed facts but may not derive tombstones,
  must clear any prior relation-complete marker, and must keep context disabled
  until a later zero-skipped complete scan replaces the source claims.
- R8 Preserve honest compatibility for current `document`/`documents`,
  `session`/`sessions`, `span`/`spans`, `parent`, `parent_native_id`, and
  `is_sidechain` fields, plus the current context-message `id`. The frozen
  design must state which aliases remain readable, which remain emitted
  temporarily, and which are migrated or retired; no alias may be treated as
  complete when it is only a singular view.
- R9 Preserve source read-only behavior and the existing sync, incremental
  re-sync, tombstone, generation, content-level no-op, atomic batch, and index
  rebuild semantics. Durable intent/no-op state must include source-owned entity
  membership, placement claims, scan completeness, and relation-completeness
  markers, including an honestly empty replacement.
- R10 Keep CLI context, MCP `get_session_context`, and TUI on the same
  Application request/response ADT. Frontends may not query SQLite or implement
  branch selection independently.
- R11 Add synthetic tests for all four real-data shapes, including one native
  message placed in multiple sessions/documents with different parents. Tests
  must fail under the old global-parent model and must not weaken conflict or
  privacy assertions. Preserve `ParseReport` accounting and make the no-loss
  gate compare emitted source records with persisted placement claims while
  requiring zero skipped records; legal stable-Message de-duplication is not
  parse loss. Also cover a complete scan followed by an incomplete re-scan,
  zero-message session/document attribution, path-independent fallback identity,
  multiple placements in one session, and compatibility response aliases.
- R12 Re-run the authorized real-data harness against both provider roots after
  all code gates. A green result requires all six invariants, harness exit 0,
  and an aggregate-only report; anything else is recorded as failed without
  hiding the error.

## Acceptance Criteria

- [ ] One stable native message can be represented once and placed in multiple
      session/source/document contexts with different parent edges and spans.
- [ ] A requested session's `mainline` and `full` context are deterministic and
      use only relationships belonging to that session/context; `mainline`
      starts from a real graph leaf even when timestamps are missing.
- [ ] Cross-context parent differences no longer cause `catalog_error`, and a
      genuinely conflicting stable message identity/content still fails loudly.
- [ ] Existing catalogs follow the frozen compatibility/migration policy:
      `get`/`list`/`show` remain readable, context does not consume aliases and
      fails explicitly until all known contributing sources are relation-complete,
      incomplete scans cannot retire unseen facts or retain a completeness
      marker, and sync/re-sync/tombstone/generation/no-op/rebuild behavior stays
      correct.
- [ ] CLI, MCP, and TUI continue to consume one shared Application ADT and agree
      on context results. Context messages retain `id == message_id` as a
      compatibility alias while placement identity is authoritative; TUI
      ambiguity is based on distinct sessions, not raw placement count.
- [ ] Synthetic unit/contract/e2e coverage exercises every verified real-data
      shape, including re-parenting across resumed/forked transcripts, and proves
      emitted source-record count equals persisted placement claims with zero
      skipped records even when stable Messages are de-duplicated.
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
      warnings`, `cargo test --workspace`, `cargo deny check`, and `cargo build
      --locked --release -p agentsessions-cli` all pass.
- [ ] `python -m unittest scripts/evidence/test_real_data_regression.py` passes.
- [ ] The authorized real-data regression sync completes, all six invariants
      pass, the harness exits 0, and reports contain no transcript text,
      prompts, native ids, fingerprints, hostnames, usernames, or personal
      paths.
- [ ] Evidence documents and maturity matrices match the latest real run; both
      providers remain Experimental until all independent promotion gaps close.

## Constraints and Out of Scope

- Real provider transcripts are read-only and never copied, modified, uploaded,
  or committed. Fixtures are synthetic or irreversibly de-identified.
- Do not relax catalog conflicts, branch invariants, privacy validation, or exit
  codes merely to obtain a green run.
- Do not amend RFC status, mark Draft/Proposed governance records Accepted, or
  promote providers as part of this task.
- Do not add unrelated provider support, semantic/vector search, or frontend
  features.
- No product implementation begins until `design.md`, `implement.md`,
  `implement.jsonl`, and `check.jsonl` are complete and the final planning
  summary receives fresh user approval.
