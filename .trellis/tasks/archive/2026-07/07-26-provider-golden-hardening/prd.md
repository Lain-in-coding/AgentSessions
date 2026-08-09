# Claude/Codex golden fixtures, contract tests, and property hardening

Child 2 of `07-24-advance-integration-beta`. Prerequisite child 1
(canonical Source/Session/Span foundation) is archived complete.

## Goal

Give both providers (claude-code, codex) the evidence the Beta promotion
template requires: committed golden fixtures with provenance, contract tests
pinning parse output exactly, and deterministic property coverage for
malformed/Unicode/large-field/threading/duplicate-mirror inputs — then update
the provider maturity matrix honestly.

## Requirements

- Golden fixtures are synthetic and format-faithful (never real transcripts),
  follow `docs/security/FIXTURE-REDACTION-POLICY.md`, and carry a provenance
  manifest stating how they were constructed and what real-format knowledge
  they encode.
- Golden tests pin the full canonical output: per-message native_id/parent/
  role/text/timestamp/sidechain/span plus report counts and
  session_native_id; any parser behavior change must break a golden test.
- Byte-exactness is protected against git line-ending conversion
  (`core.autocrlf=true` today, no `.gitattributes`): fixture bytes on disk
  must reach the parser unmodified on every platform.
- Property tests are deterministic (fixed-seed, no new dependencies, matching
  the repo's zero-framework test idiom) and cover at minimum: malformed lines
  interleaved, Unicode (CJK/emoji/multibyte) with correct byte spans,
  large fields, threading chains (claude parentUuid/sidechain), and codex
  event_msg mirror duplication never double-counting.
- Core properties: span round-trip on arbitrary generated transcripts; seq
  starts at 0 and is contiguous; parse determinism (same bytes → identical
  output); accounting consistency (committed/skipped/diagnostics).
- `docs/product/PROVIDER-MATURITY-MATRIX.md` is updated to reflect the gates
  actually met, with pointers to the evidence; no overclaiming.
- Providers remain strictly read-only; no production-code behavior change is
  expected — if a property test finds a real defect, fix it with a minimal
  diff and a regression note.
- Quality gates green: fmt, clippy -D warnings, full workspace tests, and
  `cargo deny check` unaffected (no new dependencies).

## Acceptance Criteria

- [ ] Each provider has ≥1 committed golden fixture + expected-output pair
      and a golden test that fails on any canonical-output drift.
- [ ] Fixture provenance manifest exists and satisfies the redaction policy.
- [ ] Property suites run deterministically and cover the five mandated
      dimensions; failures reproduce from a printed seed.
- [ ] Byte-exactness holds after a fresh checkout with autocrlf (verified via
      `.gitattributes` coverage for fixture paths).
- [ ] Maturity matrix rows for claude-code and codex updated with met/unmet
      gates and evidence links.
- [ ] All quality gates green on Windows.

## Out of Scope

- New providers, CLI/storage/protocol changes, real-transcript fixtures,
  external fuzzing infrastructure (cargo-fuzz), remote/publishing actions.
