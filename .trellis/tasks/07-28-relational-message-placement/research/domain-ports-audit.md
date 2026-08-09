# Domain and ports audit: relational message placement

- **Query**: Audit the canonical model, RFC-0001, branch selectors, and ports to identify stable Message fields, required contextual relation types, and port impact.
- **Scope**: Internal source/spec audit only. No provider transcripts were accessed.
- **Date**: 2026-07-28

## Conclusions

1. The stable canonical message entity in the current slice should own **`id`, `role`, normalized `text`, and provider-reported `timestamp`**. `StableId` carries kind/stability/value in memory; the provider-native message id is represented by `Message.id`, not by a separate domain field (`crates/agentsessions-domain/src/ids.rs:80-91`, `crates/agentsessions-domain/src/ids.rs:99-122`). Role/text/timestamp are intrinsic canonical projection fields, not necessarily inputs to the stable-id algorithm.
2. **`parent`, `seq`, `is_sidechain`, `span`, session membership, and document membership are placement facts**, not stable Message fields. The active PRD explicitly requires session/source/document, parent, span, and branch position to be contextual (`.trellis/tasks/07-28-relational-message-placement/prd.md:7-10`, `:31-46`). Current real-data evidence reaches the same split: identity is shared; session, parent, and byte position vary by source (`docs/evidence/integration-beta/real-data-regression.md:70-89`).
3. The current domain model has `EvidenceSpan`, `Message`, and `Session`, but **no `MessageEdge`, message-placement/session-membership type, `ConversationThread`, or `ConversationBranch` implementation**. The only implemented canonical types are listed by the domain spec (`.trellis/spec/agentsessions-domain/backend/index.md:9-21`), and source search finds no relation/edge struct in the product crates.
4. RFC-0001 is **Draft**, not Accepted (`docs/architecture/RFC-0001-canonical-model-and-stable-id.md:2-16`). It does declare `ConversationThread`, `ConversationBranch`, and `MessageEdge` (`:93-123`), but its current `Message` still contains singular `session_id`, `thread_id`, `branch_id`, and `branch_local_ordinal` (`:106-117`). Its `MessageEdge` has only parent, child, and relation (`:119-123`), with no session/document/placement context. Therefore the RFC's literal field set is insufficient for the active task's one-message/many-context cardinality and context-dependent parent edges.
5. The required semantic domain relations are:
   - a **contextual message placement/session-membership relation** keyed by at least message + session + source document (or by an opaque placement id that resolves those), carrying branch-local ordinal/`seq`, sidechain/thread/branch position, and an exact evidence occurrence;
   - a **context-scoped `MessageEdge`** whose child is a placement (or whose key otherwise includes the same session/document context), plus parent message/placement and an edge-relation value;
   - an **edge-relation enum** covering RFC values `reply | retry | fork | continuation | subagent | tool_result`;
   - `EvidenceSpan` retained as the range value, but associated through the exact placement/document relation rather than stored on stable `Message`;
   - `ConversationThread`/`ConversationBranch` domain identifiers/entities if the RFC's explicit thread/branch identifiers are to be returned. The current code returns only `branch_leaf`, not a thread or branch id (`crates/agentsessions-application/src/lib.rs:106-120`).
6. A single ternary placement relation `(session, document, message occurrence)` supplies both message-in-session and session-across-document cardinality. If empty session/document occurrences must survive with no messages, that association additionally needs a session-document relation; the current session payload records documents independently of messages (`crates/agentsessions-cli/src/main.rs:849-871`).
7. **A backend-independent context-relation read capability must be added to the ports boundary**—either by expanding `CatalogStore` or by introducing a dedicated context/placement store port. Existing `CatalogStore` only supports `get`/`put`/`list`/`count`/`active_generation` by entity id (`crates/agentsessions-ports/src/lib.rs:78-96`). Once placements and edges are no longer embedded in one session/message payload, `App::handle_context` cannot load them through `get(id)` alone. The required query is scoped by requested session/context and must return domain relation DTOs, not SQLite rows or JSON details, matching the port rules (`.trellis/spec/agentsessions-ports/backend/index.md:19-32`).
8. **`CanonicalEventSink` does not require a new method, and `MessageEvent` already carries the raw facts needed to construct relations**: `seq`, native message id, native parent id, role/text/timestamp, sidechain, and source-snapshot span (`crates/agentsessions-ports/src/lib.rs:236-276`). `ParseReport.session_native_id` supplies session identity at batch level (`:220-234`), and staging combines that report with buffered events (`crates/agentsessions-application/src/lib.rs:151-185`, `:247-266`). The mapping must change: contextual event fields should populate placements/edges/evidence, not stable `Message`. The only contract information not currently emitted is an explicit RFC edge-relation classification; current providers expose a parent link and sidechain flag, not `reply/retry/fork/...` as a field.

## Current canonical-model evidence

### `Message`, `Session`, and `EvidenceSpan`

