# Structured activity and context facets — Design (slice R1+R2)

> Parent task: `08-15-structured-activity-context-facets`.
> This design covers slice R1+R2 ONLY: `ToolActivity` model, provider extraction,
> adapter projection (schema v12), and facet filters on search.
> Session-metadata search (R3, schema v11) is delivered separately on branch
> `session-metadata-search-08-15`; it is NOT reimplemented here.

## Context

Retrieval today is message-text-only: `SearchIndex::query(query, limit)` runs one
FTS5 MATCH with no facet predicates. Structured tool activity is not modeled in
the domain, not extracted by the providers, and not stored. This slice adds:

1. A domain `ToolActivity` model with a **fail-closed, ordered rule set** for
   kind inference and target extraction (below).
2. Provider extraction from Claude Code `tool_use`/`tool_result` content blocks
   and Codex `custom_tool_call`/`function_call_output` response items, emitted
   as typed observations alongside message emission.
3. An additive schema migration (v7 → v12 in this worktree; next free version
   after the metadata-search branch's v11) with a `tool_activities` projection
   table + per-source claims, mirroring the v7 `message_placements` lifecycle
   (complete-scan replace, incomplete-scan union, tombstone via claims).
4. Facet filters on search (sidechain facet + tool kind/name), indexed, with
   default behavior byte-identical to current.

## The ToolActivity model (domain)

```rust
pub enum ToolActivityKind { File, Command, Web, Query, Unknown }
pub enum ToolActivityActor { Main, Subagent }
pub enum ToolActivityStatus { Success, Error, Unknown }

pub struct ToolActivity {
    pub kind: ToolActivityKind,
    pub actor: ToolActivityActor,
    pub name: String,              // provider tool name, verbatim
    pub target: Option<String>,    // extracted target; None when unknown
    pub status: ToolActivityStatus,
}
```

All enums serialize to `snake_case` wire values. `ToolActivity` carries a
`validate()` invariant check (name non-empty, bounded later at the persistence
boundary).

## Ordered rule set (fixes the PRD's Recall/ctx references concretely)

The PRD left kind inference and target extraction as references to Recall's
`events.rs` and ctx's `provider_policy_event_text`. The rule set below is the
concrete fix, written down as explicit ordered rules. **Deterministic,
provider-recorded-first, fail-closed: unknown → `kind = Unknown`,
`target = None`, never guessed.**

**Borrowings (MIT, attribution recorded):**

- Target key set/order borrows from Recall `src/adapters/events.rs:94`
  (`target_from_value`: `path/file_path/filePath/target/command/cmd/query/
  pattern/glob/glob_pattern/regex/url`), with two documented adjustments:
  `url` moved before `query` (our Web tools' inputs carry only `url`), and
  `description` appended (Task-class tools). Two deliberate deviations:
  string values only (Recall also parses `bash -c …` argument arrays — that
  shape does not occur in Claude/Codex records; fail-closed we do not guess),
  and case-sensitive key matching (provider-recorded exact field names).
- Kind inference stays **exact-name matching** on the closed set below. This
  deliberately deviates from Recall's `infer_tool_kind` (lowercase containment,
  e.g. `bash` matches `notbash`): exact matching is stricter fail-closed for
  the two in-scope providers, which emit exact known names.
- Facet surface aligns with claude-historian-mcp's context facets
  (`filesReferenced` → kind=file, `toolsUsed` → name, `bashCommands` →
  command kind) — inspiration only, no code borrowed.
- memex's `prune_and_merge_tool_calls` (compact/service.rs) was reviewed; it
  is compaction-tier merging (summarization), out of scope for extraction —
  noted as a follow-up, not borrowed.

### R1 — Target extraction priority chain (first present wins)

Given a provider-recorded tool input object, take the FIRST key present with a
non-empty (after trim) string value; all keys are the provider's own field
names:

1. `path`
2. `file_path`
3. `filePath`
4. `target`
5. `command`
6. `cmd`
7. `url`
8. `query`
9. `pattern`
10. `glob`
11. `glob_pattern`
12. `regex`
13. `description`
14. none present / all empty / non-string values → `target = None` (never
    guessed; array values are skipped, not joined)

### R2 — Kind inference (ordered; name is provider-recorded, first match wins)

| Order | Tool name (verbatim) | Kind | Target source |
|---|---|---|---|
| 1 | `Bash`, `shell`, `exec` | `command` | chain |
| 2 | `Read`, `Write`, `Edit`, `MultiEdit`, `NotebookEdit`, `ApplyPatch` | `file` | chain |
| 3 | `Glob`, `Grep` | `query` | chain |
| 4 | `WebFetch`, `WebSearch` | `web` | chain |
| 5 | `Task` | `query` | chain |
| 6 | any other name (case-sensitive; `BASH`/`read` are unknown) | `unknown` | `None` (never guessed) |

Known names still fall back to the chain; a known name with no usable input
keeps its kind but gets `target = None`. Unknown names are `unknown`/`None`
even when the input carries a look-alike field — the name is the discriminator.

### R3 — Actor rule

`actor = Subagent` iff the CALLER message (the message carrying the
`tool_use`/`custom_tool_call` record) has `is_sidechain == true`; otherwise
`Main`. Codex has no sidechain field → always `Main`.

### R4 — Status rule

1. Result record observed with `is_error == true` → `Error`.
2. Result record observed with `is_error` false or absent → `Success`.
3. Call observed with NO result record in the same source (truncated
   transcript) → `Unknown` (reported at end of parse; never guessed).

### R5 — Attachment (message association) rule

1. Claude: the activity is attached to the message carrying the `tool_result`
   block (the record's own `uuid`). An unpaired `tool_use` at EOF is attached
   to the assistant message that carried the call.
2. Codex: attached to the most recently emitted canonical message at the moment
   the `custom_tool_call` record is observed (the issuing assistant message).
3. If the anchor message was not emitted as a canonical message (skipped or
   non-conversational), the activity is dropped at staging — never fabricated.

### R6 — Fail-closed (opaque records never fabricate activities)

- `tool_result` with no matching `tool_use_id` in the source → skipped silently.
- `function_call_output` with no matching `call_id` → skipped silently.
- Tool records with a missing/empty name → skipped.
- Unknown content-block / payload shapes never produce activities.

## Provider extraction

### Claude Code (`provider-claude`)

`RawBlock` gains `name`, `input`, `id`, `tool_use_id`, `is_error` fields
(additive; serde ignores unknown fields). Parse keeps a per-file map
`tool_use_id -> PendingCall { name, input, caller_uuid, caller_is_sidechain }`:

- `tool_use` block in an assistant message → record pending call.
- `tool_result` block in a user message → resolve pending by `tool_use_id`,
  extract kind/target per R1-R2, status per R4, actor per R3 from the CALLER,
  emit `ToolActivityEvent { message_native_id: current record uuid, activity }`.
- EOF: remaining pending calls emit with `status = Unknown`.

### Codex (`provider-codex`)

`RawPayload` gains `name`, `arguments` (JSON string or object), `tool_call_id`,
`call_id`, `is_error` (additive). Parse keeps
`call_id -> PendingCall` and the last emitted message's native id:

- `response_item`/`custom_tool_call` → record pending call; anchor = last
  emitted message. `tool_call_id` preferred, fallback to `id` (version
  difference in real rollouts).
- `response_item`/`function_call_output` → resolve by `call_id`, emit activity.
- EOF: remaining pending calls emit with `status = Unknown`.

## Ports

- `CanonicalEventSink` gains
  `fn emit_activity(&mut self, event: ToolActivityEvent<'_>) -> PortResult<()>`
  with a **default no-op impl** (existing sinks compile unchanged).
  `ToolActivityEvent { message_native_id: &str, activity: ToolActivity }`
  (owned `ToolActivity`; the provider must allocate the target anyway).
- `extract_tool_activity_target(input: &serde_json::Value) -> Option<String>`
  and `infer_tool_activity_kind(name: &str) -> ToolActivityKind` live in ports
  (canonical input contract), shared by both providers, unit-tested there.
- `SearchFacets { sidechain: SidechainFacet, tool_kind: Option<String>,
  tool_name: Option<String> }` and `SidechainFacet { Include, MainOnly,
  SubagentOnly }` (default `Include`), living in ports (boundary contract).
- `SearchIndex` gains
  `fn query_faceted(&self, query, limit, facets) -> PortResult<Vec<SearchHit>>`
  with a default impl delegating to `query` (fake backends unchanged).

## Application

- `StagedBatch` gains `activities: Vec<StagedActivity>`; `StagingSink`
  buffers `emit_activity` events.
- `AppRequest::Search` gains `facets: SearchFacets`; `App::handle` calls
  `query_faceted`.
- Cursor binding folds facets into the query digest input (same canonical
  string at issue and verify), so a facet change invalidates prior cursors.
- `AppResponse::Search` unchanged (facet echo is a CLI render concern).

## CLI (composition root)

- `staged_to_source` resolves each `StagedActivity.message_native_id` to the
  batch's stable message id (first occurrence in seq order, same rule as
  entity dedup) and produces `SourceBatch.activities:
  Vec<SourceActivity { message_id, activity }>`; unresolvable anchors are
  dropped (R5.3).
- `sync_files`/`ingest` pass the batch through unchanged.
- `search` arm parses `--main-only` / `--subagent-only` / `--include-sidechain`
  (mutually exclusive combo is a usage error) and `--tool-kind <kind>` /
  `--tool-name <name>`. `--tool-kind` is validated against the closed enum.
- `render` echoes `"facets": { sidechain, tool_kind, tool_name }` in the
  search data **only when facets are non-default** (default output stays
  byte-identical).
- New flags registered in every prefix scanner that skips value-bearing flags
  (`command_name`, `extract_request_id`, `intercept_help_or_version`,
  `extract_db_flag_impl`, `bare_positionals`) and in `is_known_flag_name`.
  New `take_bool_flag` helper added (does not exist in this worktree).
- Help texts updated (top-level + `search` subcommand).

## MCP

`search_sessions` gains optional params `sidechain` (enum
`include|main_only|subagent_only`), `tool_kind` (enum
`file|command|web|query|unknown`), `tool_name` (string); `reject_unknown_keys`
and `tool_catalog` inputSchema updated. No new tool.

## Adapter — schema v12 (additive)

This worktree's base is **v7** (`SCHEMA_VERSION = 7`); the next free version
after the metadata-search branch (v11) is **12**. The v12 step is guarded
`if current < 12` and depends only on v7 tables, so it runs cleanly on any
catalog at v7..v11 — merge-safe with the parallel branches.

```sql
CREATE TABLE tool_activities (
    activity_id TEXT PRIMARY KEY,
    message_id  TEXT NOT NULL,
    kind        TEXT NOT NULL,
    actor       TEXT NOT NULL,
    name        TEXT NOT NULL,
    target      TEXT,
    status      TEXT NOT NULL
);
CREATE INDEX tool_activities_message ON tool_activities(message_id);
CREATE INDEX tool_activities_kind    ON tool_activities(kind);
CREATE INDEX tool_activities_name    ON tool_activities(name);
CREATE TABLE tool_activity_membership (
    source_path TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    PRIMARY KEY(source_path, activity_id)
);
CREATE INDEX tool_activity_membership_activity ON tool_activity_membership(activity_id);
PRAGMA user_version = 12;
```

- **activity_id**: content-addressed
  `act_v1_<hex16(blake3("tool-activity-v1" || message_id || kind || actor ||
  name || target || status))>`. Same facts + same anchor derive the same id,
  so cross-source copies dedup; identical facts from two sources produce two
  claims on one row (mirrors `message_placements`).
- **Bounding** (persistence boundary, explicit): `name` truncated to 128 chars,
  `target` to 512 chars (char-based) before id derivation and storage.
  Constants `TOOL_ACTIVITY_NAME_MAX_CHARS` / `TOOL_ACTIVITY_TARGET_MAX_CHARS`
  in the adapter.
- **Rebuild doctrine**: the PRD allows "catalog-rebuildable projection **or**
  additive schema, backward-compatible migration". Activities are an additive
  relation projection (like placements), not catalog-derived; `rebuild_index`
  is unchanged. Documented here per the PRD's additive alternative.

### Commit lifecycle (mirrors placements exactly)

- `SourceBatch` gains `activities: Vec<SourceActivity>`.
- `commit_source_batches_if_changed`:
  - reads `source_activity_membership_state()` + `stored_activities()`;
  - validates/derives per-source activity ids, rejects duplicate ids and
    cross-source fact conflicts (mirror placements/edges);
  - complete scan → replace the source's claims; incomplete scan → union
    (no tombstones);
  - tombstone: prior claimed ids of complete sources not observed and not
    claimed by any surviving source → deleted, only when the row exists;
  - `RelationManifests` gains `Activity` upsert/delete variants; the durable
    outbox intent (relation JSON columns + digest) covers activities;
    the phase-2 transaction writes/deletes activity rows + membership in the
    same transaction as catalog/FTS/generation (no new crash window).
- `sources_are_current` / `source_batches_are_current` compare activity rows
  and claims for the batch (scoped queries, no full-table loads).
- `SourceReplacementManifest` gains `activity_ids` (additive JSON field).

### Facet query path (indexed, no full scans)

`query_faceted` extends the pinned SQL with correlated EXISTS probes (each
backed by an index):

- `main_only`:
  `NOT EXISTS (SELECT 1 FROM fts_ids fi2 JOIN message_placements mp
   ON mp.message_id = fi2.wire_id WHERE fi2.id_json = fts.id
   AND mp.is_sidechain = 1)` — uses the `fts_ids.id_json` UNIQUE index and
  `message_placements_message(message_id)`.
- `subagent_only`: same with `EXISTS` and `is_sidechain = 1`.
- `tool_kind`/`tool_name`: same shape against `tool_activities` via
  `tool_activities_message(message_id)` (+ `tool_activities_kind`/`_name`
  for the predicate column). Exact equality only; no LIKE.

## Compatibility

- `sync`/`ingest`/`search` without new flags: byte-identical behavior and
  output (facets echo omitted when default).
- Schema v12 additive; v7-v11 catalogs migrate forward; older binaries refuse
  a v12 catalog via the existing `current > SCHEMA_VERSION` gate.
- `list`, `context`, TUI, and Robot protocol envelope are unchanged by this
  slice (context sidechain filtering already exists via `--policy mainline`).

## Boundaries / privacy

- Provider transcripts stay read-only; no writes to source files.
- Discovered/stored activity `target` values may be absolute paths from real
  transcripts; they are stored locally (like message text) and never surface
  in diagnostics, progress frames, or error messages. Machine output carries
  counts/ids only (facets echo `{sidechain, tool_kind, tool_name}`, never
  targets).
- Synthetic fixtures only; no real personal paths/hostnames/identities.

## Deferred (out of this slice)

- Facet filters on `list` (wire-id listing); TUI facet controls; Robot 1.1
  `retrieval_mode`-style capability declaration; activity text-tier preview
  policy (PRD req 2 text-retention tiers); memex-style compaction-tier tool
  call merging — follow-up tasks.
