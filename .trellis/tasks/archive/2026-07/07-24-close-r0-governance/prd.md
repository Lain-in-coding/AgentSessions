# Close R0 governance gate

## Goal

Turn the existing R0 drafts and spike evidence into a decision-ready governance package, resolving factual drift and recording every remaining decision that requires project-owner approval.

## Requirements

- Reconcile RFC-0001, RFC-0002, ADR-0001, ADR-0002, the CLI/Robot/MCP contract, Threat Model, fixture policy, SLI format, license audit, and external-readiness gate.
- Fix demonstrably stale or contradictory technical claims without silently changing product scope.
- Repair and rerun the data-root locking spike so its evidence matches the checked-in program.
- Add missing Spike Cards or explicitly consolidate their required fields into an accepted evidence record.
- Document unresolved owner/approver decisions; do not fabricate approvals or `Accepted` status.

## Acceptance Criteria

- [x] Data-root locking spike and evidence agree and reproduce on Windows.
- [x] R0 documents have no known internal contradictions about implemented behavior or target count.
- [x] Every R0 open question has a proposed decision, trade-off, and named approval requirement.
- [x] R0 evidence distinguishes local Windows results from unverified cross-platform claims.
- [x] Architecture review checklist is ready for the project owner, with no false acceptance claims.

These criteria mean the package is decision-ready, not accepted. R0 remains Pending until owner/approver action.
