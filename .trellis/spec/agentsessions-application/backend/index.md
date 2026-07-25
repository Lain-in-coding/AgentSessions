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

- `AppError` — `Domain(DomainError)`, `Port(PortError)`, `Provider(ProviderError)`.
  Preserves the origin layer so the protocol layer maps each to the right code.
- `AppRequest` / `AppResponse` — the use-case envelope. Handlers match
  exhaustively; a new variant must be rendered by the CLI.
- `select_and_stage(adapters, bytes)` — probe/select policy: pick the highest
  non-ambiguous confidence adapter; a top-confidence tie across *different*
  variants is an error, not a coin flip.
- `StagedMessage` — the in-memory staged row before commit
  (seq / native_id / parent_native_id / role / text / timestamp / is_sidechain).

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
