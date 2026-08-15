# Structured Tool Activity

## Goal

Normalize searchable structured tool calls and results while preserving bounded evidence and Source read-only rules.

## Requirements

### R1 Structured projection
- Tool calls and results are extracted from provider transcripts into structured, searchable fields (tool name, bounded input/output summary).
- Projection is additive to the existing Message text and never replaces it.

### R2 Bounded evidence
- Tool payloads are bounded before indexing; oversized results are summarized or truncated explicitly, never silently.
- Evidence spans still point back to the exact Source byte range.

### R3 Searchability
- Structured tool activity participates in search and context without destabilizing Message ownership, session grouping, or the FTS projection.
- Tool input schemas may need bounded `maxLength`/`maxItems` enforcement.

### R4 Safety
- Provider Sources remain read-only; no tool output is executed, rendered to a terminal, or treated as a command.

## Acceptance Criteria

- [ ] Tool calls/results appear as structured searchable fields alongside existing text.
- [ ] Payloads are bounded with explicit truncation; spans remain exact.
- [ ] Search/context/grouping semantics are unchanged for non-tool Messages.
- [ ] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- Sequence after the Resume core stream.
- Additive contract changes only.
- No commit or push without explicit owner authorization.