- `EvidenceSpan` is only `{ start, end }`; it has no document or placement identity (`crates/agentsessions-domain/src/lib.rs:25-34`).
- Current `Message` fields are `id`, `parent`, `role`, `text`, `seq`, `timestamp`, `is_sidechain`, and `span` (`crates/agentsessions-domain/src/lib.rs:36-69`). Four of those—parent, seq, sidechain, span—are consumed as topology/placement facts.
- Current `Session` embeds singular `document_id` plus `Vec<Message>` (`crates/agentsessions-domain/src/lib.rs:71-78`). This cannot represent one stable message once with multiple contextual placements, and its singular document field does not match the already-persisted multi-document payload.
- `Session::validate` assumes messages are embedded in vector order (`m.seq == index`) and validates parent/span on each message (`crates/agentsessions-domain/src/lib.rs:80-130`). Those invariants currently belong to the embedded projection, not to an independent stable Message entity.
- The persisted payload already demonstrates the split that the domain does not model: message JSON carries singular and plural session/span aliases plus parent/seq-derived placement data (`crates/agentsessions-cli/src/main.rs:800-845`), while session JSON carries `documents` and ordered member ids (`:849-871`).

### RFC-0001 actual shape

- RFC identity/location rule: physical root/path/locator must not enter preferred Session/Message logical identity (`docs/architecture/RFC-0001-canonical-model-and-stable-id.md:31-36`).
- RFC topology declarations: Session, Thread, Branch, Message, and MessageEdge are specified at `docs/architecture/RFC-0001-canonical-model-and-stable-id.md:74-123`.
- RFC source evidence already couples span coordinates to `source_document_id` through `SourceSpan` (`docs/architecture/RFC-0001-canonical-model-and-stable-id.md:133-162`). The implemented domain `EvidenceSpan` lacks that association.
- RFC branch rule says `show`/context return Thread/Branch identifiers and branch traversal follows parent/child paths (`docs/architecture/RFC-0001-canonical-model-and-stable-id.md:164-168`).
- RFC stable-id priority makes provider-native Message/Event id primary, then reconstructed identity (`docs/architecture/RFC-0001-canonical-model-and-stable-id.md:181-203`).

## Branch-selection evidence

- `select_mainline(&[Message])` selects the highest-`seq` non-sidechain leaf, then follows each message's single `parent` backward; its result depends exactly on `seq`, `parent`, and `is_sidechain` (`crates/agentsessions-domain/src/thread.rs:24-72`).
- `select_full(&[Message])` sorts all messages by `seq` (`crates/agentsessions-domain/src/thread.rs:74-82`).
- The selector input is therefore currently a **placement projection disguised as `Message`**. With a stable Message stripped of contextual fields, selectors need a context-scoped placed-message graph/projection; their deterministic policies can then operate unchanged on that selected graph.
- `App::handle_context` currently reads the session's embedded `messages`, reconstructs `Message.parent` from singular payload key `parent`, `seq` from member-array index, and `span` from singular key `span`, then invokes the selectors (`crates/agentsessions-application/src/lib.rs:503-638`).
- Evidence assembly also reads singular `session.document` and singular `message.span` aliases (`crates/agentsessions-application/src/lib.rs:530-541`, `:652-665`; `crates/agentsessions-application/src/evidence.rs:51-102`). Those aliases are explicitly only one contributor after merges (`crates/agentsessions-adapters-sqlite/src/lib.rs:172-198`, `:266-275`), so they cannot satisfy exact contextual selection.

## Port and persistence evidence

- `CatalogStore` is a generic entity key/value interface; it has no query for session placements, session-document membership, or context edges (`crates/agentsessions-ports/src/lib.rs:78-96`).
- The previous child explicitly avoided adding a reverse-membership port and documented that doing so would require a `CatalogStore` method, blanket impl, test stores, and the Application read path (`.trellis/tasks/archive/2026-07/07-27-cross-source-session/design.md:8-24`). The active task now requires precisely that backend-independent contextual lookup (`.trellis/tasks/07-28-relational-message-placement/prd.md:42-47`).
- SQLite schema v6 has only generic `catalog`, FTS/outbox metadata, and source membership tables; no placement or edge table exists (`crates/agentsessions-adapters-sqlite/src/lib.rs:457-570`; schema version at `:1317-1318`).
- Current merge logic treats only `session`/`sessions` and `span`/`spans` as unionable and still rejects different parent projections (`crates/agentsessions-adapters-sqlite/src/lib.rs:95-199`). That behavior is consistent with the absence of a context edge relation.

## Files inspected

- `crates/agentsessions-domain/src/lib.rs`
- `crates/agentsessions-domain/src/ids.rs`
- `crates/agentsessions-domain/src/thread.rs`
- `crates/agentsessions-ports/src/lib.rs`
- `crates/agentsessions-application/src/lib.rs`
- `crates/agentsessions-application/src/evidence.rs`
- `crates/agentsessions-cli/src/main.rs`
- `crates/agentsessions-adapters-sqlite/src/lib.rs`
- `docs/architecture/RFC-0001-canonical-model-and-stable-id.md`
- `docs/contracts/CONTRACT-cli-robot-mcp-draft.md`
- `.trellis/spec/agentsessions-domain/backend/index.md`
- `.trellis/spec/agentsessions-ports/backend/index.md`
- `.trellis/spec/agentsessions-application/backend/index.md`
- `.trellis/tasks/07-28-relational-message-placement/prd.md`
- `.trellis/tasks/archive/2026-07/07-27-cross-source-session/design.md`
- `docs/evidence/integration-beta/real-data-regression.md`
- `docs/product/PROVIDER-MATURITY-MATRIX.md`

## Caveats

- The RFC is Draft and its current `Message`/`MessageEdge` cardinalities do not fully express the active task; no governance status change was made.
- This audit identifies required semantics, not frozen Rust type names or SQL schema names.
- No product code, provider transcript, or git state was modified or accessed.
