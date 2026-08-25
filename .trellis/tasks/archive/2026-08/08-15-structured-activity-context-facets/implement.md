# Structured activity and context facets — Implementation (slice R1+R2)

> Follows `design.md` in this task dir. Implemented in an isolated worktree
> based on main `f4175a0` (schema v7). Do NOT touch session-metadata-search
> work.

## Steps

### 1. Domain — ToolActivity model

- `crates/agent-session-grep-domain/src/lib.rs`: add `ToolActivityKind`
  (file/command/web/query/unknown), `ToolActivityActor` (main/subagent),
  `ToolActivityStatus` (success/error/unknown), `ToolActivity { kind, actor,
  name, target, status }` with `validate()`; `as_str()` on the enums;
  wire-value stability test.

### 2. Ports — events and facets

- `crates/agent-session-grep-ports/src/lib.rs`:
  - `ToolActivityEvent { message_native_id: &str, activity: ToolActivity }`;
  - `CanonicalEventSink::emit_activity` (default no-op impl);
  - `SearchFacets` + `SidechainFacet` (default Include) + canonical binding;
  - `SearchIndex::query_faceted` (default delegates to `query`).

### 3. Providers — extraction

- `crates/agent-session-grep-provider-claude/src/lib.rs`: extend `RawBlock`
  with `name`/`input`/`tool_use_id`/`is_error`; pending-call map; emit
  activities per design R1-R6; unit tests (file/command/web/query kinds,
  error/success/unknown status, unpaired call, orphan result, subagent actor).
- `crates/agent-session-grep-provider-codex/src/lib.rs`: extend `RawPayload`
  with `name`/`arguments`/`tool_call_id`/`call_id`/`output`/`is_error`;
  custom_tool_call/function_call_output pairing; unit tests.

### 4. Application — staging + facets

- `crates/agent-session-grep-application/src/lib.rs`: `StagedActivity` +
  `StagedBatch.activities`; `StagingSink::emit_activity`; `AppRequest::Search`
  gains `facets`; handle calls `query_faceted`; cursor digest folds facets.
- Update all `AppRequest::Search` constructions in tests to pass
  `facets: SearchFacets::default()`.

### 5. Adapter — schema v12 + commit lifecycle + facet query

- `crates/agent-session-grep-adapters-sqlite/src/lib.rs`:
  - `SCHEMA_VERSION = 12`; `if current < 12` migration step (tool_activities
    + tool_activity_membership + indexes + `PRAGMA user_version = 12`);
  - `SourceActivity { message_id, activity }`; `SourceBatch.activities`;
  - activity_id derivation + bounding constants (128/512 chars);
  - `RelationUpsertManifest::Activity` / `RelationDeleteManifest::Activity`;
    `SourceReplacementManifest.activity_ids`; manifest JSON + validate;
  - commit: membership state read, conflict checks, final claimers,
    tombstones, apply SQL, `sources_are_current` +
    `source_batches_are_current` comparisons;
  - `SearchIndex::query_faceted` (EXISTS probes, indexed).
- Adapter tests: commit + tombstone + cross-source dedup + facet queries
  (main_only/subagent_only/tool_kind/tool_name) + migration v7→v12.

### 6. CLI — flags, echo, staging resolution

- `crates/agent-session-grep-cli/src/main.rs`: `take_bool_flag` helper;
  search arm facet flags with validation; `staged_to_source` activity anchor
  resolution into `SourceBatch.activities`; render facets echo (non-default
  only); register new flags in prefix scanners + `is_known_flag_name`;
  help texts; unit tests.
- `crates/agent-session-grep-cli/src/mcp.rs`: search_sessions params
  `sidechain`/`tool_kind`/`tool_name` + schema; reject_unknown_keys.
- `crates/agent-session-grep-cli/tests/e2e.rs`: golden e2e tests:
  - claude fixture with tool_use/tool_result → sync → search facet filters;
  - codex fixture with custom_tool_call/function_call_output;
  - default search unchanged; re-sync convergence; tombstone removes
    activities.

### 7. Docs in main repo task dir

- `design.md` (done), `implement.md` (this file), `check.jsonl` entry.

## Validation

```
CARGO_TARGET_DIR=<repo>/target-08-15-structured-activity
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Review gates

- No absolute transcript path in any result/error/progress frame.
- Migration additive, non-destructive, gated on user_version.
- Facet predicates use indexed columns only.
- Default search output byte-identical.
- Do NOT commit/push.
