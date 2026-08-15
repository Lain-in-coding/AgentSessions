# Research: Claude provider and provider-source discovery audit

- **Query**: Audit Claude provider parser/probe/source discovery, metadata extraction, prefilter, provider-scoped/session identity, and malformed JSON behavior; report completed items and P0/P1 regression risks with file:line evidence.
- **Scope**: internal
- **Date**: 2026-08-15
- **Worktree inspected**: `C:/AgentSessions/.claude/worktrees/agent-a341e3add01961c60`

## Findings

### Files Found

| File Path | Description |
|---|---|
| `crates/agent-session-grep-provider-claude/src/lib.rs` | Claude Code JSONL `ClaudeCodeAdapter`, probe, parser, metadata/content extraction, and unit tests. |
| `crates/agent-session-grep-application/src/lib.rs` | `stage`, `select_and_stage`, staging sink, probe selection, and `StagedBatch`/`ParseReport` handoff. |
| `crates/agent-session-grep-cli/src/main.rs` | Provider registry, source staging-to-entity conversion, explicit `sync_files`, capture/verify pipeline, and session/message identity derivation. |
| `crates/agent-session-grep-adapters-sqlite/src/source_fs.rs` | Read-only source capture, BLAKE3 snapshot verification, and test-only `SnapshotFs` discovery implementation. |
| `crates/agent-session-grep-ports/src/lib.rs` | `SourceDiscovery`, `ProviderAdapter`, `MessageEvent`, `ParseReport`, and `SourceSnapshot` contracts. |
| `.trellis/tasks/08-14-provider-source-auto-discovery/prd.md` | Active task requirements for explicit `sync --discover`, root completeness, read-only sources, and convergence. |
| `.trellis/tasks/08-14-provider-source-auto-discovery/design.md` | Current design decisions D1-D5 for CLI-local root resolution, path enumeration, tombstone diffing, schema/provider namespace, and completeness reporting. |

### Completed audit items

1. **Claude parser and metadata/content extraction are present.** `RawLine` extracts `type`, `uuid`, `parentUuid`, `timestamp`, `isSidechain`, `isMeta`, `sessionKind`, `sessionId`, and optional `message` at `crates/agent-session-grep-provider-claude/src/lib.rs:41-75`. `RawContent::to_plain_text` handles string and block-array content, joins text-bearing blocks with `\n`, recognizes the constrained local-command envelope, and applies the exact meta-prompt shape at `:141-175`; helper predicates are at `:178-207`.

2. **Claude probe and malformed-line behavior are implemented and covered.** `ClaudeCodeAdapter::probe` samples up to 16 nonblank lines at `:270-289`, tolerates at most 3 bad sampled lines when at least one JSON line parses, and degrades confidence at `:328-380`; beyond tolerance or with zero parsed JSON it returns `ProviderError::AmbiguousVariant` with bounded line detail at `:332-339`. `parse` skips malformed lines rather than aborting, increments `ParseReport.skipped`, and distinguishes invalid JSON from valid JSON with unsupported shape at `:424-442`. Conversational records without `message` are recoverable skips at `:464-470`; event-only `system` records are ignored by `is_conversational` at `:209-217` and `:459-462`. Regression tests cover these behaviors at `:594-701`, `:843-884`, and `:987-1036`; property coverage is in `tests/properties.rs:508-533`.

3. **Native message/session identity and threading handoff are present.** Claude emits `uuid` as `MessageEvent.native_id`, normalizes empty `parentUuid` to `None`, and forwards role, text, timestamp, sidechain, and source span at `crates/agent-session-grep-provider-claude/src/lib.rs:473-502`. The first non-empty `sessionId` becomes `ParseReport.session_native_id`, while distinct IDs are collected and reported as a diagnostic without changing first-session ownership at `:445-457` and `:508-525`. Application staging preserves the report at `crates/agent-session-grep-application/src/lib.rs:273-300`. CLI conversion uses `StableId::native(IdKind::Message, ...)` for non-empty message native IDs and `StableId::native(IdKind::Session, sid)` for a non-empty provider session ID, with document-derived reconstructed session fallback at `crates/agent-session-grep-cli/src/main.rs:1328-1365`.

