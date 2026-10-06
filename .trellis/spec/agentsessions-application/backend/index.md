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

## Quality Check

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
