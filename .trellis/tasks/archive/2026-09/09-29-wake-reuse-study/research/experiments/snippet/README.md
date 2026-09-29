# Experiment A: bounded, hit-centered snippets

**Isolated synthetic correctness prototype — not product integration or a
performance benchmark.** This crate is its own `[workspace]`, with its own
`Cargo.lock`; it imports neither Wake nor the product. Source attribution and
exact pinned coordinates are in [PROVENANCE.md](PROVENANCE.md), with the complete
upstream MIT notice in [LICENSE-WAKE](LICENSE-WAKE).

## Run from the repository root (PowerShell)

```powershell
$manifest = '.trellis/tasks/09-29-wake-reuse-study/research/experiments/snippet/Cargo.toml'
$output = '.trellis/tasks/09-29-wake-reuse-study/research/results/snippet.json'
$env:CARGO_TARGET_DIR = Join-Path $PWD 'target/wake-reuse-snippet'

cargo fmt --manifest-path $manifest --check
cargo clippy --manifest-path $manifest --all-targets --all-features --locked --offline -- -D warnings
cargo test --manifest-path $manifest --all-features --locked --offline

# Creates only the task-local results directory if needed.
New-Item -ItemType Directory -Force -Path (Split-Path $output) | Out-Null
cargo run --manifest-path $manifest --locked --offline -- report $output
cargo run --manifest-path $manifest --locked --offline -- validate-report $output
```

Each command must exit successfully before continuing. The output file is
explicit and is overwritten only when `report <path>` is requested. `report`
without an output argument prints JSON to stdout. `--help` prints the two CLI
commands. No default database, provider source or home directory is discovered.
The report's `status` is `failed` and report generation exits nonzero if a golden
expectation/check fails; a failure report is still written for inspection.

The initial lock was resolved with
`cargo generate-lockfile --manifest-path $manifest --offline`. Normal replay
uses the checked-in lock and cached dependencies; there is no silent online
fallback. All build artifacts must stay under the separate, ignored
`target/wake-reuse-snippet`, not a root workspace build or this tracked directory.

## What is compared

1. **`raw_prefix`** reproduces exactly
   `text.chars().take(max_snippet_chars).collect()`, the pinned product's text
   extraction expression. It deliberately has **no local byte gate**. Its byte
   count is evidence about this expression only, not a claim that the complete
   product ignores budgets.
2. **`bounded_prefix`** applies the experiment's local serialized-byte gate to
   that prefix. This is a comparison control, not a product behavior claim.
3. **`candidate`** independently adapts Wake's Unicode scalar-origin map and
   hit-window idea. It retains a complete source hit, then adds two right
   scalars and one left scalar repeatedly, up to 40 before and 80 after. Each
   accepted slice satisfies both caps. One expensive adjacent scalar stops that
   side; source text is never skipped or rearranged.

All output text is an exact contiguous original slice. No highlight tags,
ellipses or other synthetic characters are inserted. `source_range` uses
half-open original Unicode scalar offsets. `source_matches` records all
verified overlapping matches; `visible_source_matches` contains only complete
source matches inside the emitted range. The anchor is the earliest original
start, then query term index, then end offset; this is not result ranking.

### Literal and prefix semantics

- Terms are explicit strings, not a parsed search query. Ordinary terms use
  per-scalar lowercase substring matching. Lowercase expansions such as
  `İ -> i + combining dot` are mapped back to whole original scalars, including
  when the match begins or ends inside an expansion.
- Exactly one final `*` is a prefix operator. Its remaining stem stays literal;
  e.g. `foo*bar*` requires the literal `foo*bar`, and `work**` requires `work*`.
  Interior/leading stars, `%`, `_`, `?`, regex syntax and quotes are not wildcards.
- A prefix must begin at an original scalar boundary and at source start, after
  whitespace, or after ASCII punctuation other than `_`. Thus `work*` does not
  match `network`, `_work`, `éwork`, `e + combining acute + work` or `中work`.
  Non-ASCII punctuation/emoji do not open tokens under this conservative rule.
  This is **not** SQLite/FTS tokenizer parity, and repeated stars deliberately
  differ from the product sanitizer's trim-all-terminal-stars behavior.
