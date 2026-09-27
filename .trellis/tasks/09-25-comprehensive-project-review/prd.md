# Comprehensive Project Review

## Goal

Review the current agent-session-grep project for actionable defects and optimization opportunities, with reproducible evidence and a prioritized report for the owner. This is an independent review, not completion of the earlier repair task.

## Background

- Baseline: `6cd1e6f6b43f2d9c42a080b2679a6d8a5d801f72`, reviewed on 2026-09-25.
- The owner requested a comprehensive review and explicitly authorized the proposed Trellis planning and review workflow.
- An untracked earlier repair task exists; preserve it and validate historical claims against current source.
- Workspace packages, executable behavior, applicable specs, schemas, and current contracts are the evidence sources.

## Requirements

1. Cover all product workspace crates, ingestion and provider parsers, domain invariants, application queries and budgets, SQLite catalog/projections, CLI/MCP/Web/TUI/hooks/resume, installation/release scripts, schemas, and CI.
2. Assess correctness, data integrity, privacy/security boundaries, platform behavior, performance/scaling, error propagation, test gaps, and maintainability.
3. Every confirmed defect must include severity, exact source anchors, trigger, impact, evidence, and a proposed correction. Keep hypotheses and optimization proposals distinct from confirmed defects.
4. Run available repository quality gates and targeted reproductions using synthetic fixtures. Record exact commands, outcomes, and environmental limits.
5. Produce a Chinese consolidated review and a coverage/verification matrix. Do not claim proof that every possible bug has been found.

## Constraints

- Review only: no product source changes, automatic fixes, commits, pushes, or remote publication.
- Only task review artifacts may be written; builds/tests may create their normal ignored outputs and temporary synthetic fixtures.
- Do not access real transcripts, secrets, or private provider state. Preserve existing user changes.
- Generated workflow assets, competitor clones, archived plans, and spikes are not product implementations.

## Acceptance Criteria

- [x] Coverage accounts for every workspace package and each shipped entry point.
- [x] Baseline lint/tests and available feature/script checks have recorded results.
- [x] High-priority findings are independently checked against current source and, where practical, reproduced.
- [x] Report distinguishes confirmed defects, unconfirmed risks, optimization proposals, and verification limitations.
- [x] Final diff contains review artifacts only and preserves pre-existing changes.
