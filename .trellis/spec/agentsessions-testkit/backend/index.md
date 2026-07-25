# agentsessions-testkit — Backend Guidelines

> Shared test helpers and builders. Keeps test setup consistent and avoids
> duplicating fixture-construction logic across crates.

---

## Role in the architecture

`agentsessions-testkit` provides reusable builders and helpers for tests across
the workspace (e.g. `SessionBuilder`). It exists so tests construct canonical
domain objects consistently instead of hand-rolling them in each crate.

Real file: `src/lib.rs`.

---

## Pre-Development Checklist

- [ ] When a domain type gains a field (e.g. `Message.parent` / `timestamp` /
      `is_sidechain`), update the builder here so every downstream test compiles
      and stays realistic. A missing field here breaks the whole workspace build.
- [ ] Builders produce **valid** domain objects by default (pass
      `Session::validate`). Provide setters for the fields a test needs to vary.
- [ ] This crate is test-support only. Do not pull production logic into it, and
      do not depend on it from non-test production code.
- [ ] Keep fixture data synthetic and redacted; never embed a real transcript.

---

## Quality Check

- [ ] `cargo fmt --all --check` clean, `cargo clippy ... -D warnings` clean.
- [ ] `cargo test -p agentsessions-testkit` green.
- [ ] Builder changes keep the full-workspace build green
      (`cargo test --workspace`), since every crate's tests depend on it.

---

**Language**: write all guideline docs in **English**.