- Empty/whitespace-only terms, an empty prefix stem, nonmatching terms and
  explicit `semantic_only` mode do not invent literal evidence. They retain a
  bounded prefix. Semantic-only is a fixture mode, not a model invocation.
- This is not Unicode normalization or full case folding. Composed/decomposed
  accents, sharp-s and contextual Greek sigma negative cases make that visible.

### Exactly which byte budget is enforced

The bounded variants serialize the **complete local payload**
`{"text":<string>}` to compact UTF-8 JSON with `serde_json`. The cap includes the
key, braces, quotes, multibyte scalars, `\"`, `\\`, short control escapes and
`\u00xx` escapes. It is not a UTF-8 length estimate of the raw text. The empty
payload is 11 bytes. Below that floor, `output: null` in the diagnostic report
means *no snippet payload can be emitted*, not that a four-byte JSON `null`
payload satisfies the cap.

If the source anchor cannot fit as a whole under either cap, the candidate
emits `{"text":""}` with `empty_anchor_exceeds_budget`; source evidence remains
recorded, but no visible hit is claimed. This is a deliberate small-budget
prototype policy, not a proposed production fallback.

**The diagnostic JSON report is not capped by the snippet's local byte budget.**
Neither is this an implementation of the complete product SearchHit, guidance,
evidence or Robot response envelope. Adoption would require separate budget and
contract integration; no API, ranking, cursor or production edit is made here.

## Evidence and validation

`src/cases.rs` is the sole fixture generator: hand-authored expected texts and
source spans, fixed repetition for long tails, generic synthetic paths only.
The report contains complete inputs, independent expected values, actual
outputs (including exact wire JSON), and individual checks. It covers long-tail
and multiple/overlapping hits, one/two-character CJK, Windows/POSIX/code paths,
emoji, combining/expansion characters, escapes/controls, tiny budgets, positive
and negative prefix cases, no-match and semantic-only cases.

Integration tests additionally sweep 14 character caps x 49 byte caps x 4 term /
mode combinations (2,744 combinations), exercise every U+0000..U+001F control
character at exact/one-byte-short caps, assert original-scalar prefix boundaries,
and reject tampered report inputs, goldens, outputs, byte counts and summaries.
These are correctness checks, not randomized fuzzing or performance samples.

`validate-report` performs a fresh deterministic replay, recomputes the dataset
hash and all counts, and requires exact JSON-value equality (object-key order
and insignificant whitespace do not matter). It rejects unknown fields as well
as changed results. This is replay validation, not a signature or independent
algorithm oracle; the hand-authored goldens and focused tests are separate
checks. Dependency/toolchain behavior changes may legitimately require a new
fixture revision and regenerated evidence.

The SHA-256 input can also be reconstructed independently from a report: retain
only `id`, `input` and `expected` for each case in array order; recursively sort
object keys; serialize compact JSON without ASCII-escaping Unicode; encode
UTF-8 with no BOM/newline. The local locked Rust serializer produces that
canonical input. The report stores both hash algorithm and encoding rules.

## Interpretation and limitations

Descriptive visible-hit counts apply only to this intentionally selected suite.
They are **not** recall, representative quality, latency, throughput or SLO
measurements. The full scalar/lowercase arrays and all occurrences are
materialized, and candidate payloads are repeatedly serialized; no memory or
speed improvement is claimed. A single earliest-hit window may miss a more useful
later hit. Scalar safety is not grapheme safety: combining sequences or emoji
ZWJ sequences can be split at an edge.

Product CJK term extraction, payload string-leaf guidance, `why_matched`, FTS
retrieval, ranking/cursor identity, full response budgeting and actual semantic
models remain outside this experiment. Wake itself is only source-inspected;
its GUI, provider readers, fixtures and transcripts are not run or used. This
prototype does not constitute legal approval or an adoption recommendation.
