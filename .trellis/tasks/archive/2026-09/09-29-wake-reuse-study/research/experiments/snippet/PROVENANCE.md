# Experiment A provenance

## Source inspected (read-only; no Wake execution)

- Project: [iAmCorey/Wake](https://github.com/iAmCorey/Wake), tag `v0.8.5`.
- Commit: `71aeca67ec80f8645d1f9d5199290c2c732036ce`.
- Source: [`crates/wake-core/src/db.rs`, lines 3302-3345](https://github.com/iAmCorey/Wake/blob/71aeca67ec80f8645d1f9d5199290c2c732036ce/crates/wake-core/src/db.rs#L3302-L3345),
  `make_like_snippet` (function starts at line 3307).
- Git source blob: `d0152507eba4fcc470f57538ae2661f0dcc9a799`.
- License: MIT, **Copyright (c) 2026 Corey Chiu**.
- Git LICENSE blob: `77ad15a2bbf1194c9728572cbbe7393b8295954e`.
  The complete notice is retained in `LICENSE-WAKE`.

Classification is **adapt**, not direct-copy or a claim of clean-room
independence from the inspected source. The scalar-origin map and the 40-before /
80-after window idea are derived from this function; the implementation,
selection/budget policy, term handling, fixtures and report are written for this
experiment. No upstream modules or fixtures are imported. No Wake code is run,
no source checkout is indexed or mutated, and no default provider roots are read.
Product reuse approval remains a separate review.

## Product comparison

Pinned commit: `2b8f89562e43bd1f68ad3c8e5cf8ccc9281f9af2`.

| File | Lines / symbol | Git blob |
| --- | --- | --- |
| `crates/agent-session-grep-application/src/lib.rs` | 970-1009, `assemble_search_hit`; 992-994 prefix expression | `5cce489f96bb4b1692a263569f07128143f0a832` |
| `crates/agent-session-grep-application/src/guidance.rs` | 22-34 `literal_terms`; 50-82 `why_matched` / `why_matched_in` | `9fe74633b38a436cf06c818d6fc9678f0d87f9fc` |
| `crates/agent-session-grep-adapters-sqlite/src/lib.rs` | 8490-8509, `safe_fts_query` | `300ef1c834512ed6cbbf2ecea6460f5c1724d80e` |

The application/guidance and FTS sanitizer were located/read with CodeGraph
first, then pinned Git coordinates were verified. The sole reproduced baseline
behavior is `text.chars().take(max_snippet_chars).collect()`. The real product is
not executed by this crate. Neither the product's complete budget machinery nor
its CJK/guidance pipeline is simulated or modified.

## Modifications and differences

- Wake searches its first term and adds highlight markers plus ellipses. This
  prototype records all literal occurrences, anchors the earliest source hit
  (query-order tie break), and emits only an original contiguous text slice.
- Both sides use per-scalar lowercase here. Wake lowercases the term as a whole
  string but the haystack per scalar; contextual lowercase is deliberately not
  claimed. Expanded offsets always map back to whole original scalars.
- A single final `*` is a prefix operator with a conservative original-text
  boundary check. Earlier stars and `%`, `_`, `?`, etc. remain literal. The
  product FTS sanitizer removes all terminal stars; equivalence is NOT claimed.
- Bounded displays serialize the exact local `{"text":...}` payload before
  accepting it. This is not full SearchHit/Robot envelope integration.
- Complete anchors take priority over context. A 2-right/1-left expansion schedule
  is bounded by character and serialized JSON bytes; an unfit anchor produces
  empty text rather than invented evidence. An unfit empty payload is omitted.
- No ranking, cursor, APIs, schemas, providers or root Cargo state are changed.

## Fixtures and dependencies

`src/cases.rs` contains only hand-authored, independently synthetic inputs and
expectations, revision `snippet-synthetic-v1`. Repetition of fixed characters
creates long tails; paths are fictional generic examples. No real transcript,
external fixture, source-derived test data, credential or personal path is used.
The report preserves the full inputs, independent expected values, actual
outputs, source scalar ranges and per-case checks. Its SHA-256 covers the ordered
compact JSON serialization of the case array (`id`, `input`, `expected`), with recursively sorted object keys and UTF-8 Unicode rather than ASCII escapes.

`Cargo.lock` is local to this explicit standalone workspace. Dependencies are
`serde` (derive), `serde_json`, and `sha2`; there are no Wake/product/path/Git
dependencies. Offline replay requires these locked crates already cached. The
upstream MIT notice does not replace dependency-license review or product reuse
approval. Tests and report replay make correctness claims only, not performance
or provider-certification claims.
