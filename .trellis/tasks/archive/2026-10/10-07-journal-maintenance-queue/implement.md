# Implementation

Use ../10-07-journal-online-maintenance/implement.md for gates and sequence.

Own Ports/Application maintenance modules and adapter maintenance_queue.rs plus related tests. First publish exact shared interface signatures to the coordinator and storage/CLI workers. Keep queue persistence separate from catalog. Do not edit adapter lib.rs/Cargo.toml or CLI files; request module declarations/dependencies from owners.

Read applicable specs, inspect current code first, edit directly in the shared workspace, run targeted tests, report exact changed files and evidence. Do not spawn nested agents or commit.

## Implemented and verified
Ports72, Application299 (maintenance15), queue14 passed; scoped fmt and three-package all-target Clippy passed. Independent safety review verified logical lost-ack/cancel and cleanup-cutoff fixes.

See ../10-07-journal-online-maintenance/research/verification.md for final integrated evidence. Code is implemented and locally checked; user-approved commits and bookkeeping are being finalized. No cross-platform CI claim.
