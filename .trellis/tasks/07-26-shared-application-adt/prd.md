# Shared Application ADT for cursor, budget, branch, and evidence

Child 3 of `07-24-advance-integration-beta`. Children 1-2 archived complete.
Normative source: `docs/contracts/CONTRACT-cli-robot-mcp-draft.md` §1-3, §7
(status Draft — implementing it does not mark it Accepted; deviations must be
documented in design.md, not silently made).

## Goal

Move the business rules that every frontend would otherwise duplicate —
cursor lifecycle, response budget, branch selection, evidence DTO assembly,
partial-result semantics, generation stamping — into the shared Application
ADT, and prove them end-to-end through the existing CLI so child 4 (Human/
Robot protocol completion) and children 5-6 (MCP, TUI) only map protocol,
never re-implement semantics.

## Requirements

- Versioned CursorToken per contract §7: self-contained opaque token carrying
  generation, issued/expiry, query/filters/sort digests, and contract major;
  tampering, TTL expiry, generation mismatch, and contract-major mismatch each
  fail loudly with the distinct canonical codes (`cursor_invalid`,
  `cursor_expired`, `generation_mismatch` family) — never silently restart
  from page one. Time is injected (testable), never read ambiently in the
  application layer.
- ResponseBudget per contract §3: validated on entry (absurdly small budget →
  invalid_request, no invalid JSON emitted); sort first, then truncate;
  truncation is reported structurally (reason + next_cursor + partial
  outcome) and the envelope/generation always survive the byte gate.
- Branch selection over the canonical threading facts (parent pointers,
  sidechain flags) as a pure, deterministic domain rule: at minimum a
  `mainline` policy (default) and a `full` policy; selection is stable across
  runs on the same input and unit-tested against fork/sidechain shapes.
- Versioned EvidenceSpan DTO per contract §2 assembled in the application
  layer from stored message payloads: byte precision when a stored span
  exists, explicit `precision: unknown` for legacy rows — fields are never
  fabricated. No absolute paths in the DTO.
- Session context use case (`get_session_context` core): given a session id,
  return session + selected branch + ordered messages + evidence + truncation
  + generation, honoring budget and policy.
- Search/List gain cursor + budget + generation-stamped results through the
  shared ADT; existing simple behavior remains available via defaults.
- Every response carries the generation it was computed against.
- CLI wires the new semantics (cursor flag, budget flags, `context` command,
  new error codes in the Robot error catalog with schema cross-validation
  kept green) — enough to prove the contract end-to-end; full protocol
  surface completion remains child 4.
- No MCP/TUI code; no new dependencies; providers untouched; layering
  invariant domain ← ports ← application ← adapters preserved.
- Quality gates green (fmt, clippy -D warnings, workspace tests, cargo deny
  unchanged).

## Acceptance Criteria

- [ ] Cursor round-trip: page 1 → token → page 2 resumes exactly, same total
      coverage as unpaged; tampered/expired/cross-generation/cross-major
      tokens produce their distinct canonical errors (unit + CLI e2e).
- [ ] Budget: over-budget search/list/context responses report partial +
      truncation reason + next_cursor; a too-small budget is rejected as
      invalid_request; envelope fields survive truncation (unit + e2e).
- [ ] Branch selection: deterministic mainline on forked and sidechain
      fixtures; full policy returns everything in seq order (domain unit
      tests, no I/O).
- [ ] Evidence DTO: byte-precision spans for post-v6 rows, explicit unknown
      precision for legacy rows; message/document/generation/fingerprint
      references correct (unit + e2e over a real ingest).
- [ ] `context <ses_v1_...>` returns session/branch/messages/evidence/
      truncation/generation through the shared ADT.
- [ ] New canonical codes appear in `schemas/robot/v1` catalog and the
      include_str! cross-validation tests stay green.
- [ ] All gates green on Windows; no new dependencies (cargo deny clean).

## Out of Scope

- MCP server, Skill, TUI (children 5-6); full Human/Robot output truth-table
  completion (child 4); keyed cursor signing infrastructure beyond integrity
  digest (documented deviation); cross-file session stitching; marking the
  contract Accepted.
