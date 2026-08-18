# Real-data regression: recorded authorized local run

> Evidence record
>
> - evidence id: `IB-REAL-DATA-REGRESSION-001`
> - status: **passed** — the latest fixed-binary full-corpus run
>   (2026-08-13) satisfies every invariant the harness evaluated at that time
>   with harness exit 0.
> - latest full run: generated `2026-08-13T01:46:55Z` (Thursday, August 13,
>   2026 local date), outcome `passed` (v5, review-fixed renamed binary).
> - harness: `scripts/evidence/real_data_regression.py`
> - authorization: the operator's own machine and transcripts; data never
>   leaves the host. This file carries aggregate-only facts.
> - invariant set: the runs below were executed against the six invariants the
>   harness defined at the time (`INV-SYNC-OK` through `INV-REBUILD-STABLE`).
>   `INV-SOURCES-UNCHANGED` was added on 2026-08-16, after these runs, bringing
>   the current set — authoritatively `INVARIANT_IDS` in the harness — to seven.
>   Each run below is reported with the invariant count it actually evaluated;
>   the counts are not retroactively restated.

## Synthetic 100k-message run (2026-08-19) — seven invariants, full coverage

A separate run on the deterministic synthetic corpus
(`scripts/evidence/synthetic_corpus.py`, 100,000 messages / 5,000 sessions /
6 providers / 48.6 MiB). This is **not** a substitute for the authorized
real-corpus runs recorded below — a synthetic corpus cannot surface the format
irregularities real transcripts carry. It is recorded because it is the first
run at the scale every release threshold is stated at, and because it exposed
two harness defects that the real-corpus runs could not have revealed.

| Aggregate | Value |
|---|---|
| sources | 5,000 |
| emitted records | 100,000 |
| skipped | 0 |
| persisted source-placement claims | 100,000 |
| de-duplicated `msg_v1_` entities | 100,000 |
| sessions | 5,000 (0 failed, 0 zero-placement) |
| spans | 43,894 total — 43,019 byte-precise, 875 declared-absent |
| rebuild | catalog 110,000 → 110,000, ids match, 5 sampled terms match |
| sources unchanged | 5,000 of 5,000 |
| outcome | **passed**, harness exit 0, 440 s |

Reproduce with a generated corpus (the corpus itself is never committed):

```
python scripts/evidence/synthetic_corpus.py generate
python scripts/evidence/real_data_regression.py \
    --binary target/release/agent-session-grep \
    --sources evidence-output/synthetic-corpus-100k/corpus
```

**Two harness defects this run exposed**, both fixed before the numbers above
were taken:

1. `collect_sources` kept only `.jsonl`, so 3,675 of 5,000 files were collected
   and the harness still reported every invariant green — **a quarter of the
   corpus silently excluded by a report that read as full coverage.** Candidacy
   is now an explicit short exclusion list rather than an allow-list of
   transcript extensions, so a provider format nobody anticipated reaches the
   tool instead of vanishing.
2. With full coverage restored, `INV-SPAN-COVERAGE` failed: 875 spans were not
   byte-precise. Those 875 are exactly the cline (450) and hermes (425)
   documents, both declaring `source_span: Unsupported` because a whole-JSON
   document has no in-file byte range. `Precision::Unknown` is the documented
   correct value for that case, so the invariant had been demanding something
   untrue. It now requires at least one byte-precise span, rejects any tier
   outside the contract, and reports the declared-absent count with its reason.

The 875 declared-absent spans are therefore an honest capability limit, not a
regression; they are reported rather than hidden so the number stays auditable.

## Method

The harness built a throwaway temporary data root, ingested the authorized
Claude Code and Codex corpus through the real release binary in `--robot` mode,
and was configured to evaluate the six invariants then defined. Sources remained
read-only.


The generated report matched the exact closed aggregate key sets. A raw-value
scan found no Windows, Unix-home, or UNC paths, UUIDs, or complete stable entity
IDs. The closed schema has no source-fingerprint, username, hostname,
transcript-content, or prompt fields; the binary SHA-256 is the only permitted
hash. Process stderr was empty. The report remains under the gitignored
`evidence-output/` directory and is not committed.

## Green full-corpus run (2026-08-09/10, PASSED)

The full authorized run over both provider roots passes all six invariants then defined:

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
invariants then defined, harness exit 0, aggregate-only report. Provider Beta
promotion gates that depend on this evidence are now met (independent review and
owner decision still apply).

## Latest green full-corpus runs (2026-08-12 v3 and 2026-08-13 v4, PASSED)

Two later full-corpus runs over the same authorized roots also pass all six
invariants then defined. The numbers below are aggregate counts quoted from the
gitignored `evidence-output/` reports; the reports themselves are not copied
into the repository.

| Item | v3 (2026-08-12) | v4 (2026-08-13) |
|---|---|---|
| Generated | `2026-08-12T23:51:23Z` | `2026-08-13T00:26:57Z` |
| Binary | `agentsessions.exe` (pre-rename name, 0.1.0) | `agent-session-grep.exe` (renamed, 0.1.0, sha256 `423319076286d288b954282716e3c0884c3d55cb66c9edd83eebe38149c4e073`) |
| Corpus | 1,328 source files, 1,253,494,481 bytes | 1,330 source files, 1,255,049,984 bytes |
| Outcome | `passed` | `passed` |
| Sync result | exit 0, 180,218 emitted records, 0 skipped | exit 0, 180,718 emitted records, 0 skipped |
| Catalog | 164,812 de-duplicated `msg_v1_` entities, 242 sessions, 1,326 documents | 165,312 de-duplicated `msg_v1_` entities, 242 sessions, 1,328 documents |
| Rebuild | catalog 166,380 -> 166,380, ids match | catalog 166,882 -> 166,882, ids match |

