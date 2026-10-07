# Design: journal maintenance v1

## Boundaries
Ports own typed maintenance DTOs and interfaces; Application owns orchestration, cancellation, retries and recovery policy without SQLite/filesystem imports. Adapter owns independent durable queue and catalog operations. CLI parses/routes before generic store open, composes services, and spawns detached processes.

Queue v1 lives at the catalog writer-lease data-root under `.maintenance/queue.sqlite`; never under Trellis. Jobs bind canonical absolute catalog path and file identity; each job retains its own target regardless of subsequent CLI args. Root-scoped queue transactions, worker OS lock, short lifecycle lock and catalog writer lease are separate. Default queue must use durable short transactions and preserve cancel/pause fields against concurrent worker progress updates. Existing catalog stays schema 19.

## Public commands
`asg [--db catalog] journal preview`
`asg [--db catalog] journal submit --plan TOKEN [--max-write-seconds N]`
`asg [--db catalog] journal status [JOB_ID]`
`asg [--db catalog] journal cancel JOB_ID`
`asg [--db catalog] journal retry JOB_ID [--max-write-seconds N]`
`asg [--db catalog] journal worker start|stop`
The detached entrypoint is internal. `start` clears persistent pause; internal worker never does. `stop` sets pause, interrupts safely and preserves jobs. Optional status ID lists current target's jobs and root worker state. Preserve global prefix-only argument placement.

## Confirmation and persistence
Preview is read-only and does not create the queue. Token contains a versioned hash binding canonical target/file identity/schema/selection digest, not raw paths or row IDs. Submit recomputes and compares before persisting the thin frozen manifest. Same token is idempotent, including after successful completion. Preview random compaction IDs must NOT make stable selection token comparison impossible; assign/persist a single execution compaction ID per accepted job.
Jobs retain typed selection IDs/digests/counts (no raw detail bodies), target, token, soft budget, state/phase, attempts, next retry time, cancellation, backup intent/verification, stage recovery records and metrics. Public status projects safe bounded summaries rather than serializing private job records.

## Execution
Phases: validate target/selected rows -> verified backup -> atomic selected-batch compaction -> in-place VACUUM -> checked TRUNCATE checkpoint -> integrity/invariant verification -> cleanup -> completed.
New selected-batch path validates exactly the stored subset and writes audit/aggregates/committed state in ONE catalog transaction. Preserve existing B2 full-preview stage/apply/recover behavior; extract only shared internal helpers needed. New jobs do not leave standalone staged audit rows that unrelated recovery could apply after cancellation.
Use maintenance open with writer lease but no create/migration/reprojection; extract from relocation opener without changing relocation compatibility. Backup API is incremental/deadline-aware with typed errors for maintenance; legacy relocation wrapper behavior remains unchanged.

## Backup, recovery and validation
One owned backup at a deterministic job-private path, no clobber. Persist source snapshot generation/invariants and intent before copying; verify integrity/schema/generation and publication before logical mutation. On crash after publication, validate and adopt only the owned matching backup. Reuse verified backup across retries; it is a pre-maintenance snapshot, not a continuous backup of later writes. Never auto-restore.
Catalog audit is authority for whether logical compaction committed even when queue ack is absent. Once VACUUM is known complete persist phase and do not repeat solely for checkpoint contention. A crash before VACUUM ack may repeat safe physical work, never logical compaction. Compare current attempt's pre/post invariants under writer lease; legitimate writes between attempts must not cause false drift. Integrity validation includes FTS rowid sidecar mapping and logical contents, stable IDs and generation, not byte-identical DB files.
Source/WAL/SHM, queue and owned backup/temp footprint are separately measured. Check available space for remaining backup + VACUUM (up to two database sizes extra) + WAL/reserve before destructive steps. Worker uses private temp directory, not SQLite global temp_directory. Retained older job backups already occupy space and cannot be deleted for this job.
Failure/cancel retain verified backup. Successful cleanup verifies resolved ownership, closes handles and removes only job-owned files. Cleanup failure is needs_attention and retry only resumes cleanup. Never report completion with leftover required cleanup.

## Scheduling and process lifecycle
States: queued/deferred/running/needs_review/needs_attention/completed/failed/cancelled; separate phase/reason/cancel_requested. Busy retries at 1,2,5,15,30,60 seconds capped at 60. Keep worker alive while retryable jobs exist. Three consecutive budget exhaustion attempts without durable progress -> needs_attention; reset on durable stage progress. Disk insufficiency/permissions require attention; selected drift or target replacement requires review.
Soft deadline starts after lease acquired and applies to backup/SQL/checkpoint/verification in that acquisition. Use progress handler and per-step checks, never reentrant SQLite in callbacks. Cancellation/pause delivered through independent atomic control. Noninterruptible OS I/O may exceed deadline; document and measure rather than promising hard real time.
Root worker OS lock is authoritative. Lifecycle lock must serialize producer durable enqueue/check/start against final worker queue check/lock release/idle exit. Idle exit after 30 seconds with no runnable/automatically-retryable work. Existing writer wake happens after lease release even on stdout broken pipe; reads and no-queue writes have no wake/create side effects. Windows detach hidden with stdio disconnected, no shell-built command strings. Spawn failure leaves accepted durable task with honest worker state.

## Compatibility and tests
Robot envelope 1.1, bounded dotted success command names, existing root error command behavior and canonical errors. No native IDs/body/absolute private paths even in debug errors. Deferred is not Outcome::Partial. Keep serve mutations 501 and do not introduce query-service maintenance handshakes.
Unit, storage failure injection and real process tests cover token/selection drift, cancellation races, pause, exact recovery commit boundary, enqueue/exit race, no-new-request resume, detached child, checked busy checkpoint, unsupported/missing/replaced catalogs, deadline/space/permission/cleanup failures, output flags/help/broken pipes and stable FTS/search/cursors. Synthetic fixtures only.

## Implementation ownership
Queue worker owns Ports maintenance module, Application maintenance module, adapter maintenance_queue module and their tests. Storage worker owns adapter maintenance module, necessary lib/relocation internals and adapter Cargo feature changes. CLI worker owns CLI maintenance/process routing, flags/help/protocol and CLI tests. Main coordinates contracts, task artifacts, product/spec docs and integration evidence. Workers must agree on exported signatures early and not revert each other's changes.

## Reviewed implementation refinements
- Logical-commit acknowledgement gaps use a read-only catalog reconciliation seam; public commit status is nullable until known, never false merely because queue ack is absent.
- Cleanup has an explicit durable commit intent, atomically claimed against prior cancellation. Before the claim cancellation retains backup; after it cancellation is closed and reported (`cancellation_closed`), since physical deletion may already have succeeded. Finish/retry cleanup and finalize Done without claiming a retained backup. Regression tests cover cancellation on both sides and cleanup success before lost queue ack.
- Timing summaries aggregate per phase rather than grow per retry. Reclaimed bytes reflect latest matching before/after footprint, not max historical observations.
- Concurrent first queue initialization checks/creates schema inside one Immediate transaction. New private maintenance directories use Unix0700 without changing caller parent permissions.
