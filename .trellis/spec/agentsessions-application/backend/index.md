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
- [ ] Provider selection: does the change respect
      `Confirmed > High > Low`, and *reject* ties across different variants
      rather than guessing?

---

## Key types

- `AppError` — `Domain(DomainError)`, `Port(PortError)`, `Provider(ProviderError)`,
  `Cursor(CursorError)`, `Budget(BudgetError)`. Preserves the origin layer so
  the protocol layer maps each to the right code (cursor errors have dedicated
  canonical codes: `cursor_invalid` / `cursor_expired` / `generation_mismatch`).
- `AppRequest` / `AppResponse` — the use-case envelope. Handlers match
  exhaustively; a new variant must be rendered by the CLI (`render` in main.rs).
  `Search`/`List` carry `cursor: Option<String>` + `budget: ResponseBudget`;
  `Context { session_id, policy, budget }` assembles a session branch.
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
  payloads; never invents values (missing span → `precision: unknown`, all
  location fields null).
- Pagination model — offset inside cursor claims over a PINNED total order
  (search: bm25 + id tiebreak = `SORT_SCORE_DESC`; list: wire id ASC =
  `SORT_WIRE_ID_ASC`). Ports have no offset parameter: `handle` over-fetches
  `offset + page + 1` (sentinel for has_more) and slices. Any cursor failure is
  an explicit error — never a silent restart from page one.
- Context assembly — reads the session payload (`{document, messages}`),
  rebuilds domain `Message`s (seq = member index, parent from the RESOLVED
  `parent` wire id the CLI persists), selects via `select_mainline`/`select_full`,
  then applies `max_messages` + byte gate + `max_evidence_spans`. A session row
  written by ingest that is malformed → `InvariantViolation` (loud, not lenient).
- `select_and_stage(adapters, bytes)` — probe/select policy: pick the highest
  non-ambiguous confidence adapter; a top-confidence tie across *different*
  variants is an error, not a coin flip.
- `StagedMessage` — the in-memory staged row before commit
  (seq / native_id / parent_native_id / role / text / timestamp / is_sidechain / span).

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
