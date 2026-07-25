# Repository Governance Migration Design

## Boundary

This task replaces stale handoff/Plan files as current-state authorities with clone-reproducible Agent entrypoints, Trellis tasks/specs, formal records, and executable evidence. It does not start the 0.1/0.2 evidence batch, alter product behavior, create a remote, commit, or push.

## Authority model

- `AGENTS.md`: stable onboarding procedure only; no milestone snapshot.
- `.trellis/tasks/`: current and archived execution state.
- `.trellis/spec/`: executable implementation contracts.
- RFC/ADR/contracts: formal decisions.
- Tests, CI runs, and Spike Evidence: verified implementation evidence.
- Git history: historical change record.
- Date-stamped handoff documents and legacy Plan checkboxes: retired after unique information is migrated.

## Migration procedure

1. Inventory every statement in `Plan/` that is not already represented elsewhere.
2. Classify it as active work, durable contract, formal decision, historical note, stale/conflicting claim, or private/machine-specific content.
3. Migrate only active and durable information to the appropriate Trellis task/spec or formal document.
4. Record a source-to-destination migration matrix in this task before deleting ignored source files.
5. Search and update repository references that would become broken or misleading.
6. Validate onboarding from a clean-reader perspective using only tracked/shared entrypoints.

## Tracking policy

Each untracked workflow root receives an explicit policy:

- Track only shared, deterministic, privacy-safe configuration and task/spec artifacts.
- Ignore runtime pointers, worktrees, caches, transcripts, local journals, developer identity, approvals, and machine-specific state.
- Do not bulk-track `.agents/`, `.codebuddy/`, `.codex/`, or `.trellis/` before inspecting their contents.
- Keep Trellis development tooling clearly separate from AgentSessions product features.

## Deletion safety

`Plan/` is currently gitignored, so deletion is not recoverable from repository history. Delete a source file only after reading it, recording its migration disposition, and verifying that all unique active/durable content has a destination. If any unique requirement cannot be classified safely, retain that source until resolved.

## Validation

- Every workflow root is explicitly tracked or ignored.
- Shared files contain no personal paths, credentials, runtime session identifiers, or machine approvals.
- `AGENTS.md` enables a new Agent to locate current state without reading legacy Plan files.
- Parent progress remains 2/5; third batch remains planning and inactive.
- Repository references to retired Plan documents are resolved.
- Product quality gates remain green if product-tracked files are touched.
