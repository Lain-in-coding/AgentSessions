# PRD: Cross-source session identity

Parent: `07-24-advance-integration-beta` child 8. Opened because the authorized
real-data regression (child 7) proved the current store cannot ingest a real
corpus at all. See `docs/evidence/integration-beta/real-data-regression.md`.

## The defect

On a real corpus (634 Claude Code transcripts, 73 Codex rollouts) `sync` fails
with exit 6:

```
catalog_error: message ses_v1_db969015-... has conflicting projections across sources
```

Verified root cause: **one Claude Code session routinely spans many `.jsonl`
files.** For the session named in that error, 55 distinct files each declare it
as their own `sessionId` (each file internally has exactly one sessionId, so
this is not intra-file mixing — it is one conversation continued/sharded across
files).

The store rejects it because `staged_to_entries` makes every source emit its own
copy of the session container entity carrying `{document, messages[]}`. That
member list is inherently a cross-source aggregate, so two sources claiming the
same `ses_v1_` produce byte-different payloads for one wire id, and
`commit_source_batches_if_changed` refuses the whole batch.

Synthetic fixtures are one-file-per-session, so this path was never exercised.

## Normative grounding

`RFC-0001 §3.2` defines `Session` with `source_instance_id` and **no**
`document` field. "One session belongs to one document" is an implementation
overreach, not a model requirement. This task brings the implementation back in
line with the RFC; it does not amend the RFC.

## Requirements

- R1 One session entity may be claimed by many sources. Committing sources that
  share a session must succeed, and the resulting session entity must cover the
  union of members across all of them.
- R2 Container entities (session, document) merge by union at commit time
  instead of requiring byte equality. Message entities keep the existing strict
  rule: a message id claimed twice with different content is still a conflict
  (that would be real identity corruption).
- R3 Member ordering stays deterministic and reproducible regardless of the
  order sources are passed on the command line, so ids and payloads are stable
  across re-ingests.
- R4 A session spanning several documents must reference all of them. The
  single-valued `document` field cannot express that; the payload gains a
  documents list. Keep `document` as the first entry for backward compatibility
  with stores written before this change, and treat its absence as honest
  missing data rather than fabricating one.
- R5 Incremental re-sync of a subset of a shared session's sources must not drop
  the members contributed by sources not in this batch. Tombstone derivation
  already consults other sources' membership; verify it holds for container
  entities too.
- R6 The read path (`context`, MCP `get_session_context`, TUI) keeps working
  unchanged for single-source sessions and correctly assembles multi-source
  ones. Evidence spans must still resolve to the document that actually
  contains each message, not to an arbitrary one.
- R7 Regression coverage: a golden or e2e case where two files declare one
  session, asserting union membership, deterministic order, and a working
  `context` over the merged session. Plus the authorized real-data regression
  re-run reaching a verdict on all six invariants.

## Acceptance criteria

- [ ] Two-file shared-session e2e passes: `sync` exits 0, the session entity
      lists members from both files in deterministic order, `context` assembles
      the merged chain.
- [ ] Re-running `sync` on only one of the two files preserves the other's
      members.
- [ ] Message-level conflicts still fail loudly (negative test retained).
- [ ] Pre-change stores still readable: sessions with the old single-`document`
      payload assemble without error.
- [ ] Authorized real-data regression re-run on the local corpus reaches a
      verdict on all six invariants; the result is recorded honestly whatever it
      is, and the evidence file plus maturity matrix are updated to match.
- [ ] Full gates green: `cargo fmt --all --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check`.
- [ ] No new dependencies. No changes to the ports traits (see design §0).

## Out of scope

- Amending RFC-0001 or marking any governance record Accepted.
- Session merging across providers, or merging sessions that do not share a
  provider-native session id.
- Promoting either provider to Beta — that stays the owner's decision after CI
  and review.
- Codex-side session identity changes beyond what R1-R6 require (Codex rollouts
  are one-file-per-session today).
