# SQLite safe maintenance executor

Parent requirements: ../10-07-journal-online-maintenance/prd.md

Own adapter maintenance.rs, necessary lib.rs/relocation.rs helpers, adapter Cargo.toml, and storage integration tests. Declare both maintenance and maintenance_queue modules when files exist. Keep the original B2 API semantics and tests unchanged. Coordinate Ports signatures with queue worker. Do not edit queue implementation or CLI/Application.

All relevant parent acceptance criteria apply. This is approved implementation, not new requirements discovery.
