# Resolve repository governance state

## Goal

Make the repository's local Agent/Trellis workflow state intentional and reproducible without mixing runtime state into product history.

## Requirements

- Decide and document whether `.trellis/`, `.agents/`, `.codebuddy/`, `.codex/`, and `AGENTS.md` are shared tracked assets or local-only ignored state.
- Exclude runtime pointers, caches, backups, developer identity, and machine-specific approvals from shared content.
- Reconcile the stale bootstrap task and workspace journal indexes.
- Keep AgentSessions product features separate from Trellis development tooling.
- Do not create a remote, commit, or push without separate explicit authorization.

## Acceptance Criteria

- [x] Every currently untracked workflow root has an explicit tracked-or-ignored policy.
- [x] Shared configuration is clone-reproducible and contains no runtime/private state.
- [x] Bootstrap task and workspace session indexes match actual state.
- [x] Documentation cannot be misread as claiming Trellis tooling is an AgentSessions product Skill.
- [x] Git status no longer contains ambiguous workflow configuration state after the chosen policy is implemented.
