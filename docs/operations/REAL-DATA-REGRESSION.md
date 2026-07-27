# Real-data regression

Scope: running `scripts/evidence/real_data_regression.py` against your own
Claude Code and Codex transcripts to check that ingestion, context assembly,
and index rebuild hold their invariants on real input instead of only on
synthetic fixtures.

## Why this runs locally and stays local

Real transcripts are personal data. The authorization for this check is
narrow: **the data stays on the machine that produced it.** The harness is
built around that constraint rather than trusting the operator to redact
afterwards.

- Sources are opened read-only. The provider path never writes to a
  transcript.
- Ingestion targets a throwaway temporary data root created per run and
  deleted at the end. Your real data root is untouched.
- The report contains aggregate counts and invariant verdicts only. No
  message text, no source paths, no provider-native ids, no fingerprints, no
  usernames, no hostnames.
- Reports are written under `evidence-output/`, which is gitignored. The
  harness does not commit, push, or upload anything.

What lives in the repository is the harness, its unit tests, and an example
report produced from synthetic fixtures. Reports produced from real data do
not go into the repository.

## What this does not establish

Passing this check does not promote a provider to Beta. It closes the
"repeatable process" gap in `docs/product/PROVIDER-MATURITY-MATRIX.md` —
the procedure and the checker are auditable even though the corpus is not
shareable. The remaining gate (three-platform CI certification recorded
against a specific run) is separate. Both providers stay Experimental until
every documented gate is green.

## Prerequisites

- A release binary. Build it with
  `cargo build --locked --release -p agentsessions-cli`, or use an installed
  one (see `INSTALL-AND-UPGRADE.md`).
- Python 3. No third-party packages.
- One or more directories or files of `.jsonl` transcripts you are authorized
  to read.

## Run it

```
python scripts/evidence/real_data_regression.py \
  --binary target/release/agentsessions \
  --sources C:/data/example-transcripts \
  --out evidence-output/real-data-regression.json
```

| Flag | Meaning |
|---|---|
| `--binary <path>` | The `agentsessions` binary to exercise. Required. |
| `--sources <dir\|file>` | Transcript directory (searched recursively for `.jsonl`) or single file. Repeatable. |
| `--out <path>` | Report destination. Defaults under `evidence-output/`. |
| `--json` | Emit the JSON report to stdout in addition to the file. |
| `--dry-run` | Print the plan and write no file. |

Exit codes: `0` all invariants passed; `1` at least one invariant failed
(the report is still written, with `outcome: failed`); `2` usage error, such
as source arguments that matched no `.jsonl` files.

## What it exercises

Against a temporary data root, in order: one `sync` over all collected
sources, then `status` and `doctor` for catalog counts plus generation,
schema, and interrupted-batch state, then `context --policy mainline` for
every session entity, then `index rebuild` followed by `status` and sampled
`search` calls. All CLI calls go through `--robot` and are read from the
response envelope, not from human-readable text.

## The six invariants

| id | What it asserts | What a failure means |
|---|---|---|
| `INV-SYNC-OK` | The `sync` envelope reports `ok: true` and more than zero messages. | Ingestion failed outright, or every source was rejected. Read the envelope's error code. |
| `INV-NO-PARSE-LOSS` | The message count the provider reported equals the number of `msg_v1_` entities in the catalog. | Messages were parsed but not persisted, or were merged. A real data-loss signal. |
| `INV-SESSION-PRESENT` | At least one `ses_v1_` entity exists, and no more than one per source file. | Session attribution is missing or duplicated. |
| `INV-CONTEXT-NONEMPTY` | Every session assembles at least one message, with no `internal` error. | Context assembly cannot reach messages it owns — usually a threading or parent-link problem. An `internal` error is a bug signal, not bad input. |
| `INV-SPAN-COVERAGE` | Every evidence span from this ingest has `precision == "byte"`. | Byte offsets were not recorded. Freshly ingested sources should always have them; `unknown` precision belongs to pre-v6 rows, which a fresh temporary store cannot contain. |
| `INV-REBUILD-STABLE` | Catalog counts before and after `index rebuild` match, and sampled searches return the same number of hits. | Rebuild is not a faithful reprojection of the catalog. See `rebuild-and-migration-runbook.md`. |

## Reading the report

The JSON report is the authority; the Markdown form is a human projection of
the same fields.

| Field | Meaning |
|---|---|
| `schema_version`, `kind` | Report format version and `real-data-regression`. |
| `generated_at_utc` | Run timestamp, UTC. |
| `binary` | Basename, version, and SHA-256 of the exercised binary. The basename only — no directory. |
| `environment` | OS family, OS release, Python version. |
| `corpus` | Number of source files and total bytes. Counts and sizes only, never names. |
| `totals` | Messages, sessions, documents, and total catalog entities. |
| `role_distribution` | Message count per role. |
| `evidence_precision` | Span count per precision tier (`byte`, `line`, `record`, `unknown`). |
| `invariants` | One entry per invariant: `id`, `passed`, and an aggregate-only `detail`. |
| `outcome` | `passed` or `failed`. |

## When it fails

Read `invariants` first: the failing entry names the check, and its `detail`
carries the aggregate numbers that disagreed. That is intentionally all it
carries, so triage happens against your local store rather than against the
report.

To investigate, re-run the same steps by hand on a temporary data root and
inspect the specific session. `context --robot` surfaces `warnings` and the
error envelope carries a stable `code`; the exit code follows the same
mapping as every other command (`2` invalid request or cursor, `4` not found,
`5` source I/O or source changed, `6` catalog error or writer busy, `7`
provider error, `9` schema incompatible, `70` internal, `10` partial result
after budget truncation).

Before sharing anything, check it. A hand-run `context` or `search` prints
real message text — that output is not covered by the harness's privacy
guarantees. Only the generated report is.
