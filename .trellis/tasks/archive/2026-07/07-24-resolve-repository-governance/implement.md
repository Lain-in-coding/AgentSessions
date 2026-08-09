# Repository Governance Migration Plan

## Parallel discovery

- [x] Inventory unique active requirements and stale claims in all `Plan/` files.
- [x] Inventory `.trellis/` shared assets versus runtime/private state.
- [x] Inventory `.agents/`, `.codebuddy/`, `.codex/`, and `AGENTS.md` for clone-reproducible versus local-only content.
- [x] Search repository references to Plan/handoff paths and determine replacement targets.

## Migration

- [x] Write a source-to-destination migration matrix in this task.
- [x] Move active work to existing third/fourth batch Trellis tasks without activating them.
- [x] Move durable implementation rules to existing code-specs only where missing.
- [x] Improve `AGENTS.md` with a stable quick-start and authority order, without current milestone snapshots.
- [x] Implement explicit tracked/ignored policy for each workflow root.
- [x] Reconcile the stale bootstrap task and workspace indexes without preserving machine/session state as shared truth.
- [x] Delete date-stamped handoff files only after their unique information is accounted for.
- [x] Retire/delete the legacy execution Plan only after all active/durable information is migrated.

## Validation

- [x] Simulate a new Agent handoff using only `AGENTS.md`, Trellis task commands, specs, and Git state.
- [x] Confirm parent progress remains 2/5 and no third-batch task is active.
- [x] Confirm no private path, identity, credential, transcript, runtime pointer, or local approval enters shared content.
- [x] Run reference searches and `git diff --check`.
- [x] Run product quality gates only if product code/configuration is changed.
- [x] Obtain independent Trellis review before archive.

## Review gates

- No source is deleted before its migration disposition is recorded.
- Historical/stale checkboxes are not copied into a new current-state document.
- Trellis is described as development workflow tooling, not an AgentSessions product feature.
- No commit or push without a separate explicit user request.
