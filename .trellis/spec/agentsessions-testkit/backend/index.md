# agentsessions-testkit — Backend Guidelines

> Shared test helpers and builders. Keeps test setup consistent and avoids
> duplicating fixture-construction logic across crates.

---

## Role in the architecture

`agentsessions-testkit` provides reusable builders and helpers for tests across
the workspace. `SessionBuilder` constructs a valid placement-aware
`SessionContextGraph`; `InMemoryStore` implements `CatalogStore`,
`SearchIndex`, and `ContextGraphStore`.

Real file: `src/lib.rs`.

---

## Pre-Development Checklist

- [ ] When a stable or contextual domain type changes, update Message,
      placement, edge, document, and graph construction together. Do not put
      parent/ordinal/sidechain/span back onto stable `Message`.
- [ ] Builders produce **valid** domain objects by default (pass
      `SessionContextGraph::validate`). Provide setters for the fields a test
      needs to vary.
- [ ] This crate is test-support only. Do not pull production logic into it, and
      do not depend on it from non-test production code.
- [ ] Keep fixture data synthetic and redacted; never embed a real transcript.

---

## Quality Check

- [ ] `cargo fmt --all --check` clean, `cargo clippy ... -D warnings` clean.
- [ ] `cargo test -p agentsessions-testkit` green.
- [ ] Builder changes keep the full-workspace build green
      (`cargo test --workspace`), since every crate's tests depend on it.
- [ ] `InMemoryStore::message_contexts` returns candidates grouped by distinct
      Session and `context_stats` remains aggregate-only.

---

**Language**: write all guideline docs in **English**.
