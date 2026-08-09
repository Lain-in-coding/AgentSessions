# Application context-flow audit

## Verified findings

1. Domain `Message` currently mixes stable identity/content with contextual
   `parent`, `seq`, `is_sidechain`, and `span`; `Session` has one
   `document_id` (`crates/agentsessions-domain/src/lib.rs:36-78`).
2. `staged_to_entries` derives a stable Message ID from provider-native identity
   but writes parent, sidechain, session aliases, and span aliases into the same
   global message payload. Session payloads contain document aliases and a
   de-duplicated Message ID list
   (`crates/agentsessions-cli/src/main.rs:772-872`).
3. SQLite cross-source merging unions sessions and per-document spans, but all
   remaining fields must match. A divergent parent or sidechain value therefore
   still conflicts. De-duplicating a session's members by Message ID also loses
   the correspondence between a member and its source/document placement
   (`crates/agentsessions-adapters-sqlite/src/lib.rs:94-275`).
4. `handle_context` reads compatibility aliases rather than contextual
   relationships: one session `document`, one Message `parent`, one Message
   `is_sidechain`, and one Message `span`; sequence is reconstructed from the
   session member-array index. It applies branch selection only after this
   global projection is built
   (`crates/agentsessions-application/src/lib.rs:502-637`).
5. `select_mainline` and `select_full` can remain pure. Their input must first be
   scoped to one requested session/context and populated with that context's
   ordinal, sidechain, and parent edge
   (`crates/agentsessions-domain/src/thread.rs:24-82`).
6. Evidence currently uses one session-level document for every selected
   message, while offsets come from one Message-level span. Occurrence IDs are
   derived from Message wire ID plus ordinal, so the same stable Message at the
   same ordinal in distinct contexts can collide and cannot prove which
   placement owns the span
   (`crates/agentsessions-application/src/lib.rs:529-540,651-679`;
   `crates/agentsessions-application/src/evidence.rs:28-92`).
7. Ports expose only generic raw-payload CatalogStore operations, and SQLite v6
   `source_membership(source_path, message_id, document_id)` lacks session,
   placement identity, contextual ordinal, sidechain, span, and parent edge.
   Application therefore has no backend-independent context-graph read
   capability (`crates/agentsessions-ports/src/lib.rs:71-114`;
   `crates/agentsessions-adapters-sqlite/src/lib.rs:529-571,1317-1378`).
8. The required flow is: ingest stable Message separately from placement/edge;
   Application asks a typed port for one session's stable messages, placements,
   exact source/document spans, and contextual edges; Application builds the
   local graph, invokes pure branch selection, then assembles evidence from the
   selected placement. Compatibility aliases may remain views but cannot be the
   authoritative read source.
9. CLI and MCP already construct `AppRequest::Context` and share response
   rendering. TUI also loads context through the ADT, but its search-hit path
   first calls `Show` and parses the Message payload's singular `session` alias.
   That choice becomes incomplete for shared messages and must move into the
   Application ADT (`crates/agentsessions-cli/src/main.rs:618-643,984-1111`;
   `crates/agentsessions-cli/src/mcp.rs:252-275,311-321`;
   `crates/agentsessions-cli/src/tui/mod.rs:164-218`).
10. `AppResponse::Context.messages` currently identifies items only by Message
    ID and payload, while TUI keys evidence by Message ID. The shared response
    needs a placement/occurrence identity on both context message items and
    evidence so multiple contextual occurrences do not collapse
    (`crates/agentsessions-application/src/lib.rs:106-120`;
    `crates/agentsessions-cli/src/tui/mod.rs:221-247`).

## Design consequence

Application remains the only business boundary. No frontend reads SQL or
chooses a compatibility alias. The port returns a typed session-scoped graph;
Domain owns pure validation/selection; Application owns branch policy, budget,
evidence, and contextual response identity; all frontends project the same ADT.
