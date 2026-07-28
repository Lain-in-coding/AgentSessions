# Real-data regression: recorded authorized local run

> Evidence record
>
> - evidence id: `IB-REAL-DATA-REGRESSION-001`
> - status: **executed repeatedly, outcome still `failed`** — the runs surfaced a
>   layered product defect (see [Finding](#finding-a-messages-position-is-a-per-source-fact));
>   results are recorded as-is rather than being tuned until they passed.
> - last run: 2026-07-27, after the child-8 partial fix (three of four layers
>   addressed); `INV-SYNC-OK` still fails.
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
| Corpus | ~700 `.jsonl` source files, 605 MB total (Claude Code + Codex) |
| Temporary store | created per run, deleted afterwards |
| Outcome | `failed` — `INV-SYNC-OK` failed; the five downstream invariants were reported as not evaluated |

## Result

Each run was executed against the same corpus. The committed-message count is
how far ingestion got before the store refused a batch, so it measures progress
across the child-8 fixes:

```
run 1 (pre-fix)                INV-SYNC-OK  FAIL  exit 6,   0 messages
run 2 (session union)          INV-SYNC-OK  FAIL  exit 6, 6767 messages
run 3 (+ message session refs) INV-SYNC-OK  FAIL  exit 6, 6767 messages
run 4 (+ per-document spans)   INV-SYNC-OK  FAIL  exit 6, 7060 messages
INV-NO-PARSE-LOSS    FAIL   not evaluated: sync failed
INV-SESSION-PRESENT  FAIL   not evaluated: sync failed
INV-CONTEXT-NONEMPTY FAIL   not evaluated: sync failed
INV-SPAN-COVERAGE    FAIL   not evaluated: sync failed
INV-REBUILD-STABLE   FAIL   not evaluated: sync failed
```

Every run failed loudly and atomically with a catalogued code rather than
committing something inconsistent:

```
code: catalog_error   (exit 6)
message: backend failure: message <redacted id> has conflicting
         projections across sources
```

Ingesting each source individually succeeds. The failure needs at least two
sources that share an entity in one batch, which is why synthetic fixtures and
the CI smoke never hit it.

## Finding: a message's position is a per-source fact

Four distinct conflicts were diagnosed against the local corpus, each by
comparing the canonical fields of the same entity across the files that declare
it. They are the same defect seen from four angles.

| Layer | Conflicting entity | What differs per source | Verified by |
|---|---|---|---|
| 1 | `ses_v1_` session | member list — each file carries only its own slice | one session id declared as the owning `sessionId` by **55 distinct files**, each file holding exactly one distinct id (so not intra-file mixing) |
| 2 | `msg_v1_` message | `session` back-reference | one message uuid declared under **3 different `sessionId`s**, with identical text, parent, and timestamp |
| 3 | `msg_v1_` message | `span` byte offsets | same message uuid, same text, raw record length 929 vs 810 bytes in two files |
| 4 | `msg_v1_` message | **`parentUuid`** | same message uuid, same text and timestamp, parent `5c6d…` in one file and `5423…` in two others |

Layers 1-3 were fixed in child 8 by unioning the per-source facts in the commit
path and keeping the singular keys as aliases. Layer 4 is not amenable to the
same treatment, and that is the real conclusion of this exercise:

The unifying rule is that a message's **identity** (its provider-native uuid) is
shared across sessions, while its **position** — which session owns it, which
message is its parent, where its bytes sit in a file — is a property of the
source. The current implementation stores position inside the message entity, so
every position field becomes a cross-source conflict. Unioning them one at a
time treats symptoms.

Layer 4 additionally breaks a reader invariant that unioning cannot repair:
`select_mainline` walks a single parent chain to pick the highest-sequence leaf.
A message with two parents has no defined mainline, so a union would make branch
selection return an arbitrary result. Failing loudly is correct until the model
is fixed.

`RFC-0001` §3.2 already separates these concerns: `Message` carries
`session_id`/`thread_id`/`branch_id`, and parent-child relations live in a
distinct `MessageEdge` relation rather than inside the message. The fix is to
bring the implementation back to the RFC — move position out of the message
entity — not to keep adding arrays to the payload. That is a model-level change
and is deliberately not attempted in child 8.

The source-membership guard behaved as designed throughout. It is what made all
four layers visible instead of letting one source silently overwrite another's
projection.

## What this run establishes and does not establish

Establishes:

- The regression procedure is repeatable and auditable, and it runs on a real
  corpus without copying, modifying, or transmitting any transcript. The
  provider read-only boundary held: 605 MB of sources were read and none were
  written.
- Per-source ingestion of real Claude Code and Codex data works; the failures
  are specific to entities shared by several sources in one batch.
- The store fails loudly and atomically on an identity conflict. Nothing
  partial was committed, and the error carries a catalogued code and exit
  status.
- Layers 1 to 3 are fixed and covered by tests that fail without the fix:
  cross-source session membership, cross-session message ownership, and
  per-document spans now merge as unions. Each re-run advanced further into the
  corpus (0 → 6767 → 7060 messages committed before the next distinct
  conflict), which is what confirms the fixes take effect on real data rather
  than only on fixtures.

Does not establish:

- Any provider promotion. Both Claude Code and Codex stay **Experimental**.
  Gap 4 in `../../product/PROVIDER-MATURITY-MATRIX.md` has a repeatable
  process, but a corpus-wide green run is still blocked — now on layer 4.
- Any claim about aggregate corpus statistics (message counts, role
  distribution, span coverage). Those fields were not evaluated because the
  ingest never completed.

## Next step

Layer 4 needs a model change, not another merge rule. Per RFC-0001 §3.2 the
canonical model already separates a message's identity from its placement: a
`Message` carries `session_id` / `thread_id` / `branch_id`, and parent-child
relations live in a separate `MessageEdge` relation. The current implementation
instead stores placement inside the message payload, which is why every added
placement field produced another cross-source conflict. Moving placement out of
the message entity — so one message can be placed differently in each session
that carries it — is the fix that closes this class of defect rather than its
next instance.

That work is deliberately not attempted here. Merging divergent parents would
make branch selection silently arbitrary, which is worse than a loud refusal: a
passing report must come from a corrected model, not from a relaxed invariant or
a guess about which parent is real.
