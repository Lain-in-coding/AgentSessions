# Real-data regression: recorded authorized local run

> Evidence record
>
> - evidence id: `IB-REAL-DATA-REGRESSION-001`
> - status: **executed, outcome `failed`** — the run surfaced a product defect
>   (see [Finding](#finding-sessions-span-multiple-source-files)); it is recorded
>   as-is rather than being tuned until it passed.
> - harness: `scripts/evidence/real_data_regression.py`
> - authorization: the operator's own machine, own transcripts, data never
>   leaves the host. This file carries aggregate counts only.

## Method

The harness built a throwaway temporary data root, ingested the corpus through
the real release binary in `--robot` mode, and evaluated six invariants. No
transcript content, path, provider-native id, or fingerprint is recorded here
or in the generated report. Reports land in the gitignored `evidence-output/`
directory; only this aggregate summary is committed.

| Item | Value |
|---|---|
| Binary | `agentsessions-cli 0.1.0`, release profile, built with `--locked` |
| Host class | Windows 11 x64, Python 3.10 |
| Corpus | 707 `.jsonl` source files, 605 MB total (Claude Code + Codex) |
| Temporary store | created per run, deleted afterwards |
| Outcome | `failed` — `INV-SYNC-OK` failed; the five downstream invariants were reported as not evaluated |

## Result

```
INV-SYNC-OK          FAIL   exit 6, ok=false, 707 sources, 0 messages
INV-NO-PARSE-LOSS    FAIL   not evaluated: sync failed
INV-SESSION-PRESENT  FAIL   not evaluated: sync failed
INV-CONTEXT-NONEMPTY FAIL   not evaluated: sync failed
INV-SPAN-COVERAGE    FAIL   not evaluated: sync failed
INV-REBUILD-STABLE   FAIL   not evaluated: sync failed
```

The store refused the batch and reported a stable error code rather than
committing something inconsistent:

```
code: catalog_error   (exit 6)
message: backend failure: message ses_v1_<redacted> has conflicting
         projections across sources
```

Ingesting each of those sources individually succeeds. The failure needs at
least two sources of the same session in one batch, which is why synthetic
fixtures and the CI smoke never hit it.

## Finding: sessions span multiple source files

Verified against the local corpus by counting, per session id, how many files
declare it in their `sessionId` field:

- One session id was declared as the owning `sessionId` by **55 distinct
  transcript files**. Only one of those files is named after that id; the other
  54 have unrelated file names.
- Each individual file contains exactly one distinct `sessionId` — so this is
  not intra-file mixing. It is one logical session persisted across many files
  (continuation, resume, and sidechain transcripts).

The current ingest model assumes one document maps to one session:
`staged_to_entries` derives the session entity per source and lists that
source's messages as its members. When two sources of the same session are
committed together, the same `ses_v1_` entity arrives with different member
lists, and the source-membership guard correctly rejects the batch instead of
letting one source silently overwrite the other's projection.

The guard behaved as designed. The modeling gap is upstream of it: a session
entity needs to be the union of its sources, not a per-source projection.

## What this run establishes and does not establish

Establishes:

- The regression procedure is repeatable and auditable, and it runs on a real
  corpus without copying, modifying, or transmitting any transcript. The
  provider read-only boundary held: 605 MB of sources were read and none were
  written.
- Per-source ingestion of real Claude Code and Codex data works; the failure is
  specific to multi-source sessions in one batch.
- The store fails loudly and atomically on an identity conflict. Nothing
  partial was committed, and the error carries a catalogued code and exit
  status.

Does not establish:

- Any provider promotion. Both Claude Code and Codex stay **Experimental**.
  This run makes the remaining gap sharper rather than closing it: gap 4 in
  `../../product/PROVIDER-MATURITY-MATRIX.md` now has a repeatable process, but
  a corpus-wide green run is blocked on the modeling fix above.
- Any claim about aggregate corpus statistics (message counts, role
  distribution, span coverage). Those fields were not evaluated because the
  ingest never completed.

## Next step

Fix the session-identity model so a session entity aggregates members across
all its sources, then re-run this harness unchanged and record the result. That
work is out of scope for child 7 and is not attempted here; a passing report
must come from a corrected model, not from a relaxed invariant.
