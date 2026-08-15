# Provider Resume Metadata extraction

## Goal

Extract authoritative native Provider Session ID and Original Working Directory from Claude Code and Codex transcripts, preserving the association between the two values and failing closed on ambiguity.

## Requirements

### R1 Claude Code
- Retain the existing top-level `sessionId` as the authoritative native Session ID.
- Capture the top-level `cwd` field as the Original Working Directory. The field exists in real transcripts and is currently not declared by `RawLine`, so it is silently discarded.
- Missing or blank values become `None`. Do not guess a value.
- A Source that carries multiple distinct Session IDs must emit a bounded diagnostic and must not claim Resume Metadata under first-ID-wins.

### R2 Codex
- Retain `session_meta.payload.session_id` as the authoritative native Session ID.
- Capture `session_meta.payload.cwd` as the Original Working Directory.
- Never use `turn_context.cwd` as the working directory; it is turn-scoped.
- Missing or blank values become `None`. Do not guess a value.
- A Source that carries multiple distinct Session IDs must emit a bounded diagnostic and must not claim Resume Metadata under first-ID-wins.

### R3 Association and fidelity
- Preserve the association between the observed Session ID and the observed working directory. Never pair values observed independently from different records or sources.
- The provider must report the metadata in the existing `ParseReport` so a future identity/persistence task can project it without re-parsing.
- Every new field is additive; existing parse semantics and Message events remain unchanged.

### R4 Privacy and diagnostics
- Diagnostics never include a real Source path or a full transcript content.
- Diagnostics bound the number of listed IDs and never disclose content.

## Acceptance Criteria

- [ ] Claude Code adapter declares and returns `cwd` and keeps returning the native Session ID.
- [ ] Codex adapter declares and returns `session_meta.payload.cwd` and keeps returning the native Session ID.
- [ ] Missing and blank values return `None`; repeated identical values are not ambiguous.
- [ ] Conflicting or multiple Session IDs produce a bounded diagnostic and do not claim metadata as if only one ID existed.
- [ ] `turn_context.cwd` is proven not to be used as the working directory.
- [ ] Association between ID and working directory is proven preserved.
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo build --release` are green.

## Constraints

- Provider Source files remain read-only; checksums do not change during sync.
- The task touches only the provider crates plus the minimum `ports` types needed for the typed observation; it does not implement Session identity namespacing or persistence.
- No commit or push without explicit owner authorization.
