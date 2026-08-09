# AgentSessions audit remediation and milestone closure

## Goal

Close the gaps identified by the 2026-07-24 repository audit in dependency order, while preserving the local-first privacy model and converting implementation claims into reproducible milestone evidence.

## Requirements

- Use child tasks for governance closure, correctness/security blockers, 0.1/0.2 evidence, 0.3 integration, and repository workflow governance.
- Execute blocking correctness and governance work before declaring later milestones complete.
- Keep product changes minimal and validated by the repository quality gates.
- Do not create a remote, push, publish, sign, notarize, or use external credentials without separate explicit authorization.
  - Remote creation and push were authorized by the owner on 2026-07-25 and are done: private repository `qin-devs/AgentSessions`, branches `main` and `chore/batches-1-3-governance-and-evidence`, pull request #1 open and unmerged.
  - That authorization covers remote creation and push only. Publishing to crates.io, code signing, notarization, and release creation remain unauthorized and outstanding.
- Do not mark Governance Records Accepted merely because implementation exists; acceptance requires explicit owner/approver decisions.

## Acceptance Criteria

- [ ] R0 records and open decisions have explicit resolved or externally-blocked status.
- [ ] Confirmed correctness, privacy, and evidence-drift defects are fixed and regression-tested.
- [ ] 0.1/0.2 claims match reproducible local and cross-platform evidence.
- [ ] Claude and Codex have a documented, test-backed path from Experimental toward Beta.
- [ ] Repository-local Agent/Trellis configuration has an explicit tracking policy.
- [ ] Parent integration review confirms child outcomes are mutually consistent.

## Child Tasks

1. `07-24-close-r0-governance`
2. `07-24-fix-correctness-security`
3. `07-24-close-core-beta-evidence`
4. `07-24-advance-integration-beta`
5. `07-24-resolve-repository-governance`

## Notes

- Governance decisions requiring a named owner or approver may be prepared but not self-approved by an implementation agent.
- The parent coordinates outcomes; implementation occurs in child tasks.
