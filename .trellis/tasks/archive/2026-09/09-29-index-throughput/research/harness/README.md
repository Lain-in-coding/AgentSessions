# Experiment B: search strategy and statement-cache microbenchmarks

**Isolated synthetic microbenchmarks plus an unmodified-product baseline
harness — not a Wake end-to-end benchmark and not a product performance
certification.** Two measurement levels are deliberately separated:

1. `wake-reuse-search` (Rust, this directory): one SQLite database built from
   a deterministic beacon corpus, comparing a faithful adaptation of the
   current CJK unigram/bigram + literal-prefix FTS route against a
   Wake-inspired trigram route with a short-term `LIKE` fallback, and
   `prepare` versus `prepare_cached` under identical SQL, ordering and data.
2. `baseline.py` (Python): end-to-end wall-clock samples of the **unmodified**
   release product binary (`sync`, `search`, `--version`) on the same
   synthetic corpus generator. Python `sqlite3` is used only as a read-only
   count observer, never as runtime evidence.

## Commands (repository root, PowerShell)

The measured product commit is always an explicit caller value
(`--expected-commit`, full lowercase 40-hex): the harness itself pins no
commit. The workspace HEAD must equal it and the product paths must be
unmodified relative to it, or the run is rejected.

```powershell
$s = '.trellis/tasks/09-29-index-throughput/research/harness'
$env:CARGO_TARGET_DIR = (Join-Path $PWD 'target/index-throughput')
$ws = (Resolve-Path '.').Path
$envfile = (Resolve-Path '.trellis/tasks/09-29-index-throughput/research/harness/environment.json').Path
$out = (Resolve-Path '.trellis/tasks/09-29-index-throughput/research/results').Path

# Rust micro harness: build both feature variants, then smoke/full.
cargo build --manifest-path "$s/Cargo.toml" --release --locked --offline
cargo build --manifest-path "$s/Cargo.toml" --release --locked --offline --features cache
python -B "$s/run_search.py" run --workspace $ws --binary "$env:CARGO_TARGET_DIR/release/wake-reuse-search.exe" `
  --scratch-dir "$ws/target/wake-reuse-scratch/full-nocache" --output-dir $out `
  --environment $envfile --feature no-cache --profile full --scale-timeout-seconds 1800
python -B "$s/run_search.py" validate "$out/search-micro-full-no-cache.json" --workspace $ws

# Product baseline (explicit absolute binary, fresh scratch/output paths).
python -B "$s/baseline.py" run --workspace $ws --binary "$ws/target/product-baseline/release/agent-session-grep.exe" `
  --scratch-dir "$ws/target/index-throughput-scratch/baseline-full" --output-dir $out `
  --environment $envfile --profile full --expected-commit (git rev-parse HEAD) --scale-timeout-seconds 1800
python -B "$s/baseline.py" validate "$out/search-baseline-full.json"

# Python-side tests (3.10-safe; includes the Cargo.lock fallback reader).
python -B -m unittest discover -s $s -p 'test_*.py' -v
```

Every report refuses to overwrite an existing file; use a new output directory
or new scratch directory. Scale timeouts are finite and clipped per command.

## Corpus and queries

`fixtures/beacons.json` is a frozen synthetic fixture (`revision
wake-search-beacons-v1`). Beacon texts are injected only into the first
`len(beacons) * repeat` messages; the rest are deterministic filler. qrels are
derived independently from `relevant_for`, and the negative control must
return zero hits. Data hashes use
`sha256(concat(u64le(id), u64le(utf8_bytes), utf8_text))` over `id = 1..N`.

## Method notes

- Strategy comparison holds method (`prepare`) and lifetime (persistent
  connection) constant; the cache comparison holds SQL, dataset and result
  ordering constant and validates identical retrieved ID lists.
- Cache evidence: `prepare_cached` only helps a long-lived connection on
  cheap (preparation-dominated) statements; a reopened connection has a cold
  per-connection cache and gains nothing.
- Short-term `LIKE` fallback for <3-scalar terms is a full scan: at 1M
  messages it is two-to-three orders of magnitude slower than the CJK
  bigram route. Trigram `MATCH` for >=3-scalar terms is fast.
- The product baseline records warm process-level samples only. There is no
  OS cache flush and no cold-disk claim; initial sync may run in bounded
  batches, so its duration is batch-total.
- RSS comes from 100 ms sampling with a final peak read; short subprocesses
  may have no RSS samples (recorded as null, never invented).
- Build artifacts must stay under `target/`; generated corpora and databases
  stay under the explicit scratch path and are not committed.

## Python 3.10

`evidence.py` prefers the standard-library `tomllib` and otherwise falls back
to a minimal `[[package]]` reader used only to select the locked
`rusqlite`/`libsqlite3-sys` versions for provenance. It never parses
executable content and rejects missing selections.
