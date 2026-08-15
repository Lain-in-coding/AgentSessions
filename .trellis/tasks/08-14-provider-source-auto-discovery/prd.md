# Provider source auto-discovery

## Goal

Add an explicit, provider-aware `sync --discover` that locates each provider's Source transcripts under the current user's known provider data roots, with safe root-completeness semantics and read-only Source handling.

## Requirements

### R1 Discovery scope
- `sync --discover` scans each Provider's canonical data root for JSONL Sources (Claude Code session/transcript files, Codex rollouts).
- Discovery is explicit; it never runs implicitly during plain `sync` or `search`.
- Each discovered Source is fingerprinted and incremental-synced with the existing unchanged-source skip semantics.

### R2 Root completeness
- A discovery run records whether the root scan was complete or partial.
- An incomplete scan must not tombstone Sources that were merely not seen; absence is confirmed only by a complete root scan.

### R3 Safety
- Provider Source files remain read-only; checksums are verified before and after.
- No Source is moved, renamed, or deleted by discovery or sync.

### R4 Reproducibility
- Re-running `sync --discover` converges to the same catalog state.
- Per-provider roots are resolved from the known Data Root layout, not guessed.

## Acceptance Criteria

- [ ] `sync --discover` finds and syncs each provider's Sources under known roots.
- [ ] Incomplete scans never tombstone unseen Sources.
- [ ] Provider checksums are unchanged after discovery and sync.
- [ ] Re-runs converge with no new generation when nothing changed.
- [ ] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- Sequence after the Resume core stream and before Provider expansion.
- Additive CLI surface only.
- No commit or push without explicit owner authorization.
