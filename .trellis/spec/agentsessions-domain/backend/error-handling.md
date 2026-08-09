# Error Handling — agentsessions-domain

> How the domain layer models and raises errors.

---

## Overview

The domain crate owns `DomainError`, the innermost error type. It is raised
when a domain invariant is violated or a request is semantically invalid,
independent of any storage, provider, or transport concern. Everything the
domain returns is a `Result<T, DomainError>`; there are no panics on the
expected-failure path and no `unwrap`/`expect` in non-test code.

`DomainError` carries a human-safe message only — never a secret, a raw file
path, or a transcript excerpt. Outer layers map it onto their own error
provenance (`AppError::Domain`) and the CLI maps it further onto a canonical
protocol code.

---

## Error Types

`DomainError` variants (see `src/error.rs`):

- `NotFound(String)` — a requested domain entity does not exist.
- `InvalidRequest(String)` — the request violates a domain rule (bad argument,
  malformed value) before any backend is touched.
- `InvariantViolation(String)` — a "this should never happen" breach of an
  internal guarantee; signals a bug, not user error. Maps to `internal` /
  exit 70 at the CLI.
- `UnstableIdentity(String)` — an operation requires a stable identity but only
  an `Unstable` `StableId` is available (see the stable-id three-tier model in
  `index.md`).
- `AmbiguousGraph(String)` — parent resolution found multiple candidate
  placements for a child and none is uniquely selected by the child's source
  document; the request fails loudly rather than picking an arbitrary branch.
  Maps to a client-visible error at the CLI, not `internal`.

Do not add a new variant unless a genuinely new failure *category* appears.
Prefer reusing `InvalidRequest` for "caller passed something wrong" and
`InvariantViolation` for "our own code broke a guarantee". Every new variant
forces a matching arm in the CLI's `From<DomainError> for ProtocolError`.

---

## Error Handling Patterns

- Construct with an owned `String` message that states the fact, not the fix:
  `DomainError::InvalidRequest("parent id must be a Message".into())`.
- Propagate with `?`. The domain layer never logs — it returns.
- `Message::validate` / `Session::validate` are the canonical examples: they
  check id kind and id-value consistency; `SessionContextGraph::validate`
  walks structural invariants (edge parent must reference a
  `IdKind::Message`, spans within document bounds) and returns
  `InvariantViolation` / `InvalidRequest` rather than mutating or guessing.
- Keep messages deterministic. The same violation must produce the same string
  so contract tests and cross-layer mapping stay stable.

---

## API Error Responses

The domain has no wire surface. It does not format JSON, choose exit codes, or
decide `retryable`. That is the CLI protocol layer's job. The domain only
guarantees: a stable variant + a safe message. If you find yourself thinking
about HTTP status, exit codes, or JSON shape here, the logic belongs in the CLI
layer instead.

---

## Right / Wrong

Raising a failure on invalid input (from real `src/error.rs` + `Session::validate`):

```rust
// Wrong — panics on the expected-failure path, leaks the raw value,
// and gives outer layers nothing stable to map onto.
fn set_parent(&mut self, parent: &StableId) {
    assert!(parent.kind() == IdKind::Message, "bad parent {parent:?}");
}

// Right — return a stable variant with a safe, deterministic message.
fn set_parent(&mut self, parent: &StableId) -> DomainResult<()> {
    if parent.kind() != IdKind::Message {
        return Err(DomainError::InvariantViolation(
            "parent id must be a Message".into(),
        ));
    }
    self.parent = Some(parent.clone());
    Ok(())
}
```

Choosing the variant:

```rust
// Wrong — user passed a malformed argument, but this maps to exit 70 (bug).
return Err(DomainError::InvariantViolation("empty query".into()));

// Right — bad caller input is InvalidRequest (exit 2), not a bug signal.
return Err(DomainError::InvalidRequest("query must not be empty".into()));
```

---

## Common Mistakes

- **Leaking data into the message.** Never interpolate a file path, a payload
  body, or a raw transcript line into a `DomainError` string.
- **Adding a variant for a single call site.** If only one place raises it and
  it is really "caller passed something wrong", use `InvalidRequest`.
- **Using `InvariantViolation` for user error.** It maps to exit 70 (bug
  signal). Reserve it for broken internal guarantees, not bad input.
- **Panicking on the expected-failure path.** No `unwrap`/`expect` outside
  tests; return a `DomainError`.
