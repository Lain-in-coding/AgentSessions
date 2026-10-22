# Implementation

Use ../10-07-journal-online-maintenance/implement.md for gates and sequence.

Own adapter maintenance.rs, necessary lib.rs/relocation.rs helpers, adapter Cargo.toml, and storage integration tests. Declare both maintenance and maintenance_queue modules when files exist. Keep the original B2 API semantics and tests unchanged. Coordinate Ports signatures with queue worker. Do not edit queue implementation or CLI/Application.

Read applicable specs, inspect current code first, edit directly in the shared workspace, run targeted tests, report exact changed files and evidence. Do not spawn nested agents or commit.

## Implemented and verified
Adapter334 passed, including maintenance19, B2 journal13, relocation42 and queue14; all-target Clippy passed. Independent safety findings fixed and reverified. Full coordinator workspace gates passed.

See ../10-07-journal-online-maintenance/research/verification.md for final integrated evidence. Code is implemented and locally checked; user-approved commits and bookkeeping are being finalized. No cross-platform CI claim.
