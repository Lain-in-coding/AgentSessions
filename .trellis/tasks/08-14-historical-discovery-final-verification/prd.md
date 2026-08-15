# Historical discovery final verification

## Goal

Run cross-layer E2E, privacy/read-only verification, full-corpus Gate D, docs reconciliation, and final integration review for the entire Historical Session Discovery & Resume umbrella.

## Requirements

### R1 Cross-layer verification
- All new Application, CLI, Robot, MCP, TUI, and Skill behaviors pass E2E against synthetic fixtures.
- Cursor, schema, identity, and Resume Metadata interactions are verified together, not only in isolation.

### R2 Privacy and read-only
- No real personal path, hostname, or identity is committed in tests, fixtures, or docs.
- Provider Source checksums are unchanged before and after any sync run.
- Ordinary responses never expose Source paths or Resume-sensitive values.

### R3 Gate D
- Full-corpus real-data regression (six invariants) is green on the official catalog.
- Any blocked feature is reported truthfully rather than silently dropped.

### R4 Documentation
- CONTEXT.md, ADRs, schemas, CONTRACT, SKILL, and operations docs agree with runtime behavior.

### R5 Release readiness review
- The owner reviews the final integration; commit, push, and merge remain owner decisions.

## Acceptance Criteria

- [ ] Cross-layer E2E green.
- [ ] Privacy/read-only verification green.
- [ ] Gate D full-corpus re-run green.
- [ ] Docs reconciled with behavior.
- [ ] Owner completes final review; no automatic commit/push/merge.

## Constraints

- Repository stays private until a separate owner release decision.
- No automatic commit, push, merge, or Provider Source modification.
