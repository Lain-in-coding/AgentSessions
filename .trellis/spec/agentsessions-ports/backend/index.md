# agentsessions-ports — Backend Guidelines

> Hexagonal port layer. Defines the trait boundaries between the application
> core and concrete adapters. No I/O, no concrete backend, no `std::fs`.

---

## Role in the architecture

`agentsessions-ports` is the **interface layer** of the hexagon. It declares the
traits (ports) that the application core depends on and that adapters
(`adapters-sqlite`, `provider-claude`, `provider-codex`) implement. It is the
narrowest crate on purpose: it names capabilities, it does not perform them.

Depends only on `agentsessions-domain`. Never depends on any adapter crate,
`rusqlite`, `serde_json`, or the filesystem.

---

## Pre-Development Checklist

- [ ] Am I adding a *capability the core needs*, or leaking an adapter detail?
      Ports describe what the core requires, not how SQLite/FS provides it.
- [ ] Does the new method belong on an existing port, or is it a genuinely new
      seam? Prefer extending an existing trait when there is one caller.
- [ ] Does the signature use domain types (`StableId`, `Session`, `Message`)
      and `PortResult<T>` — never `rusqlite::Error` or raw `io::Error`?
- [ ] If I add a `PortError` variant, does it map to a canonical protocol code
      in the CLI's `protocol.rs` `From<PortError>`? Adding a variant without
      updating that mapping breaks the error contract.
- [ ] Structured event params (e.g. `MessageEvent`) — should this be a struct
      rather than a positional tuple, so fields can be added without breaking
      every call site?

---

## Key types

- `PortError` / `PortResult<T>` — the error currency crossing every port.
  Variants: `Backend`, `SourceIo`, `SchemaIncompatible`, `NotFound`,
  `SnapshotChanged`, `WriterBusy`. Each maps to a canonical code downstream.
- `CanonicalEventSink` + `MessageEvent<'a>` — structured sink for provider
  parse output. `MessageEvent` is a struct (native_id / parent_native_id /
  role / text / timestamp / is_sidechain) precisely so new fields don't break
  callers.
- `ProviderAdapter` — `probe(bytes) -> Probe` + `parse(bytes, sink)`. The
  probe/select contract lives here; the selection *policy* lives in the
  application core.

---

## Quality Check

- No `use rusqlite`, no `use std::fs`, no `serde_json` in this crate.
- Every port method returns `PortResult<_>`; concrete adapter errors are
  wrapped into `PortError` by the adapter, never surfaced raw.
- New `PortError` variants have a matching arm in the CLI protocol mapping.
- Traits stay object-safe where the composition root needs `&dyn Trait`
  (e.g. `ProviderAdapter` is used as `&dyn ProviderAdapter` in the registry).
- `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`.

---

**Language**: All documentation in **English**.