```text
v3 2026-08-12T23:51:23Z
INV-SYNC-OK           PASS  exit 0, ok=True, 1328 sources, 180218 emitted, 0 skipped
INV-NO-PARSE-LOSS     PASS  provider emitted 180218, persisted 180218 claims, skipped 0
INV-SESSION-PRESENT   PASS  242 sessions for 1328 source files
INV-CONTEXT-NONEMPTY  PASS  242 sessions, 0 failed, 11 zero-placement, 0 internal
INV-SPAN-COVERAGE     PASS  630/630 spans have byte precision
INV-REBUILD-STABLE    PASS  rebuild exit 0, catalog 166380 -> 166380, sampled terms match

v4 2026-08-13T00:26:57Z
INV-SYNC-OK           PASS  exit 0, ok=True, 1330 sources, 180718 emitted, 0 skipped
INV-NO-PARSE-LOSS     PASS  provider emitted 180718, persisted 180718 claims, skipped 0
INV-SESSION-PRESENT   PASS  242 sessions for 1330 source files
INV-CONTEXT-NONEMPTY  PASS  242 sessions, 0 failed, 11 zero-placement, 0 internal
INV-SPAN-COVERAGE     PASS  630/630 spans have byte precision
INV-REBUILD-STABLE    PASS  rebuild exit 0, catalog 166882 -> 166882, sampled terms match
```

The v4 run is the first full-corpus Gate D executed by the renamed
`agent-session-grep` binary, confirming the project rename introduced no
functional regression on the real corpus (v3 ran the pre-rename binary and
serves as the comparison baseline at the same corpus scale).

## Latest run: v5 (2026-08-13, review-fixed binary, PASSED)

The v5 run exercised the binary rebuilt after the full-repo review fixes
(2 major + ~48 minor findings). All six invariants then defined pass:

| Item | Value |
|---|---|
| Generated | `2026-08-13T01:46:55Z` |
| Binary | `agent-session-grep.exe` 0.1.0, sha256 `481d8c98a52f9cecb68682a914fbe6fa29a79b41ea1615571af686e3e83ddfeb` |
| Corpus | 1,340 source files, 1,267,099,050 bytes |
| Outcome | `passed` |
| Sync result | exit 0, 182,886 emitted records, 0 skipped |
| Catalog | 167,480 de-duplicated `msg_v1_` entities, 242 sessions, 1,338 documents |
| Rebuild | catalog 169,060 -> 169,060, ids match |

```text
INV-SYNC-OK           PASS  exit 0, ok=True, 1340 sources, 182886 emitted, 0 skipped
INV-NO-PARSE-LOSS     PASS  provider emitted 182886, persisted 182886 claims, skipped 0
INV-SESSION-PRESENT   PASS  242 sessions for 1340 source files
INV-CONTEXT-NONEMPTY  PASS  242 sessions, 0 failed, 11 zero-placement, 0 internal
INV-SPAN-COVERAGE     PASS  630/630 spans have byte precision
INV-REBUILD-STABLE    PASS  rebuild exit 0, catalog 169060 -> 169060, sampled terms match
```

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
that previously stalled the old binary. **All six invariants then defined pass**:

```text
INV-SYNC-OK           PASS  exit 0, ok=True, 137 sources, 15246 emitted, 0 skipped
INV-NO-PARSE-LOSS     PASS  provider emitted 15246, persisted 15246 claims, skipped 0
INV-SESSION-PRESENT   PASS  17 sessions for 137 source files
INV-CONTEXT-NONEMPTY  PASS  17 sessions, 0 failed, 0 unexpected empty, 0 internal
INV-SPAN-COVERAGE     PASS  41/41 spans have byte precision
INV-REBUILD-STABLE    PASS  rebuild exit 0, catalog 14140 -> 14140, sampled terms match
```

This proved the fixed binary produces a green result on every invariant then
defined, on real data, and that the previously observed stall on this directory
is resolved. A subsequent fix added `Role::Developer` read-path support, which
the green full-corpus run above includes.

## Reader note on verifiability

Every count on this page is an aggregate quoted from a report generated on the
maintainer's development machine over the maintainer's own private transcripts.
Those transcripts are never committed or uploaded, and the generated reports
live under the gitignored `evidence-output/` directory, so **a reader cannot
independently reproduce or verify these numbers.** They are recorded here for
provenance and for tracking regressions between runs on that one corpus, not as
externally checkable evidence. The reproducible, committed benchmark is the
synthetic gate fixture under `scripts/evidence/fixtures/`.

## What these runs establish and do not establish

Establishes:

- A green full-corpus Gate D: 1,242 sources, 164,136 emitted, 0 skipped,
  all six invariants then defined pass, harness exit 0 (2026-08-09T21:10:16Z).
- Two later full-corpus greens over the same roots: 2026-08-12T23:51:23Z
  (1,328 sources, 180,218 emitted) and 2026-08-13T00:26:57Z (1,330 sources,
  180,718 emitted), all six invariants then defined pass, harness exit 0; the
  2026-08-13 run is the renamed-binary Gate D verification.
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
