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
- `evidence` module — `EvidenceSpanDto` assembly from stored canonical
  placements/documents; `occurrence_id` is the Placement ID, exact spans retain
  their source document, and missing placement spans remain explicitly
  `precision: unknown`.
- Pagination model — offset inside cursor claims over a PINNED total order
  (search: bm25 + id tiebreak = `SORT_SCORE_DESC`; list: wire id ASC =
  `SORT_WIRE_ID_ASC`; recency browse: newest session activity DESC, sessions
  without a provider timestamp last, wire id ASC tiebreak = `SORT_RECENCY_DESC`,
  selected by `ListSort` on `AppRequest::List` and only valid with
  `sessions_only`). The sort identifier is bound into the cursor, so a token
  issued under one sort is rejected under another. Ports have no offset
  parameter: `handle` over-fetches
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
  half-open `[since, until)`. Omitted filters preserve the legacy digest and
  storage query path.
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
  (seq / native_id / parent_native_id / role / text / timestamp / is_sidechain / span).
- `StagedBatch` — all emitted messages plus the complete provider `ParseReport`;
  callers must retain committed/skipped/diagnostic accounting rather than
  replacing it with guessed zeros.

---

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
