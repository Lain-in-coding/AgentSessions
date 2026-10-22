# Journal online maintenance and physical reclamation

## Approval and goal
The user approved the decision-complete plan in this conversation and requested implementation. Task-creation consent was already granted during planning. Deliver a durable, online-first maintenance workflow with verified physical disk reclamation, not merely reduced journal strings.

## Requirements
- Preview before irreversible compaction; submission freezes the confirmed terminal batch selection. Changes before submission reject the token; later new batches are excluded; changed selected batches require a new preview.
- Durable independent queue; on-demand detached worker survives CLI exit. No system service/autostart installation. Restart after crash/reboot on the next authorized invocation.
- Worker retries transient contention without needing another CLI invocation. Read-only commands, help/version, preview/status never wake it.
- Successful writing commands may wake an existing unpaused queue only after releasing catalog writer lease. Do not create queues when none exists.
- Default per-acquisition writer-lock soft budget 30 seconds; explicit positive override only. Never silently increase it.
- One verified maintenance-before-state backup per job; successful verification and cleanup required for completion. Failed/cancelled jobs retain verified backup, never auto-restore over newer writes.
- Logical compaction followed by in-place VACUUM, checked WAL checkpoint/TRUNCATE, integrity and logical invariant validation, backup cleanup.
- Maintain catalog schema v19, old B2 API behavior, stable identities, generation, FTS mappings, ranking/cursors, existing protocol envelope 1.1/privacy/error categories.
- Queue/status/cancel work without opening/migrating the catalog, including when it is absent or busy. Unsupported/replaced targets fail closed.
- Explicit persistent worker pause; automatic wakes never unpause. Cancellation cannot undo committed work.

## Acceptance
- Existing quality gates and B2 journal tests remain green.
- Real CLI/worker tests show 200-message/21-revision fixture catalog shrinks and total footprint including queue/WAL/SHM/remaining backups also shrinks.
- Multiprocess tests cover single executor, enqueue versus idle exit, automatic progress after lock release with no new CLI, detached survival, crash recovery, long-reader checkpoint deferral, pause/start and no read-triggered wake.
- Fault injection covers backup publication, logical commit before queue acknowledgement, VACUUM/checkpoint boundaries and cleanup. No duplicate logical compaction and no unauthorized expansion.
- Report measured stage durations, max lock duration, memory/disk footprint and real reclaimed bytes; no invented search-speed claims.

## Exclusions
No E5/ANN, GUI, periodic GC, system service, general parser rewrite, user-data VACUUM during development, or claim that this completes the earlier 15-project audit.
