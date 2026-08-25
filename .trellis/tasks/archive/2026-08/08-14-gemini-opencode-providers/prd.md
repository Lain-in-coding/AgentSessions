# Gemini and OpenCode providers

## Goal

Add the first Provider-expansion wave (Gemini and OpenCode) with honest maturity gates, golden fixtures, Source spans, and optional Resume capability.

## Requirements

### R1 Adapter and maturity
- New adapters implement the existing probe + parse contract and register in the Provider matrix with an honest maturity tier.
- Maturity is never promoted without golden fixtures, span evidence, privacy checks, and full quality gates.

### R2 Capability registry
- Search support and Resume support are separate capabilities.
- A new Provider may be searchable while Resume Metadata is unavailable; capability rows are explicit.

### R3 Fidelity
- Golden fixtures pin canonical output and span round-trips.
- Privacy-safe synthetic fixtures only; no real personal transcripts in committed tests or docs.

### R4 Read-only
- Provider Source files remain read-only.

## Acceptance Criteria

- [ ] Gemini and OpenCode adapters parse and search with pinned golden output.
- [ ] Provider matrix and capability registry are updated honestly.
- [ ] Resume Metadata is populated where the Provider records authoritative values and is `null`/`—` otherwise.
- [ ] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- Requires real-format evidence before adapter promotion; no speculative format support.
- Sequence after the Resume core stream and discovery task.
- No commit or push without explicit owner authorization.
