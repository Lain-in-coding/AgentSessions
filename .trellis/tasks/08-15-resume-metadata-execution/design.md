# Resume metadata execution — Read-only surfaces (Design)

> Slice scope: the read-only preview surface of resume-metadata-execution.
> Execution (spawn / process replacement / `--yes` opt-in / first-run forced
> preview) belongs to the later execute slice of the same task and is NOT
> implemented here.

## Context

The resume metadata CORE (provider session id + original working directory +
title/summary claims, schema v11, `ResumeClaimsStore` impl on `SqliteStore`,
`get-session-resume` CLI/MCP) was completed on branch
`session-metadata-search-08-15` (uncommitted). This worktree is based on
`main`, which does NOT contain the claim fields. The read-only surfaces slice
therefore:

- builds against the ports traits (`ResumeClaimsStore` / `SessionResumeMetadata`
  / `MetadataResolution` / `SourceResumeClaim` / `NoResumeClaims`) — mirrored
  into this worktree's ports crate **byte-identically** from the
  session-metadata-search-08-15 branch so the eventual merge sees one common
  addition instead of two conflicting ones;
- degrades gracefully when claims are absent: without a claims implementation
  the CLI wires the `NoResumeClaims` placeholder, so every session reports
  `resume_available = false` + an explicit structured reason — never a
  fabricated command.

## Decisions

### D1 — Ports types mirrored, not invented

`ports/src/lib.rs` gains the ADR-0009 resume types exactly as defined on the
metadata-search branch (same location, same text): `MetadataResolution<T>`,
`ProviderSessionObservation`, `SessionResumeMetadata`, `ResumeClaimsStore`,
`SourceResumeClaim`, `NoResumeClaims`, and the `&T` blanket impl. Verified
byte-identical by diff against the sibling worktree. `SearchHit.resume_available`
and `ParseReport.session_observation` (core extraction surfaces) are NOT
mirrored — they belong to the sibling branch's hunks and merge independently.

### D2 — Preview service is a standalone application-layer type

`ResumePreviewService<R: ResumeClaimsStore>` with
`resume_preview(&self, session_id: &StableId) -> PortResult<ResumePreview>`
lives in the application crate as a new section, NOT as a method on `App`.
`App` is heavily reworked on the sibling branch; keeping the service separate
keeps this slice additive with zero overlap.

### D3 — Fail-closed reason enum

`ResumeUnavailableReason { NoClaims, ConflictingClaims, UnsupportedProvider }`.
String codes reuse the store's ADR-0009 `unavailable_reason` vocabulary
(`"no resume metadata claims"`, `"conflicting resume metadata claims"`) so the
robot envelope never introduces a second vocabulary; `"unsupported-provider"`
is new (provider has no known command template). Unknown store reason strings
classify as `NoClaims` (same class: no authoritative value).

### D4 — Command templates are data, never executed

Per-provider mapping `claude-code -> ["claude","--resume","<provider-session-id>"]`,
`codex -> ["codex","resume","<provider-session-id>"]`. Unknown provider →
fail closed with `UnsupportedProvider`. The template carries
`execution: "never-by-this-tool"` and the placeholder is never substituted or
spawned by this tool; the human renderer substitutes the placeholder into the
copy-by-hand line only.

### D5 — CLI wiring uses the placeholder store, one-line swap after merge

The `resume` subcommand wires `ResumePreviewService::new(&NoResumeClaims)`.
When the metadata-search branch lands, swapping to `store_ref(store)` yields
real claim resolution with no shape change. `resume` is a read command: it is
not in the writer-lease set, opens the store read-only, and never spawns a
process.

## Boundaries

- Read-only: no process spawn, no provider file writes, no source-path
  leakage (envelope carries session/provider ids and cwd only — the resume
  target data, never transcript paths).
- No new MCP tool; `resume` is CLI-only for this slice.
- Additive CLI surface: existing commands unchanged.

## Compatibility

- Ports additions are identical to the sibling branch → merge treats them as
  one change.
- `get-session-resume` (sibling) returns raw metadata fields; `resume` (this
  slice) returns the preview projection + command template — complementary,
  no field conflicts.
