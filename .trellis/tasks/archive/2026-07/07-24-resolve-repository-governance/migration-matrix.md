# Legacy Plan and Handoff Migration Matrix

This matrix records the disposition of legacy planning material before any ignored source file is deleted. It is a migration ledger, not a milestone-status document.

## Authority replacement

| Legacy information | Disposition | Destination / authority |
|---|---|---|
| Current task, completion, next-step, and clean-worktree claims | Discard as stale snapshot | `.trellis/tasks/`, `task.py current/list`, Git status, tests and CI results |
| R0 approval/acceptance claims | Discard where inconsistent; do not backfill approval | `docs/architecture/R0-ARCHITECTURE-REVIEW.md` and the cited RFC/ADR/contract records |
| 0.1/0.2 completion and cross-platform claims | Migrate as open evidence work | `.trellis/tasks/07-24-close-core-beta-evidence/prd.md` |
| Provider promotion, canonical model, ADT, Human/Robot, MCP, Skill, TUI, installer, clean-machine, migration/rebuild work | Migrate as planned integration work | `.trellis/tasks/07-24-advance-integration-beta/prd.md` |
| Trellis tracking, bootstrap, workspace, and agent-root policy | Migrate as active repository governance | Current task PRD/design/implementation plan |
| Git, privacy, destructive-operation, and contribution rules | Preserve only where not already represented | `CLAUDE.md`, `CONTRIBUTING.md`, and stable onboarding in `AGENTS.md` |
| Windows Defender/local machine hazards, personal memory paths, raw transcript pointers | Do not migrate into shared repository content | Private/local memory only |
| README/LICENSE/publication readiness decisions | Retain as an explicitly open future repository-readiness concern | Current governance findings; schedule separately if not resolved here |
| Push/publish authorization | Preserve existing rule: explicit user request only | `CLAUDE.md` and `CONTRIBUTING.md` |

## Reference migration

The following current-authority references must stop depending on `Plan/项目执行计划.md` and instead link to, or state, their actual executable/formal authority:

- RFC-0001 and RFC-0002 → their own normative sections plus R0 review for pending decisions.
- ADR-0001 → search-backend Spike Card/Evidence, Fixture Policy, and SLI format.
- ADR-0002 → target definitions and R0 review pending decision.
- Threat Model → security contracts and R0 review.
- Fixture Policy → its own normative policy and provider promotion gates.
- SLI format → its own measurement contract and the core-evidence task for open work.
- Provider maturity matrix → provider promotion evidence and integration-beta task.
- External readiness and reuse/license audit → their own gate records.
- CLI code-spec → architecture layering and CLI/Robot/MCP contract.
- Product source comments → self-contained invariants or direct RFC/ADR/contract/schema references, never a removed Plan section.
- Spike documents → formal ADR/policy/SLI links; any retained Plan mapping must be explicitly historical and non-authoritative.

## Files retired after validation

| Source | Action | Validation completed |
|---|---|---|
| `Plan/交接文档-2026-07-23.md` | Deleted | Unique-information audit and reference replacement completed; private paths/identity were not migrated |
| `Plan/项目执行计划.md` | Deleted | Active requirements migrated and current-authority/backfill references removed |
| `Plan/Git提交与迁移规划.md` | Deleted | Durable Git/privacy rules already live in `CLAUDE.md` and `CONTRIBUTING.md` |
| Root date-stamped legacy handoff | Deleted | Unique project facts migrated; personal paths and private transcript pointers discarded |

## Open repository-readiness items retained from legacy planning

These remain unresolved but are outside this migration's product scope:

- Choose and add a root `LICENSE` before public release; no license choice is inferred here.
- Add a product `README.md` in a dedicated documentation/readiness task.
- Decide separately whether to add optional pre-commit/gitleaks automation; this migration does not make it a mandatory gate.
- Remote creation, publication, commit, and push still require explicit user authorization under `CLAUDE.md` and `CONTRIBUTING.md`.

## Workflow-root tracking policy

| Root | Policy | Rationale |
|---|---|---|
| `AGENTS.md` | Track | Stable Trellis onboarding entrypoint; generated/managed shared content |
| `.agents/**` | Track | Deterministic Trellis skills/integration assets for supported agent platforms |
| `.codebuddy/**` | Track | Deterministic Trellis platform integration assets |
| `.codex/**` | Track | Deterministic Trellis agent/skill integration assets; may require user-level trust/enablement |
| `.trellis/workflow.md`, `.trellis/spec/**`, `.trellis/tasks/**`, `.trellis/scripts/**` and deterministic templates/configuration | Track | Required for clone-reproducible workflow, current/archive state, specs, and task commands |
| `.trellis/.developer`, `.trellis/.runtime/**`, local journals/session traces, caches, backups, temporary files, machine approvals | Ignore | Developer identity and runtime/private state are not shared project truth |
| `.trellis/workspace/**` | Selective/local by default | Optional per-developer journals; never onboarding or current-state authority |

The platform skill trees contain intentional generated duplication for clone usability. They are Trellis development tooling, not AgentSessions product features.

## Implemented workflow policy

- Track `AGENTS.md`, `.agents/**`, `.codebuddy/**`, and `.codex/**` as deterministic shared Trellis integrations.
- Track `.trellis/.gitignore`, `.version`, `config.yaml`, `workflow.md`, `agents/**`, `scripts/**`, `spec/**`, privacy-sanitized `tasks/**`, and the generic `workspace/index.md`.
- Ignore `.trellis/.developer`, `.runtime/**`, `.template-hashes.json`, `.cache/**`, `worktrees/**`, per-developer `workspace/*/`, caches, backups, temp files, logs, session identifiers, and machine approvals.
- `session_auto_commit: false` keeps local workspace journals out of automatic Git operations.
- Never force-add the `.trellis` tree; stage reviewed shared paths explicitly.
- Task metadata must use non-personal shared roles/handles and keep `worktree_path` null or repository-relative.
- The stale bootstrap task is retired only after verifying the populated specs; per-developer workspace indexes/journals remain ignored rather than repaired as shared truth.

## Required onboarding correction

`AGENTS.md` must stop directing new readers to `.trellis/workspace/` as a primary knowledge source. It should state the authority order, quick-start task/spec commands, the development-tooling/product boundary, and that integrations may require platform-specific user-level enablement.

## Validation completed

- Integrated the final `AGENTS.md` onboarding content and clarified the managed workspace entry as optional/local/non-authoritative.
- Sanitized task metadata to shared role names with null worktree paths.
- Retired the stale bootstrap task after confirming populated package specs.
- Integrated formal-doc, code-comment, and spike reference replacements.
- Re-ran repository-wide legacy-reference and privacy scans before deletion.
- Removed machine-specific checkout paths from the cross-platform packaging script and Spike Card.
