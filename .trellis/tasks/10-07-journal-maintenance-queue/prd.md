# Contracts, queue and scheduling

Parent requirements: ../10-07-journal-online-maintenance/prd.md

Own Ports/Application maintenance modules and adapter maintenance_queue.rs plus related tests. First publish exact shared interface signatures to the coordinator and storage/CLI workers. Keep queue persistence separate from catalog. Do not edit adapter lib.rs/Cargo.toml or CLI files; request module declarations/dependencies from owners.

All relevant parent acceptance criteria apply. This is approved implementation, not new requirements discovery.
