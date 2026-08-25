# Second-wave providers

## Goal

Evaluate and add Grok Build, OpenClaw, Hermes, and other Providers only after format evidence and maturity gates are met.

## Requirements

### R1 Evidence-first
- A Provider is added only when its transcript format is demonstrated by real/representative evidence and reduced to the probe + parse contract.
- Each candidate is recorded with format findings before any adapter work.

### R2 Maturity honesty
- New adapters register with an honest experimental/unsupported tier and are never promoted without golden fixtures, span evidence, privacy checks, and quality gates.
- The Provider capability registry states whether Resume Metadata is available.

### R3 Bounded scope
- The task is a container for future wave-specific children; it does not pre-commit to any Provider.

## Acceptance Criteria

- [ ] Format evidence and maturity gates are recorded per candidate.
- [ ] Any added adapter is honest in the Provider matrix and capability registry.
- [ ] Resume Metadata is `null`/`—` when the Provider does not record authoritative values.
- [ ] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- No speculative format support; no commit or push without explicit owner authorization.
