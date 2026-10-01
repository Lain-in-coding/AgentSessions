# Implement and check

1. Wait for A's implementation and check; read its actual diff and parent decisions.
2. Add CLI regression tests for whole-source updates and empty identity transitions before repairs.
3. Implement source-class gating and a shared empty/provider decision with minimal edits.
4. Add the narrow stored-placeholder proof and transactional transition; include negative real-identity and rollback/shared-owner tests.
5. Add CLI Hermes partial/coexistence/full-recovery coverage; reuse existing provider fixtures, no real transcript data.
6. Run targeted CLI and SQLite tests/clippy offline with --locked; format only touched packages. Do not run a 1M benchmark or alter caps to pass tests.
7. Report changed files, executed commands/results and spec touchpoints. No commits/archives, shared spec edits or subagent spawning.
