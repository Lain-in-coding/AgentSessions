# Real-data regression: recorded authorized local run

> Evidence record
>
> - evidence id: `IB-REAL-DATA-REGRESSION-001`
> - status: **executed**; the fixed-binary subset run passes all six
>   invariants (green), while the full-corpus rerun remains **open** (slow
>   large-corpus batches, no code failure observed).
> - latest full run: generated `2026-07-31T10:04:17Z` (Friday, July 31,
>   2026 local date), outcome `failed` — recorded below as-is.
> - harness: `scripts/evidence/real_data_regression.py`
> - authorization: the operator's own machine and transcripts; data never
>   leaves the host. This file carries aggregate-only facts.

## Method

The harness built a throwaway temporary data root, ingested the authorized
Claude Code and Codex corpus through the real release binary in `--robot` mode,
and was configured to evaluate six invariants. Sources remained read-only.

The generated report matched the exact closed aggregate key sets. A raw-value
scan found no Windows, Unix-home, or UNC paths, UUIDs, or complete stable entity
IDs. The closed schema has no source-fingerprint, username, hostname,
transcript-content, or prompt fields; the binary SHA-256 is the only permitted
hash. Process stderr was empty. The report remains under the gitignored
`evidence-output/` directory and is not committed.

## Latest full-run result (2026-07-31)

The full authorized run failed at the first invariant:

| Item | Value |
|---|---|
| Generated | `2026-07-31T10:04:17Z` |
| Corpus | 879 source files, 756,515,768 bytes |
| Outcome | `failed` |
| Sync result | exit 5, `ok: false`, 79,958 emitted records, 0 skipped |
| Downstream invariants | not evaluated because sync failed |

```text
INV-SYNC-OK           FAIL  exit 5, ok false, emitted 79958, skipped 0
INV-NO-PARSE-LOSS     NOT EVALUATED: sync failed
INV-SESSION-PRESENT   NOT EVALUATED: sync failed
INV-CONTEXT-NONEMPTY  NOT EVALUATED: sync failed
INV-SPAN-COVERAGE     NOT EVALUATED: sync failed
INV-REBUILD-STABLE    NOT EVALUATED: sync failed
```

Exit 5 places the failure in the canonical snapshot/I/O family. The aggregate
report did not retain the exact canonical error code, so this evidence does
**not** prove that the specific code was `source_changed`.

## Follow-up final-batch replay (2026-07-31)

A follow-up replay of the then-current final batch succeeded:

| Item | Value |
|---|---|
| Batch | 5 of 5 |
| Sources | 186 total: 89 Claude Code, 97 Codex |
| Bytes | 223,269,278 |
| Process result | exit 0 |
| Parse result | 15,549 emitted, 0 skipped, 0 diagnostics |

This shows that the full-run failure was not persistently reproducible on that
batch. It does not turn the failed full run green, and it does not evaluate any
of the five downstream invariants. The active local provider roots were
observed changing during follow-up, so a stable full-root rerun remains
required.

## Fixed-binary subset run (2026-08-10, green)

After the code-review fixes (message-level leaf exclusion, JSON-aware rebuild
projection, timestamp merge convergence, unknown-role skip accounting,
message-contexts NotFound semantics, cursor/budget hardening), the release
binary was rebuilt and run over a real Claude Code corpus directory
(`C--Users--Q-Desktop-ObsidianVault------20260620-215544`, 137 source files)
that previously stalled the old binary. **All six invariants pass**:

```text
INV-SYNC-OK           PASS  exit 0, ok=True, 137 sources, 15246 emitted, 0 skipped
INV-NO-PARSE-LOSS     PASS  provider emitted 15246, persisted 15246 claims, skipped 0
INV-SESSION-PRESENT   PASS  17 sessions for 137 source files
INV-CONTEXT-NONEMPTY  PASS  17 sessions, 0 failed, 0 unexpected empty, 0 internal
INV-SPAN-COVERAGE     PASS  41/41 spans have byte precision
INV-REBUILD-STABLE    PASS  rebuild exit 0, catalog 14140 -> 14140, sampled terms match
```

This proves the fixed binary produces a green six-invariant result on real
data, and that the previously observed stall on this directory is resolved.

## Full-corpus rerun status (open)

A full-corpus rerun with the fixed binary was attempted (2026-08-09/10). The
run ingests ~1,124 source files in ~10 chunked sync batches; batches containing
the `AgentHub-novella2` corpus are measurably slow (single-batch isolated runs
take 2-5 minutes vs sub-second for ordinary batches), and the run was
terminated after the fifth batch (~488 MB ingested, ~64% of the corpus) without
a completed result. No code failure was observed; the batches that committed
advanced the catalog normally and the subset run above is green. A full-corpus
six-invariant green run therefore remains **open**, blocked by the slow
large-corpus batches rather than by any observed defect.

## What these runs establish and do not establish

Establishes:

- The authorized aggregate-only process executes over real sources without
  recording disallowed report fields or emitting stderr.
- The fixed binary passes all six invariants on a real Claude Code corpus
  (137 files, 15,246 emitted, 0 skipped) that previously stalled the old
  binary.
- The 2026-07-31 full run failed at `INV-SYNC-OK` (exit 5) and its final batch
  could be replayed successfully.

Does not establish:

- A green full-corpus Gate D. The 2026-07-31 full run failed and the
  fixed-binary full-corpus rerun has not completed (slow large-corpus
  batches).
- The exact canonical error within the exit-5 family.
- Any provider promotion. Claude Code and Codex remain **Experimental**.

## Next step

Complete the full-corpus rerun with the fixed binary (the slow
`AgentHub-novella2` batches may need smaller chunking or per-directory
runs). Acceptance still requires harness exit 0 and all six invariants passing
in one aggregate-only full-root run.
