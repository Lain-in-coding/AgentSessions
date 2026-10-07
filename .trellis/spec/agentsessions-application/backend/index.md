# agentsessions-application — Backend Guidelines

> The application core. Orchestrates use cases through ports; owns no concrete
> backend. This is where provider-selection policy and request handling live.

---

## Role in the architecture

`agentsessions-application` holds the use-case logic: it receives an
`AppRequest`, drives the injected ports (catalog store, search index, provider
adapters), and returns an `AppResponse`. It knows *what* should happen; the
adapters know *how*. Depends on `agentsessions-domain` and
`agentsessions-ports` only — never on `adapters-sqlite` or a provider crate.

---

## Pre-Development Checklist

- [ ] Is this business/orchestration logic (belongs here) or a storage/parse
      detail (belongs in an adapter)? If it touches SQL or file bytes, it's in
      the wrong crate.
- [ ] Does the handler depend on a *port trait*, not a concrete type? The core
      must stay backend-agnostic.
- [ ] New request/response? Extend `AppRequest` / `AppResponse` and handle the
      arm exhaustively; the CLI's `response_data` must render the new arm.
- [ ] Error provenance: does the failure come from domain, port, or provider?
      Return the matching `AppError` variant so the boundary is preserved.
- [ ] Public error text must be bounded and privacy-safe. Do not interpolate
      source paths, provider-native IDs, transcript content, or unbounded
      backend diagnostics into conflict or internal-error messages.
- [ ] Provider selection: does the change respect
      `Confirmed > High > Low`, and *reject* ties across different variants
      rather than guessing?

---

## Key types

- `AppError` — `Domain(DomainError)`, `Port(PortError)`, `Provider(ProviderError)`,
  `Cursor(CursorError)`, `Budget(BudgetError)`. Preserves the origin layer so
  the protocol layer maps each to the right code (cursor errors have dedicated
  canonical codes: `cursor_invalid` / `cursor_expired` / `generation_mismatch`).
  Boundary-visible conflict and invariant messages use bounded generic actions;
  sensitive provider or storage details remain behind the port boundary.
- `AppRequest` / `AppResponse` — the use-case envelope. Handlers match
  exhaustively; a new variant must be rendered by the CLI (`render` in main.rs).
  `Search` carries normalized provider/time filters plus cursor and budget;
  `List` carries cursor and budget; `Context` carries session, branch policy,
  `raw|talks|sessions` level, and budget; `Message` carries a stable Message,
  optional Session, around radius, and budget.
- `cursor` module — self-contained stateless pagination token (CONTRACT §7):
  `issue`/`verify` over `CursorClaims` (contract_major, generation, TTL,
  query/sort digest, offset). Keyless BLAKE3 integrity digest, domain-prefixed
  (`as-cursor-v1`, design §0 deviation 1). Time is ALWAYS injected — `App`
  carries a `fn() -> i64` clock (`App::with_clock` for tests); no module reads
  the system clock itself.
