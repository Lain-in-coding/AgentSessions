# Authoritative handoff audit

## State verified on 2026-07-28

- `git status --short` was clean before planning; branch is
  `chore/batches-1-3-governance-and-evidence`, HEAD is `5369ebb`, and
  `git rev-list --left-right --count 'HEAD...@{upstream}'` returned `0 0`.
- `task.py list --mine` returned zero because both pre-existing active tasks are
  assigned to `repository-owner`, not QIN.
- `task.py list --json` identifies the two active tasks as a parent chain:
  `07-24-remediation-milestone-closure` (planning, parent=null) contains
  `07-24-advance-integration-beta` (planning), whose eight implementation
  children are already archived. The new child 9 is
  `07-28-relational-message-placement`.

## Blocking evidence

- Parent PRD records all eight children archived and leaves canonical
  source/session/thread/branch/span round-trip and parent integration review
  open (`.trellis/tasks/07-24-advance-integration-beta/prd.md:63-129`).
- Archived child 8 records four layers and deliberately stops at divergent
  `parentUuid`, because `select_mainline` consumes one parent chain
  (`.trellis/tasks/archive/2026-07/07-27-cross-source-session/design.md:165-195`).
- The authorized real-data record reports four runs progressing from 0 to 6767
  to 7060 committed messages, then still failing `INV-SYNC-OK` with exit 6;
  the five downstream invariants are not evaluated
  (`docs/evidence/integration-beta/real-data-regression.md:30-59`).
- The evidence record attributes the fourth conflict to per-context parentage
  and explicitly rejects an arbitrary parent union
  (`docs/evidence/integration-beta/real-data-regression.md:61-96`).
- Both providers remain Experimental and promotion gaps 4 and 5 stay open
  (`docs/product/PROVIDER-MATURITY-MATRIX.md:20-31,70-93`).
- Evidence rows preserve the distinction between a locally verified process and
  a failed corpus outcome; `IB-SESSION-CROSS-SOURCE-001` remains only partially
  fixed (`docs/operations/core-beta-evidence-matrix.md:38-40`).

## Model and contract facts

- Draft RFC-0001 says logical identity and physical position are decoupled
  (`docs/architecture/RFC-0001-canonical-model-and-stable-id.md:31-36`).
- RFC §3.2 defines Message plus an independent MessageEdge relation
  (`docs/architecture/RFC-0001-canonical-model-and-stable-id.md:74-123`).
- The RFC still places one `session_id/thread_id/branch_id` on Message. Real
  cross-session reuse proves the implementation must go further: those values
  cannot be intrinsic fields of one globally stable native-message entity.
  This is an evidence-driven implementation correction while the RFC remains
  Draft, not an acceptance or status change.
- The shared interface contract requires every frontend to map to one
  Application ADT and defines evidence with exact `source_document_id`
  (`docs/contracts/CONTRACT-cli-robot-mcp-draft.md:9-27,63-68`).

## Current implementation confirmed directly

- Domain Message currently owns `parent`, `seq`, `is_sidechain`, and `span`;
  Session owns one `document_id` and a vector of Messages
  (`crates/agentsessions-domain/src/lib.rs:36-78`).
- `select_mainline` chooses the highest-sequence non-sidechain leaf and walks
  each Message's singular `parent`; `select_full` sorts by Message `seq`
  (`crates/agentsessions-domain/src/thread.rs:24-82`).
- `CatalogStore` exposes only generic get/put/list/count/generation operations;
  it cannot load a session relation graph
  (`crates/agentsessions-ports/src/lib.rs:78-96`).
- Provider events carry native message id, native parent id, ordinal,
  sidechain flag, and per-snapshot span; the session native id is report-level
  (`crates/agentsessions-ports/src/lib.rs:220-276`).
- Application `handle_context` reads the session payload, loads message payloads,
  reconstructs singular parents from the global message rows, and applies
  domain selection (`crates/agentsessions-application/src/lib.rs:503-638`).
- The same handler uses the session's singular `document` alias for every
  selected message's evidence, which is insufficient for cross-document spans
  (`crates/agentsessions-application/src/lib.rs:530-541,652-665`).
- SQLite schema is v6. `source_membership` records `(source_path, message_id,
  document_id?)`; the existing v5→v6 migration keeps old document attribution
  NULL rather than fabricating it
  (`crates/agentsessions-adapters-sqlite/src/lib.rs:530-572,1318` and
  `docs/operations/migration-v5-to-v6.md:4-35`).

## Planning consequences

1. A stable message entity must exclude context-owned parent, ordinal,
   sidechain, span, session, and document fields.
2. Context selection needs a typed backend-independent relation projection, not
   ad-hoc JSON parsing or direct SQL in CLI/MCP/TUI.
3. A new additive SQLite migration is expected because generic catalog payloads
   and current source membership cannot query contextual parent graphs safely.
4. Compatibility aliases may remain readable/emitted during migration, but
   branch/evidence correctness must use relational placement data.
5. Tests must include one native message reused across sessions/documents with
   different parents, because current one-file/one-session fixtures cannot
   expose the defect.
