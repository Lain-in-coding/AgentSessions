# Resume Metadata core and surfaces — Design

## Boundaries

- Touches `ports` (metadata structs), `adapters-sqlite` (claims table + batched read), `application` (resolver + search availability), `cli` (human table, Robot/MCP, TUI, tool list), and the Skill doc.
- Depends on: provider observation (provider task), namespaced identity (identity task), MCP/cursor hardening (protocol task).

## Persistence

New source-scoped claims table (conceptual):

```text
source_session_resume_claims
- source_path            (PK, alongside session_id)
- session_id             (canonical ses_v1_, PK)
- provider_id
- provider_session_id    (nullable)
- provider_session_id_state  (missing|resolved|ambiguous)
- original_working_directory (nullable)
- original_working_directory_state (missing|resolved|ambiguous)
- pair_observed          (bool)
```

- Written during the same transaction/generation as source replacement; cleared when the source is removed.
- Legacy catalogs without the table return `resume_available:false` / `null` until a re-sync backfills (metadata projection revision guard forces backfill even when bytes are unchanged).

## Resolution

- Batched read path: one query per page resolving all hit Session IDs (`session_of` style chunked IN) — no N+1.
- Aggregation order-independent; fails closed on conflict; a Session's normal directory change uses the earliest authoritative cwd, not a conflict.

## Application contract

```rust
// AppRequest
GetSessionResume { session_id: StableId }

// AppResponse
SessionResume {
    session_id: StableId,
    provider_id: Option<String>,
    resume_available: bool,
    provider_session_id: Option<String>,
    original_working_directory: Option<String>,
    unavailable_reason: Option<String>,
}
```

- Search hits gain `resume_available` (batched, per page).
- All new fields charged to JSON-escaped serialized `max_response_bytes`.

## Surfaces

- Human CLI: horizontal table `日期 | Provider | 会话标题 | 工作目录 | Session ID`; date `YYYY-MM-DD` local; Provider/Session ID never truncated; title end-truncate; cwd middle-collapse (`C:/…/name`); newlines/tabs → spaces; missing `—`.
- TUI: adaptive columns + detail panel; same field semantics.
- Robot/MCP: full structured values, no Markdown.
- MCP: new read-only `get_session_resume`; validate `ses_v1_*` kind; update tool list/descriptions (7 → 8).
- Skill: render MCP/Robot results as the same table; document two-step auto-resolution.

## Disclosure safety

- No Source path in ordinary responses. Metadata never written into FTS text, opaque Session payloads, diagnostics, progress, or errors.

## Tests

- Persistence round-trip; atomic replace; removal clears claims; backfill on unchanged bytes; batched no-N+1 (statement-count bound); budget byte accounting; ambiguity → `—`; human table width; MCP kind validation; privacy (no path leak).