- `budget` module — `ResponseBudget` + `validate()` floors + `clamp_items`
  (max_items gate, then greedy byte gate in order; sort-before-truncate is the
  caller's obligation). `Truncation { truncated, reason }` reports the exact
  budget knob to raise (`max_items` / `max_response_bytes` / `max_messages` /
  `max_evidence_spans`).
- `final_score(relevance, age_ms, is_sidechain, in_current_repo)` — ranking
  entry point. `relevance <= 0.0` or NaN returns exactly 0.0 before any
  recency/repo/sidechain preference is applied: preference signals may only
  reorder admitted, positively-evidenced hits and must never resurrect a
  zero-evidence candidate. Lexical hits always carry a positive FTS score;
  this defense exists so future candidate sources cannot regress that.

- `evidence` module — `EvidenceSpanDto` assembly from stored canonical
  placements/documents; `occurrence_id` is the Placement ID, exact spans retain
  their source document, and missing placement spans remain explicitly
  `precision: unknown`.
- Pagination model — offset inside cursor claims over a PINNED total order
  (search: RRF + explicit lexical signals + wire-id tiebreak = `SORT_SCORE_DESC`; list: wire id ASC =
  `SORT_WIRE_ID_ASC`). Ports have no offset parameter: `handle` over-fetches
  `offset + page + 1` (sentinel for has_more) and slices. Any cursor failure is
  an explicit error — never a silent restart from page one.
- Context assembly loads one typed `SessionContextGraph` through
  `ContextGraphStore`, invokes Domain placement-aware selectors, applies budgets
  to occurrences, and assembles evidence from each selected placement's exact
  document/span. It never reads `session`, `parent`, `span`, or sidechain
  compatibility aliases. Derived views retain the additive raw `messages` field:
  fixed structural metadata is reserved once, while each duplicated message is
  charged per retained occurrence. Budget fallback retries in detail order
  `sessions -> talks -> raw`; context item clamps expose `max_messages`, never
  the internal generic `max_items` reason.
- `ContextMessage` carries `{ id, placement_id, message_id, payload }`;
  `id == message_id` is the compatibility alias and `placement_id` is
  authoritative. `branch_leaf_placement_id` is authoritative while
  `branch_leaf` remains a Message-ID alias.
- Search filters use backend-independent normalized UTC instants. Provider
  values are sorted and deduplicated before cursor digesting; provider values
  are ORed, provider/time dimensions are ANDed, and the time interval is
  half-open `[since, until)`. Search v2 digests bind requested/effective mode,
  model, query-vector dimension/content, ranking version/window, result set,
  filters/facets/visibility/grouping and current-repository signal. Old search
  cursors fail explicitly; list cursor behavior is unchanged.
- Search continuation retains verified `CursorClaims`: recency scoring uses
  the first request's `issued_at_ms`; only `offset` changes between pages.
  Preserve `expires_at_ms`, so search pagination never extends the original
  15-minute TTL. The digest version includes the clock-anchor revision.
- System-noise messages (payload `role` system/developer) are excluded from
  search by default; `include_system: true` opts back in. `group_by_session`
  collapses hits per session over a bounded scan window: the best-scoring hit
  is kept first and `occurrences` counts all grouped hits; the default path
  keeps `occurrences == 1`. Both are additive and participate in the cursor
  digest.
- Search guidance is deterministic Application output: `why_matched` uses
  literal plain-text/CJK terms against full hit text, and bounded suggested
  calls use only real Message/Session IDs. JSON-escaped guidance bytes are
  charged before result clamping and never affect ranking or cursor identity.
- Search display snippet (`SearchHit.text`) is a match-centered window over
  the canonical payload `text` field, built by `snippet::build`: the earliest
  literal `guidance::literal_terms` hit (smallest start, ties by term order)
  anchors a 2-right : 1-left expansion up to `max_snippet_chars`; an anchor
  that alone reaches the cap emits its leading `max_snippet_chars` slice;
  without literal evidence (missing text, no match, semantic-only) the old
  character prefix is kept. Case-insensitive matching maps per-character
  lowercase expansions back to original scalar boundaries (`İ`); output is
  always an exact contiguous slice with no synthetic characters. Window bytes
  are charged by the existing `search_hit_charge` path, and the window never
  changes ranking, cursors, `why_matched`, or suggested commands.
- `Message` retrieval resolves typed Session candidates without guessing,
  selects a unique mainline placement, and returns a chronological around
  window that always retains the anchor. If only a projected anchor fits, keep
  its Message/Session/placement identity and truncate payload text under the
  final Robot byte budget.
- `MessageContexts` resolves reverse membership through the typed port and
  returns candidates grouped by distinct Session. Multiple placements in one
  Session are one candidate; Application never chooses between several Sessions.
- `select_and_stage(adapters, bytes)` — probe/select policy: pick the highest
  non-ambiguous confidence adapter; a top-confidence tie across *different*
  variants is an error, not a coin flip.
- `StagedMessage` — the in-memory staged row before commit
  (session / seq / native_id / parent_native_id / role / text / timestamp / is_sidechain / span).
- `StagedBatch` — all emitted messages plus the complete provider `ParseReport`;
  callers must retain committed/skipped/diagnostic accounting rather than
  replacing it with guessed zeros.

---

## Scenario: Filtered retrieval, ranking and strict request time

### 1. Scope / Trigger
Search across lexical, semantic, hybrid and explicit fallback modes.

### 2. Signatures
`AppRequest::Search` includes filters, facets, mode, query embedding, visibility,
cursor and budgets. `parse_search_instant(&str) -> Option<SearchInstant>` is the
strict request boundary; the Domain sorting parser remains intentionally broader.

### 3. Contracts
Read readiness once and propagate errors. Semantic candidates use the same
filters/facets/visibility as lexical candidates before the sentinel/limit.
Search v2 ranking uses RRF k=60; lexical signal constants use that scale:
sidechain penalty `0.25/61`, current-repo boost `0.5/61`, 30-day recency
half-life and 0.3 floor. Semantic/hybrid final scores do not get lexical signals.
Request clock components cannot be signed; all date/offset arithmetic is checked.

### 4. Validation & Error Matrix
Changed mode/model/vector/filter/ranking context -> `cursor_invalid`.
Changed generation -> `generation_mismatch`. Non-finite semantic data and
backend readiness failures -> explicit error. Invalid request time -> invalid request.

### 5. Good/Base/Bad Cases
Good: system rows before a user row do not consume `has_more`. Base: a genuinely
unready index returns lexical fallback with a warning. Bad: treating NaN as an
equal score or restarting a cursor after changing models.

### 6. Tests Required
Assert page concatenation, mode/model/dimension/visibility binding, facet
delimiter collision resistance, backend errors, invalid times, and strong
relevance retaining priority over weak repository/mainline preference.

### 7. Wrong vs Correct
Wrong: fetch `page+1`, then discard noise/metadata mismatches.
Correct: filter before top-k, preserve the bounded ranking window, then page.

## Scenario: Match-centered search snippet window

### 1. Scope / Trigger
`SearchHit.text` assembly on every search path (lexical, semantic, hybrid,
grouped or not) and every entry point that renders the shared projection
(CLI human/robot, MCP, Robot, Web, TUI, handoff).

### 2. Signatures
`assemble_search_hit(hit, payload, session, max_snippet_chars, query_terms)`
delegates to `snippet::build(full_text: Option<&str>, terms: &[String],
max_snippet_chars: usize) -> Option<String>`. `terms` is the same
`guidance::literal_terms(&query)` instance used by `why_matched`; no second
tokenizer, prefix operator or FTS syntax is introduced.

### 3. Contracts
Text comes only from the canonical payload `text` string; missing/non-JSON
payloads keep `text: None`. With literal evidence the snippet is an exact
contiguous original slice anchored on the earliest hit (smallest start, ties
by term order) and grown 2 right : 1 left until `max_snippet_chars` or the text
boundary; an anchor that alone reaches the cap emits its leading slice. With no
literal evidence (no match, empty terms, semantic-only) the previous
`chars().take(max_snippet_chars)` prefix is preserved. Matching is
case-insensitive through per-character lowercase expansion with an origin map
back to original scalar boundaries; the expanded offsets are never sliced
directly. Output never inserts ellipses/highlights and stays within the
character cap; bytes are charged through the existing `search_hit_charge`
estimate against `max_response_bytes`. The window is display-only: ranking,
cursor digests, `why_matched`, suggestions, occurrences, evidence and facets
are unchanged. A renderer may re-center its own bounded preview on the same
literal terms, but must not alter the wire `text` or fall back to a prefix that
hides a provable hit (CLI human mode does exactly this for its 120-char line).

### 4. Validation & Error Matrix
No new error variants: request budgets are already validated (floor 1); a zero
cap degrades to an empty window instead of panicking. A missing payload row,
non-JSON payload or non-string `text` yields `None`, never a fabricated hit.

### 5. Good/Base/Bad Cases
Good: a hit whose match sits far beyond the old prefix still shows the match in
`text`. Base: short text is emitted whole; a query with no literal evidence
keeps the prefix. Bad: slicing the lowercase-expanded string by its own
offsets, emitting an empty window when the anchor exceeds the cap, or letting a
multi-scalar expansion (`İ`) split an original scalar.

### 6. Tests Required
Assert anchor selection (earliest hit, absent earlier term), 2-right : 1-left
expansion, anchor-over-cap leading slice, CJK bigram/emoji/combining cases, the
`İ` expansion origin map, prefix fallback for no match/empty terms/empty/null
text, exact-slice containment, zero-cap safety, and that clamped pages still
charge window bytes and report `max_response_bytes`.

### 7. Wrong vs Correct
Wrong: reuse lowercase offsets as original offsets, or keep prefix truncation
and claim the hit is invisible-by-design.
Correct: map expansion offsets back to original scalar boundaries and emit a
bounded, contiguous, evidence-anchored window.

## Scenario: Pure relocation identity and plan policy

### 1. Scope / Trigger
A CLI relocation preview/apply or an installation namespace lookup crosses
physical path, persisted identity and catalog-generation boundaries.

### 2. Signatures
`relocation::{issue_plan, decode_plan, verify_plan, alias_expiry_ms}` take
injected milliseconds; `legacy_installation_namespace`,
`normalize_absolute_path`, `path_is_within`, `validate_root_mapping` and
`remap_source_path` are pure path/compatibility functions.

### 3. Contracts
Keep the legacy namespace derivation byte-for-byte compatible. Compare Windows
ASCII casing and separators component-wise; preserve Unicode spelling and
native IDs. Require absolute roots, reject ambiguous dot components and strict
ancestor overlaps, allow equivalent roots as a no-op. Destination host semantics
and actual filesystem access belong to adapters, not these helpers.

A versioned, domain-separated plan binds schema, generation, the complete
adapter-computed ownership/fingerprint digest and retention policy. Its 15-minute
lifetime is checked using the injected clock, and its 2048-byte ceiling is
checked before decode. Claims contain only digests and scalar values. Integrity
is not authentication. Reuse the cursor's base64 implementation, but keep the
relocation token domain/claims distinct. Ordinary generation mismatch is checked
before comparing mapping facts. A replay requires a matching committed receipt
and current complete ownership, never merely a matching token.

### 4. Validation & Error Matrix
Invalid, malformed, expired or mismatched plan/path -> `InvalidRequest`;
changed generation -> `GenerationMismatch`. Do not echo token contents, roots,
namespace seeds or native IDs. Never silently generate a replacement plan.

### 5. Good/Base/Bad Cases
Good: a moved directory retains its persisted seed. Base: equivalent casing
produces the same comparison key. Bad: Unicode normalization changes native
identity, or an expired plan is accepted because its digest still matches.

### 6. Tests Required
Pin compatibility seeds, plan tampering/unknown fields/size/TTL boundaries,
generation precedence, checked time arithmetic, strict component ancestry,
Windows drive/UNC/verbatim separators and Unicode-preserving remaps.

### 7. Wrong vs Correct
Wrong: read the wall clock or filesystem inside plan policy.
Correct: receive time and ownership digests through explicit arguments.

## Scenario: Durable journal-maintenance orchestration

### 1. Scope / Trigger
`MaintenanceService` coordinates queue and catalog ports without filesystem or
SQLite imports. The composition root owns process lifetime and sleeping.

### 2. Signatures
`submit(target, token, budget)` validates confirmation and enqueues idempotently;
`retry` retains the same authorization; `next_runnable` and `next_retry_at`
select work; `run_job` executes/reconciles durable phases under one session.

### 3. Contracts
Persist backup intent before copying and phase completion before moving on.
Actual catalog audit, not queue acknowledgement alone, decides whether logical
compaction committed. Cancellation stops remaining work, never undoes a commit.
Control requests must survive worker progress writes. Never present an
unreconciled commit outcome after a crash as a known negative. Before cleanup,
atomically persist a cleanup-started intent against cancellation. Once claimed,
cancellation is closed and must be reported as such; finish/retry cleanup
rather than reporting cancelled with an already-deleted backup.
Default budget is 30 seconds. Backoff is 1/2/5/15/30/60 seconds; three consecutive
budget exhaustions without durable progress require attention. Reset that
counter only on durable progress; keep timing data bounded per phase.

### 4. Validation & Error Matrix
Selected drift requires new preview, not retry-with-refreshed-selection.
Busy/pause defer; resource/cleanup failures require attention. Successful
cleanup precedes completed. Drop catalog lease before retry waiting.

### 5. Good/Base/Bad Cases
Good: checkpoint retry skips acknowledged VACUUM. Base: duplicate token returns
its existing job. Bad: a cancelled crashed job claims nothing committed solely
because queue acknowledgement was absent.

### 6. Tests Required
Use injected clocks and mock ports for phase ordering, backup intent, budget
reset/threshold, bounded backoff, pause/cancel, commit-ack gaps and invalid
adapter phase transitions. Queue tests cover concurrent controls separately.

### 7. Wrong vs Correct
Wrong: resume by replaying every phase or re-previewing all current batches.
Correct: reconcile committed work and continue only the fixed authorized set.

## Quality Check

- New or modified filesystem test helpers must not use PID + wall-clock time
  alone as a unique directory name: even `as_nanos()` can repeat across parallel
  callers. Use a process-local atomic nonce and exclusive `create_dir` for the
  fixture leaf, rather than silently reusing it with `create_dir_all`. Cover
  parallel allocations at a fixed clock tick without serializing the suite
  (see `candle_embedding::tests::tempfile_dirs_are_isolated_for_parallel_callers_at_one_clock_tick`).
- No concrete adapter imports; no `rusqlite`, no `std::fs` reads of sources.
- Handlers are exhaustive over `AppRequest`; no catch-all that silently drops
  a new request kind.
- Errors carry their origin layer via `AppError`; never flatten a
  `ProviderError` into a generic string here.
- Provider selection never guesses on ambiguity — it rejects.
- `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`
  + `cargo test --workspace`.

---

**Language**: All documentation in **English**.
