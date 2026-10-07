# Implementation

Use ../10-07-journal-online-maintenance/implement.md for gates and sequence.

Own CLI routing, journal command/process module, protocol/help/flag scanner changes and CLI E2E tests. Coordinate Ports/queue and storage signatures with other workers. No adapter or Application edits; request changes from owners. Cover real detached process lifecycle and footprint evidence.

Read applicable specs, inspect current code first, edit directly in the shared workspace, run targeted tests, report exact changed files and evidence. Do not spawn nested agents or commit.

## Implemented and verified
CLI359 unit and13 normal maintenance process tests passed. Enlarged2000-message fixture explicitly passed with default30s. Full coordinator workspace/semantic gates and final physical rerun passed.

See ../10-07-journal-online-maintenance/research/verification.md for final integrated evidence. Code is implemented and locally checked; user-approved commits and bookkeeping are being finalized. No cross-platform CI claim.
