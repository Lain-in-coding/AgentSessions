# Bounded maintenance safety review

Date: 2026-10-07. Scope: adapter backup intent/publication/adoption, target identity,
per-acquisition deadlines, and Application/Ports cancellation and lost queue acknowledgements.
CLI parsing/help and general style were outside this review. Product files remained
owned by the storage and queue/Application implementers; this reviewer made no product edits.

## Findings and owner fixes

### R1: cancellation after successful cleanup could falsely promise a retained backup

- Original evidence: Application `src/maintenance.rs:167-184` checked interruption before
  `phase == Done`; storage `cleanup:600-608` deleted `before.sqlite` before the queue
  acknowledgement; queue `request_cancel:252-265` accepted cancellation of noncompleted Done.
  These are the pre-format/pre-fix line numbers observed during review.
- Reproduction: let Cleanup delete the backup successfully; deliver cancellation before
  its phase acknowledgement or the next loop iteration. The job became Cancelled even
  though its verified backup was gone. A process crash in the same gap had equivalent ambiguity.
- Reported directly to both owners and coordinator. Coordinator approved an explicit
  durable cleanup cutoff, now recorded in design.md. Queue claims `cleanup_started`
  atomically against prior cancellation; requests after the claim expose
  `cancellation_closed` instead of claiming cancellation. Done finalizes without
  reopening/acquiring the catalog. Claimed cleanup recovery uses a filesystem-only
  session so a subsequently absent/replaced/busy catalog does not prevent owned cleanup.
- Reviewer independently ran the new Application before-claim, during-cleanup and
  cleanup-success/lost-ack regressions successfully; final storage/queue runs also passed.
- Follow-up fixed by storage owner: normal-session footprint errors after successful
  deletion map to CleanupFailed/NeedsAttention. The reviewer independently passed
  `post_delete_measurement_error_is_resumable_cleanup_failure`, including cleanup-only retry.

### R2: checkpoint deferral discarded the pre-VACUUM invariant baseline

- Original evidence: storage `run_stage:614` initialized a per-session baseline, but
  compared it only in Verify at `647-648`. A successful VACUUM followed by Busy checkpoint
  released that session. The next session compared a post-VACUUM baseline to itself.
- Reproduction: inject an unintended logical/generation change in the destructive
  acquisition, then force checkpoint contention. After retry, original code could no
  longer detect that change. This is a concrete missing verification boundary, not a
  claim that SQLite VACUUM normally corrupts data.
- Storage owner accepted and added comparison after Compact and VACUUM before successful
  phase acknowledgement, and before cleanup. This preserves per-acquisition checking
  without rejecting legitimate writes between attempts.
- Requested regression: mutate invariant content before a forced checkpoint-deferral
  boundary and assert IntegrityFailed before releasing/accepting the destructive phase.
  Owner added `destructive_stage_checks_invariants_before_checkpoint_can_defer`.

## Other paths verified against source

- Frozen subset validation checks selected IDs, states, generations, operation/detail
  digests and counts. Atomic compaction puts the audit and selected rewrites in one
  catalog transaction; no standalone new staged audit survives cancellation.
- Backup intent is returned/persisted before copying; a changed source intent returns
  to Backup without starting a copy. Incremental Backup steps share the session deadline.
- Publication is an owned, deterministic, no-clobber hard link; complete temporary copies
  and published copies are adopted only after integrity/schema/generation/invariant checks.
  A verified backup is not replaced just because newer writes arrive between retries.
- Ownership checks constrain the exact job-relative path, reject linked directories/files,
  validate the owner marker and use nonrecursive deletion of an allowlist.
- Catalog matching uses canonical path plus OS file identity, not size/mtime/content.
  Current Windows implementation uses full FILE_ID_INFO; Unix uses device/inode.
  No creation/migration/reprojection is invoked by the maintenance catalog opener.
- Lost logical-commit acknowledgements reconcile a read-only catalog audit before
  cancellation finalization. Public commit status is nullable while unresolved.
- Writer deadline is created after lease acquisition and is not renewed between stages.
  SQLite progress callbacks use atomics only; checkpoint inspects Busy/log/done values.
  An acknowledged VACUUM is not repeated merely because checkpoint needs another attempt.
- Initial deadline coverage only tested expired Validate. Owner added later-stage
  expired-deadline and mid-Backup cancellation/resume coverage after review request.

## Verification log

- Initial in-flight snapshots: cargo check workspace/all-targets passed. Formatting and
  clippy briefly failed on unfinished owner edits; the next workspace fmt/check/clippy
  run all passed. A maintenance test compile briefly failed while a new shared DTO field
  had not yet reached storage fixtures; owner synchronized it. These are not unresolved findings.
- Independently executed `cargo test -p agent-session-grep-application maintenance`:
  14 passed at that snapshot (including R1 regressions).
- Independently executed `cargo test -p agent-session-grep-adapters-sqlite --test journal_retention`:
  13 passed.
- Final independently executed targeted runs: storage maintenance **17/17**, queue **14/14**,
  Application maintenance **15/15**, all passed. No CLI executable was rebuilt for these runs.
- Whole-workspace tests and multiprocess CLI acceptance remain coordinator-owned; this
  bounded review does not claim full task completion or cross-platform execution.
## Final source assertions (post-fix snapshot)

- R1: `crates/agent-session-grep-application/src/maintenance.rs:134-141,175-225`
  finalizes Done first, persists cleanup claim before execution and filters only late
  cancellation (not pause). `crates/agent-session-grep-adapters-sqlite/src/maintenance_queue.rs:205-259,261-293`
  performs the claim/cancel decision in an Immediate transaction, preserves the claim
  once true, and refuses late cancellation. Ports `src/maintenance.rs:156-157,174,191`
  names the irreversible commitment and public closed status explicitly.
- R1 recovery: storage `src/maintenance.rs:376-382` selects CleanupSession before opening
  or validating the catalog when the claim is durable. CleanupSession reports
  `holds_writer_lease() == false`, removes only owned files and maps remaining cleanup
  failures to attention. Normal-session post-delete metrics errors at `1185-1191` do
  the same. Tests `src/maintenance/tests.rs:642-684` cover missing catalog/held writer
  lease after lost acknowledgement and a real post-delete measurement obstruction.
- R2: storage `src/maintenance.rs:1178-1182` checks pre/post invariants for BOTH Compact
  and VACUUM before successful phase return; `1170-1174` checks before cleanup.
  `src/maintenance/tests.rs:613-639` injects generation drift with a live reader and
  confirms IntegrityFailed at VACUUM before the checkpoint can lose the baseline.
- Deadline/backup regressions now include later phases and real mid-copy interruption,
  not merely an expired Validate call. The storage 17-test run covers adoption,
  no-clobber publication, subset drift, atomic failure, identity replacement,
  retained backup, checked checkpoint and physical reclamation.
- Task `design.md` explicitly records the coordinator-approved cleanup cutoff;
  affected package specs were synchronized by the coordinator/owners. This reviewer
  did not edit shared specs or product files.

## Final conclusion

Both original safety findings and the post-delete measurement follow-up are fixed and
independently verified. No unresolved high-risk defect was found within this bounded
scope. This is not a substitute for the coordinator's final whole-workspace and CLI
multiprocess gates. Tests ran locally on Windows; no cross-platform execution or
power-loss fault coverage is claimed.