# Real-data regression: recorded authorized local run

> Evidence record
>
> - evidence id: `IB-REAL-DATA-REGRESSION-001`
> - status: **passed** — the fixed-binary full-corpus run (2026-08-09/10)
>   satisfies all six invariants with harness exit 0.
> - latest full run: generated `2026-08-09T21:10:16Z` (Sunday, August 9,
>   2026 local date), outcome `passed`.
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

## Green full-corpus run (2026-08-09/10, PASSED)

The full authorized run over both provider roots passes all six invariants:

| Item | Value |
|---|---|
| Generated | `2026-08-09T21:10:16Z` |
| Corpus | 1,242 source files, 1,177,479,794 bytes |
| Outcome | `passed` |
| Sync result | exit 0, `ok: true`, 164,136 emitted records, 0 skipped |
| Catalog | 150,091 de-duplicated `msg_v1_` entities, 231 sessions, 1,240 documents |

```text
INV-SYNC-OK           PASS  exit 0, ok=True, 1242 sources, 164136 emitted, 0 skipped
INV-NO-PARSE-LOSS     PASS  provider emitted 164136, persisted 164136 claims, skipped 0
INV-SESSION-PRESENT   PASS  231 sessions for 1242 source files
INV-CONTEXT-NONEMPTY  PASS  231 sessions, 0 failed, 11 zero-placement, 0 internal
INV-SPAN-COVERAGE     PASS  659/659 spans have byte precision
INV-REBUILD-STABLE    PASS  rebuild exit 0, catalog 151562 -> 151562, sampled terms match
```

This is the corpus-wide green Gate D result: stable full-root, all six
invariants, harness exit 0, aggregate-only report. Provider Beta promotion
gates that depend on this evidence are now met (independent review and
owner decision still apply).

## Earlier failed full run (2026-07-31, recorded as-is)

The earlier full authorized run failed at the first invariant:

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

## Fixed-binary subset run (2026-08-10)

After the code-review fixes (message-level leaf exclusion, JSON-aware rebuild
projection, timestamp merge convergence, unknown-role skip accounting,
message-contexts NotFound semantics, cursor/budget hardening), the release
binary was rebuilt and run over a 137-file real Claude Code corpus directory
that previously stalled the old binary. **All six invariants pass**:

```text
INV-SYNC-OK           PASS  exit 0, ok=True, 137 sources, 15246 emitted, 0 skipped
INV-NO-PARSE-LOSS     PASS  provider emitted 15246, persisted 15246 claims, skipped 0
INV-SESSION-PRESENT   PASS  17 sessions for 137 source files
INV-CONTEXT-NONEMPTY  PASS  17 sessions, 0 failed, 0 unexpected empty, 0 internal
INV-SPAN-COVERAGE     PASS  41/41 spans have byte precision
INV-REBUILD-STABLE    PASS  rebuild exit 0, catalog 14140 -> 14140, sampled terms match
```

This proved the fixed binary produces a green six-invariant result on real
data, and that the previously observed stall on this directory is resolved.
A subsequent fix added `Role::Developer` read-path support, which the green
full-corpus run above includes.

## What these runs establish and do not establish

Establishes:

- A green full-corpus Gate D: 1,242 sources, 164,136 emitted, 0 skipped,
  all six invariants pass, harness exit 0 (2026-08-09T21:10:16Z).
- The authorized aggregate-only process executes over real sources without
  recording disallowed report fields or emitting stderr.
- The 2026-07-31 full run failed at `INV-SYNC-OK` (exit 5); its final batch
  could be replayed successfully.

Does not establish:

- Any provider promotion by itself. Claude Code and Codex remain
  **Experimental** pending independent review and owner decision; the green
  run closes the real-data gate, not the full promotion checklist.

## Next step

The corpus-wide green run closes the previously open Gate D. Remaining
promotion steps (independent review, CI certification across formal targets)
are tracked in the provider maturity matrix.
