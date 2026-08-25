# Semantic Hybrid Local Retrieval — Design

> Status: contract-and-plumbing slice implemented (2026-08-15); full task
> (model download, inference, vector store, RRF fusion, benchmark) follows.
> Parent PRD: `prd.md` in this task dir. Robot 1.1 `retrieval_mode` contract
> coordination: `08-15-unified-release-contract` (retrieval_mode fields agreed
> there first).

## Context

The workspace ships no embedding/vector/semantic code. This slice lays the
seam everything else hangs on: ports (trait-only), Application ADT plumbing
with fail-visible degradation, and the CLI `--retrieval-mode` flag + envelope
annotation. It adds **no** model download, no network code, and no new heavy
dependencies.

## Decisions (implemented in this slice)

### D1 — Retrieval contract lives in ports, trait-only

`agent-session-grep-ports` gains:

- `RetrievalMode { Lexical, Semantic, Hybrid }` — `Default = Lexical` via
  `#[derive(Default)] + #[default]`; stable wire ids via `as_str()` /
  `from_wire()` (`lexical` / `semantic` / `hybrid`).
- `RetrievalDegradationReason { SemanticUnavailable }` — the only variant
  reachable in this slice; the model-download subtask extends it (download
  failure / corrupted model / CPU unsupported / vector index not ready).
- `RetrievalDegradation { mode_requested, mode_served, reason }` — the
  fail-visible annotation (PRD R4). `mode_served == mode_requested` must
  never carry an annotation.
- `EmbeddingProvider` (trait): `model_id()`, `dimension()`,
  `embed_batch(&[&str]) -> PortResult<Vec<Vec<f32>>>`. Implementations must be
  fully local (hash-verified first download, then local inference) or an
  explicitly configured external API — external APIs are never the default
  path (roadmap Q21/Q34).
- `VectorIndex` (trait): `upsert(&[EmbeddingRecord])`,
  `delete_by_source(&str)`, `query(&[f32], limit) -> Vec<SearchHit>`.
  Doctrine matches FTS: catalog authoritative, vector table a derived
  projection fully rebuildable from catalog + embeddings, writes under the
  same WriterLease / durable outbox rules.

No production implementation exists yet; `SnapshotFs`-style test stubs are
the only impls today. Embedding failures surface as `PortError::Backend`
(`catalog_error`, exit 6) in this slice — a dedicated canonical code is
deferred to the model-download subtask (see D5).

### D2 — Application resolves degradation, never silently

`App` gains a `retrieval: RetrievalCapabilities` field
(`{ semantic_available: bool }`, default `false`); `App::new` and
`App::with_clock` are unchanged, so every existing composition path keeps the
byte-identical Lexical behavior. `App::with_retrieval_capabilities` is the
seam the semantic-serving subtask flips when it wires a real provider +
index. `resolve_retrieval(mode, caps)` is a pure, tested function:

- Lexical → served Lexical, no annotation (always).
- Semantic/Hybrid + `semantic_available` → served as requested, no annotation
  (future contract; nothing in this slice sets the flag).
- Semantic/Hybrid + unavailable → served Lexical + `RetrievalDegradation`
  with `reason = SemanticUnavailable` (fail-visible, never silent).

`AppRequest::Search` gains `retrieval_mode: RetrievalMode` (default Lexical);
`AppResponse::Search` gains `retrieval_degradation: Option<RetrievalDegradation>`.

### D3 — Cursor bound to retrieval mode (non-Lexical only)

Pagination cursors bind the query digest. For `retrieval_mode != Lexical` the
digest input becomes `retrieval:<mode>\n<query>`; the default path keeps
`digest_query(query)` exactly, so existing cursors and behavior are
unchanged. Cross-mode resume fails loudly with `cursor_invalid` instead of
silently mixing orderings across pages.

### D4 — CLI flag is additive, registered in every prefix scanner