4. **Existing source prefilter/incremental pipeline is explicit-path only.** `sync` currently passes `&rest[1..]` directly to `sync_files` at `crates/agent-session-grep-cli/src/main.rs:1001-1017`; `sync_files` captures each given path, compares cached BLAKE3 fingerprints, skips unchanged non-empty files, stages changed files, verifies snapshots, and commits one source batch at `:1581-1700`. `SourceDiscovery` is only a port contract at `crates/agent-session-grep-ports/src/lib.rs:60-71`; the only implementation found is `SnapshotFs`, which returns pre-supplied snapshots rather than walking provider roots, at `crates/agent-session-grep-adapters-sqlite/src/source_fs.rs:93-117`.

5. **Read-only and completeness primitives exist, but provider auto-discovery is not present in the inspected checkout.** `capture` records length/mtime/fingerprint and `verify_snapshot` rejects length, mtime, or content-fingerprint drift at `crates/agent-session-grep-adapters-sqlite/src/source_fs.rs:23-90`. Tombstone derivation is guarded by `SourceBatch.relation_complete` in the SQLite commit path; however, a deleted path cannot currently enter the explicit `sync_files` pipeline because `capture` fails when the file is missing. The active task design records the intended discovery-only additions: CLI-local roots `~/.claude/projects` and `~/.codex/sessions`, recursive `.jsonl` enumeration, prior-path diffing, and complete-scan-only synthetic empty batches (`.trellis/tasks/08-14-provider-source-auto-discovery/design.md:27-90`). No `--discover`, provider-root resolver, directory walker, or `source_paths_for_provider` implementation was found in the inspected worktree.

## P0/P1 regression risks observed

- **P1 — Active discovery acceptance path is absent in this checkout.** There is no `--discover` parsing or provider-root scan; `sync` remains explicit-path only (`crates/agent-session-grep-cli/src/main.rs:1001-1017`, `:1581-1700`). The active task's acceptance criteria therefore are not represented by the inspected implementation.
- **P1 — Deleted-source discovery cannot be represented by current explicit capture.** `capture` opens the supplied path and maps open failures to source I/O errors (`crates/agent-session-grep-adapters-sqlite/src/source_fs.rs:31-47`); the existing tombstone path requires an empty, complete `SourceBatch`, while unseen paths are not in `sync_files`' current batch. The task design explicitly identifies this missing-file case (`.trellis/tasks/08-14-provider-source-auto-discovery/design.md:51-70`).
- **P1 — Root-completeness metadata is not exposed by current sync output.** The existing `sync_files` result reports source/message counters and diagnostics (`crates/agent-session-grep-cli/src/main.rs:1680-1700`) but has no discovery completeness/provider summary. The task design calls for `{complete, providers}` only on the explicit discovery path (`design.md:85-90`).
- **P1 — Session identity is native but not provider-scoped in the inspected CLI handoff.** Claude's `sessionId` is extracted and surfaced correctly, but CLI conversion calls `StableId::native(IdKind::Session, sid)` directly (`crates/agent-session-grep-cli/src/main.rs:1328-1335`). The inspected code does not add a provider namespace at that point; provider identity is carried separately in the document payload (`:1499-1505`).
- **P1 — Current source scan schema has no provider discriminator.** The inspected `source_scans` schema contains `source_path`, `scanned_at_ms`, `len_bytes`, and `fingerprint` only (`crates/agent-session-grep-adapters-sqlite/src/lib.rs:1080-1085`), and writes those same fields at `:3282-3294`. Provider-scoped prior-path diffing cannot be performed from this table alone without another provider association; the active design calls for an additive `provider_id` column (`design.md:72-84`).

## Test / gate status

- No files outside the task research area were modified in this round.
- No tests, `cargo fmt --all --check`, Clippy, workspace tests, or release build were run.
- Evidence is read-only source inspection plus existing test definitions; no new test result is claimed.

## Caveats / Not Found

- The active task's `task.json` points to no worktree (`worktree_path: null`), while the inspected source is the current agent worktree. The task's existing research/design files refer to a different historical integration worktree; their line numbers are cited only for task/design statements, not as current source evidence.
- No provider-root resolver, recursive provider source walker, `sync --discover` dispatch, discovery completeness envelope, or provider-filtered prior-source query was found in the inspected checkout.
- No real provider transcript was read or modified; the Claude golden fixture is synthetic and redacted (`crates/agent-session-grep-provider-claude/tests/golden/PROVENANCE.md:1-49`).
