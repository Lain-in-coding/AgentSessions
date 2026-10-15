# O7 quality review

Date: 2026-10-15. Status: **read-only preliminary review complete; no
production-code blocker found. Final candidate/evidence handoff pending.**

## Scope and historical boundary

- Active task: `10-15-optimize-50k-changed-sync`.
- Baseline for this delta: S2
  `e8e4975817f71647c158f6d802a7b98cee2f1b15`.
- Provisional working `src/lib.rs` blob observed during implementation:
  `bdab4a0826efb0896b872494dcd7369da135cba1`. This is not a frozen candidate.
- Only `crates/agent-session-grep-adapters-sqlite/src/lib.rs` differed from S2
  at that checkpoint: the deletion helper/call site and necessary tests.
- The implementer owns product edits. The reviewer has made no product edit,
  build, test, benchmark, commit or remote mutation during this preliminary pass.
- The S2 review and frozen evidence remain historical and unchanged. S2's
  formal R4 failed only 50k shrink: median 8717.9847 ms versus strict threshold
  8451.728755 ms, a gap of 266.255945 ms. Its 12.3222% improvement is not success.

Read context: updated `check.jsonl` and its new
`research/shrink-followup-after-g4.md` reference, retained correctness/spec
context, design section 9 and implementation plan's fifth group. The approved
choice is the existing **500**-parameter chunk, not the historical 512-row
research estimate. No validation/schema/harness change is recommended.

## Findings (fixed)

None required in the production delta inspected. This pass is read-only.

## Findings (not fixed)

No concrete production correctness defect found. New full-package/lint results,
the frozen candidate, strict oracle and A/B evidence remain to be audited.
Successful S2 checks cannot be relabeled as O7 results.

## Production-path conclusions

| Contract | Read-only conclusion |
| --- | --- |
| Exact Edge key | Fixed `message_edges.child_placement_id`, unchanged from S2. |
| Exact Placement key | Fixed `message_placements.placement_id`, unchanged from S2. |
| Activity/Usage | Original `activity_id` / `usage_id` single-row paths retained. |
| Adjacent runs | The predicate joins only Edge/Edge or Placement/Placement neighbors. No sorting or global kind regrouping occurs. |
| Cross-kind order | Each run executes in input order; Activity/Usage never join a neighbor and remain single-element runs. |
| Parameter bound | Each nonempty IN statement binds at most `BATCH_IN_CHUNK = 500` values, with no hidden additional binds and an assertion against 32766. |
| Missing keys | Zero affected rows remain successful, matching the old ignored execute count. |
| Empty deletes/no-op | Empty iteration issues no SQL or ID collection. Source no-op exits before phase 2; no new no-op scan was introduced. |
| Durable intent | Helper is called from the original phase-2 location, after durable phase-1 intent and `verify_pending_in_tx`. Manifest bytes and input vector are untouched. |
| Failure propagation | Every execute error passes through the existing backend mapping and `?`; the caller immediately returns and drops the same outer transaction. No intermediate commit or catch was added. |

The actual schema was checked: all four delete keys are single-column TEXT
primary keys. Placement ordinal/span checks and usage count/source checks do
not make the relative order of same-kind deletions observable. There are no
production DELETE triggers or declared foreign keys on these four tables;
the newly added triggers occur only in tests. Final relational checks remain
in the same enclosing transaction.

The pinned local rusqlite 0.40.2 source sets
`Transaction.drop_behavior = DropBehavior::Rollback`; Drop calls `finish_`,
whose rollback branch invokes `rollback_`. This supports rollback of earlier
successful chunks after a later statement error. No existing validation was
moved or weakened to obtain batching.

## New tests inspected

- `relation_delete_batches_cover_boundaries_and_exact_keys`: covers
  0/1/500/501 keys, one unrelated surviving row and one missing ID. It checks
  actual remaining primary keys, unchanged state for missing IDs, and expected
  statement count including BEGIN/COMMIT. It also pins the schema's absence
  of relevant foreign keys and production triggers.
- `relation_delete_runs_preserve_cross_kind_order`: inserts facts of all four
  kinds and records deletion kinds with test-only triggers. Adjacent
  Activity/Usage rows remain individually executed; mixed runs preserve their
  input kind sequence. It deliberately does not demand row order within IN.
- `relation_delete_late_failure_rolls_back_prior_chunks`: uses a 502-message
  source. A trigger targets the second Placement chunk after the Edge run and
  first 500 Placement deletions, checking that earlier deletion actually
  occurred before raising ABORT. Every table except the additional durable
  building intent must match its pre-call snapshot; generation stays unchanged.
- `relation_delete_manifest_tamper_fails_before_deletes`: corrupts persisted
  relation-delete detail after intent creation, then requires the existing
  durable-intent error, zero Edge/Placement DELETE statements and an unchanged
  full snapshot.

The existing O2 full-snapshot helper was moved into shared test scope for
reuse; its data coverage was not reduced. Placement test wire IDs have the
required 32 hexadecimal digits. No new tests were run by the reviewer.

Main reports all four targeted tests passed and that adapter regressions plus
fmt/clippy are running. These are attributed progress reports, not yet an
independently audited final gate.

## Verification

- Static production delta, key mapping, schema and Drop behavior: reviewed.
- Static test design: reviewed; no pre-timing blocker found.
- Lint / TypeCheck: pending final result handoff for O7.
- Full tests / strict oracle: pending new-candidate results.
- A/B and formal R4: pending; 190 rather than 95000 relation-delete statements
  is structural arithmetic, not a measured speedup.

At final handoff, bind the exact candidate source/binary and recheck the full
range plus final tests. Preserve S2 failure and all O7 attempts. No timing
conclusion follows from the aggregate `relation_upserts` stage because it
includes deletions and upserts; the existing zero `catalog_deletes` timer also
does not establish zero real delete cost.