`search --retrieval-mode lexical|semantic|hybrid` (default lexical; unknown
value → usage error exit 2). Registered in every value-bearing-flag skip
list: `parse_output_mode` (protocol.rs), `extract_request_id`,
`command_name`, `intercept_help_or_version`, `extract_db_flag_impl`,
`bare_positionals`, `is_known_flag_name`. Help text updated (top-level +
`search` subcommand). MCP `search_sessions` and TUI stay on the default
Lexical path (no new MCP param; tool set frozen).

### D5 — Envelope: annotation in `data.retrieval` + warnings; no schema bump, no new code

The robot envelope carries the annotation **only when degradation occurs**:
`data.retrieval = { mode_requested, mode_served, reason }` plus one
`warnings` entry with the fix direction ("configure/download the local
embedding model and retry"). The default path emits neither — byte-identical
to pre-slice output, so existing e2e envelope-shape assertions and the smoke
scripts are unaffected. `SCHEMA_VERSION` stays `1.0` (additive minor data
key; the Robot 1.1 bump is owned by `08-15-unified-release-contract`). No
`CanonicalCode` was added — a dedicated semantic-unavailable code is
deferred and coordinated with that task to avoid merge friction.

## Model-selection decision matrix (full task)

Corpus reality: CJK + English + code mixed transcripts. The default model is
locked by a benchmark on a sanitized synthetic fixture (below), not by
claims; until thresholds are met, maturity stays beta and Lexical remains the
default mode (PRD AC: "未达质量阈值只能标 beta，不得默认替换 lexical").

| Criterion | multilingual-e5-small (intfloat) | all-MiniLM-L6-v2 (SBERT) | bge-m3 (BAAI) |
|---|---|---|---|
| Size / params | 118M | 22.7M | 568M |
| Dim | 384 | 384 | 1024 |
| Languages | 100+ incl. zh (explicitly trained multilingual) | English-centric | zh/en/multilingual |
| CJK quality | strong (E5 training includes Chinese) | weak (tokenizer/unsupervised losses are EN-focused) | strong |
| Code text | acceptable; not specialized | acceptable | acceptable |
| ONNX availability | official ONNX export (intfloat) | official ONNX export (sentence-transformers) | community ONNX exports exist |
| First-download size (fp32 ONNX) | ~470 MB (118M x 4B) | ~90 MB | ~2.3 GB (fp32) / ~1.1 GB (fp16) |
| RAM for inference | moderate | small | large (may exceed typical dev laptop budgets) |
| Runtime options | ort / candle | ort / candle | ort / candle (memory-heavy) |

Evaluation candidates shortlist: `multilingual-e5-small` (recommended
primary) vs `bge-small-zh`/`bge-m3` (CJK-heavy fallback) vs
`all-MiniLM-L6-v2` (baseline to prove why EN-only is insufficient). Decision
rule: pick the smallest model that clears the frozen benchmark thresholds
(recall + latency + disk), because embedding latency multiplies over
message/placement corpora.

Runtime choice (to verify in the inference spike, not settled here):

- `onnxruntime` (`ort` crate): mature, CPU-friendly, official model exports,
  quantization (`int8`/`dynamic`) to cut first-download and RAM; single
  native dependency per platform.
- `candle` (HuggingFace): pure Rust, no native runtime, but ONNX operator
  coverage and Windows support need verification on the pinned model export.
- Compatibility watch: both must work with `libsqlite3-sys` bundled builds
  (feature flags only, no link conflicts) and with the vector-store choice
  below.

Model delivery: hash-verified first download (manifest records model id,
sha256 of each file, dimension, license, benchmark results — PRD R1), then
fully local inference; download supports resume/retry/cancel; `--offline`
degrades semantic/hybrid to lexical with the explicit annotation (D2 path).

## Vector-store options (full task)

| Option | sqlite-vec extension | own f32-blob table + brute-force cosine |
|---|---|---|
| Architecture | loadable SQLite extension; virtual tables | plain table `(entity_id, dim, vector_blob)` + in-process scan |
| Bundled-libsqlite3-sys friction | extension loading must be verified with the `bundled` feature (runtime `.dll`/`.so` must ship per platform; rusqlite `load_extension` gating) | none — pure SQL |
| Scale at this product's size | overkill for <=100k rows | fine: 100k x 384-dim f32 = ~150 MB in memory / ~40 ms per brute-force scan |
| Rebuildable-projection doctrine | rebuild = delete + re-embed + re-insert (same as FTS rebuild) | identical |
| Indexed ANN | yes (but ANN accuracy tuning needed) | none (brute force is the honest baseline) |

Recommendation: start with the **own-table brute-force cosine** projection
(same writer-lease/outbox transactional discipline as FTS), keep sqlite-vec
as a spike comparison under the frozen benchmark; only adopt it if measured
p95 latency at target corpus size demands ANN. Deterministic ordering for
cursors: score desc + id tiebreak, mirroring the pinned FTS ordering.

## Hybrid fusion plan (RRF, k=60)

- Run lexical (FTS5 bm25) and semantic (cosine) retrievals independently,
  each capped at a fusion window (e.g. 100 per source).
- Reciprocal Rank Fusion: `score(d) = sum(1/(k + rank_src(d)))` with `k = 60`
  (the canonical Cormack et al. constant; benchmark compares k in {20, 60, 100}
  before locking).
- Semantic must never displace a lexical exact hit: lexical exact matches
  keep a floor (e.g. rank-1 lexical hits rank above all fused ties).
- Hits carry source provenance (per-hit source tags) so Evidence spans remain
  traceable to the semantic hit path (PRD R3).
- Fusion result feeds the existing budget/clamp/cursor pipeline unchanged.

## Benchmark corpus plan (synthetic only)

No real transcripts (privacy boundary). A fixed synthetic manifest generated
deterministically (seeded):

- Corpus: ~2k messages across ~200 sessions, mixing CJK (zh bigram-heavy),
  English prose, and code snippets (a few KB of real Open Source-licensed
  snippets as constant fixtures, or fully synthetic templates — decide with
  the license audit before commit), plus planted near-duplicate and
  paraphrase pairs to test semantic recall.
- Queries: ~100 frozen queries in zh/en/code, each with a gold-relevance
  judgment (synthetic ground truth from the generator).
- Metrics (frozen SLI schema): recall@k (k=5/10/20) per mode
  (lexical/semantic/hybrid), p50/p95 query latency, model download size,
  first-load time, vector-index disk footprint, embedding throughput.
- Reproducibility: corpus/query version pins + exact commands + hardware/OS
  in the report header; thresholds and repeat counts frozen; report JSON
  reuses the benchmark manifest owned by task
  `08-15-benchmark-install-open-source-gate` (roadmap Phase 9) where it
  overlaps.
- Acceptance gate: hybrid >= lexical recall (never worse), p95 within budget;
  otherwise maturity stays beta and Lexical stays default.

## Integration seams for sibling tasks

- `08-15-unified-release-contract`: Robot 1.1 `retrieval_mode` schema/field
  names must match `RetrievalMode::as_str()` (`lexical`/`semantic`/`hybrid`)
  and the `data.retrieval` object key names; do not rename the wire strings.
- Semantic-serving subtask (this task's next slice): flip
  `RetrievalCapabilities::semantic_available` via
  `App::with_retrieval_capabilities` **only when** provider + index are wired
  and the serve path is real; extend `RetrievalDegradationReason` for
  download/corruption/CPU/index-not-ready; replace the `_mode_served` binding
  in `App::handle` Search with the actual semantic/hybrid execution.
- MCP/TUI: keep `RetrievalMode::default()` (Lexical) until the MCP param
  decision is made.

## Deferred items

- Model download/verify/resume/cancel and the model manifest.
- `EmbeddingProvider`/`VectorIndex` implementations; vector table schema
  (f32-blob) and its `rebuild` command parity with `index rebuild`.
- RRF fusion + per-hit source provenance.
- Benchmark harness + frozen SLI report.
- Dedicated canonical code for semantic-unavailable (coordinated with
  `08-15-unified-release-contract`).
