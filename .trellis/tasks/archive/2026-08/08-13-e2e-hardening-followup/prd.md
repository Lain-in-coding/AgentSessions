# e2e hardening follow-up (exit 7/70, stderr privacy, remaining assertions)

## Goal

Close the remaining acceptance gap deferred from 08-10-performance-prerelease
(acceptance criterion 5, e2e portion) and the residual test-coverage gaps
noted by the 2026-08-13 full-repo review. This is a lightweight task: the
2026-08-13 review-fix round already added most of it (exit 5/6/2 assertions,
stderr-privacy guards for robot+human+stderr and mcp, envelope-shape
strengthening, provider-matrix structural assertions, list_sessions
protocol-layer rejection). What remains is narrow.

## Background

- The 08-10 task's acceptance criterion 5 required "e2e asserts stderr
  cleanliness, exit 5/6/7/70 coverage, and single-envelope robot output".
  The review-fix round landed exit 5/6/2 + stderr + envelope-shape + matrix
  assertions. The remaining gaps are exit 7 and exit 70 e2e coverage, which
  the 2026-08-13 closed-loop review explicitly noted as not e2e-triggerable
  by black-box legal input (exit 7 needs an adapter mid-parse failure, exit
  70 needs an invariant violation).
- The 08-13 review also recorded non-blocking items that could become tests
  later: testkit InMemoryStore "catalog exists but no placement returns empty
  success" lacks a dedicated test; codex probe tightening was not re-verified
  on a real codex file (owner self-test step).

## Requirements

- R1 Add e2e coverage for `provider_error` (exit 7): a path that makes a
  provider adapter fail mid-parse so the CLI maps it to exit 7 +
  `error.code == "provider_error"`. (May need a test-only adapter injection
  hook or a crafted input that reaches the adapter's structural-fatal path.)
- R2 Add e2e coverage for `internal` (exit 70): the `DomainError::InvariantViolation
  → Internal → 70` mapping has a unit test but no end-to-end path. If a legal
  black-box trigger is impossible, document why and keep the unit-level
  coverage as the contract guard.
- R3 Ensure single-envelope robot output is asserted for every write command
  (`sync`, `ingest`, `index rebuild`) — a progress-frame + response-envelope
  mix in `--robot` mode is a protocol violation that must be caught.
- R4 Add the testkit InMemoryStore "catalog exists but no placement returns
  empty success" dedicated test.
- R5 Record the codex-probe-on-real-file verification as an owner self-test
  step in the task notes (not something the harness can do).

## Acceptance Criteria

- [ ] e2e covers exit 7 (`provider_error`) via an injected or crafted path;
      if genuinely not triggerable, a documented reason + unit-level guard.
- [ ] e2e covers exit 70 (`internal`) or documents why only unit-level
      coverage is possible.
- [ ] `--robot` write commands assert exactly one JSON envelope on stdout
      (no progress frames mixed in).
- [ ] testkit InMemoryStore empty-success path has a dedicated test.
- [ ] All quality gates green; full-corpus Gate D still passes.

## Constraints

- No new production features; tests and any minimal test-support hooks only.
- If a test-only adapter injection hook is added, it must be dev/test-gated
  and never change production behavior.
- Repository stays private; no push without owner instruction.
