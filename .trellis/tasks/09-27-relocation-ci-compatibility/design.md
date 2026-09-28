# Relocation CI compatibility design

## Scope

The initial change covers the SQLite adapter's private f32 BLOB decoding helper and its unit test; the hosted follow-up below also covers CLI fixtures and the directly reproduced legacy identity lookup defect. The helper is used by semantic vector reads, so the implementation must remain byte-compatible and must not alter model IDs, dimensions, generations, or query behavior.

## Design

`bytes_to_f32_vec` will use the stable fixed-size slice API:

```rust
let (chunks, _) = bytes.as_chunks::<4>();
chunks.iter().map(|chunk| f32::from_le_bytes(*chunk)).collect()
```

`as_chunks::<4>()` returns only complete four-byte arrays and a remainder slice. The remainder is intentionally ignored, preserving the old `chunks_exact(4)` behavior without relying on indexing each chunk or triggering the hosted Clippy lint.

The existing serializer remains unchanged and continues to emit little-endian bytes. No schema migration, public API, or cursor version changes are required. The legacy lookup correction below restores the existing relocation proof contract.

## Compatibility and failure behavior

- Empty input returns an empty vector.
- A byte length divisible by four decodes every element.
- A byte length with a remainder drops the remainder.
- The function cannot panic on a short remainder because the remainder is never indexed.
- Invalid vector dimension and non-finite-value validation remain owned by the surrounding semantic indexing code.

## Rollback

Revert the single implementation/test commit if hosted validation reveals a compiler/MSRV issue. Do not add a lint allow or weaken warnings.

## Hosted test follow-up (2026-09-28)

The next hosted run reached CLI tests and exposed a legacy v6 re-ingest fixture mismatch on Unix plus a macOS cwd assertion comparing two path spellings for one directory. The additional allowlist is the CLI test module and its directly used test helpers. The existing production identity validation remains authoritative: every native session in a legacy source must be proved before registration.

The cwd test must verify the spawned process reached the original directory by resolving filesystem paths on both sides. It must still verify first-run preview, later execution, and provider arguments. Fixture repair must reproduce the actual platform identity namespace, retaining the old session IDs after complete re-ingest. Any discovered production-contract defect requires an explicit design update before implementation.

## Confirmed legacy locator defect and repair boundary (2026-09-28)

The Windows negative regression fails in both debug and release before production edits: expected `invalid_request`/exit 2, observed success/exit 0, two scan rows, an `allocated-v1` namespace, and a generation advance. The prior raw UUID Session remains in old membership while a new scoped Session is registered. This is a production lookup bypass, so correcting only the positive fixture would be insufficient.

At the shared storage boundary before namespace selection/allocation, compare an incoming absolute locator with existing unbound legacy scan locators using the project's existing lexical path-key contract. A matching historical locator with different spelling, or an ambiguous set of historical locators, requires explicit provenance resolution and must fail closed. Do not infer filesystem symlink/relative/case-folded aliases beyond that contract, rewrite stored paths, or manufacture Native metadata. Do not weaken `validate_legacy_source_proof`.

Preserve the registered-source/alias path and exact valid legacy proof path. Avoid scanning every registered source on the normal lookup path; inspect only the unmatched legacy boundary where historical locator equivalence matters. Scope any helper to storage unless an existing interface can express the same invariant cleanly. No schema or public-port expansion is planned.

The production allowlist expands to SQLite `src/relocation.rs` and `src/relocation/tests.rs`; a narrowly necessary CLI composition change requires an evidence-backed explanation. Regressions must cover raw and normalized input spellings, ambiguous historical rows, failure atomicity (catalog/claims/namespace/generation and original locator), valid exact legacy proof, and unchanged registered alias behavior. Because ingest, sync and discovery all resolve namespaces, check that every route traverses this guard.

## Published MSRV dependency patch (2026-09-28)

Actual Rust 1.90 compilation found a pre-existing dependency failure. Apply
only the official forward pair rusqlite 0.40.2/libsqlite3-sys 0.38.2 documented
in `research/msrv-dependency.md`. The dependency allowlist is `Cargo.lock` plus
Cargo.toml in the SQLite adapter, CLI, provider-cursor and provider-opencode.
Raise the existing rusqlite lower bounds to 0.40.2 without feature changes;
retain Rust 1.90 and bundled SQLite 3.53.2. This is not a semantic-stack upgrade
or an audit-policy change. Validate both the advertised MSRV and current stable.
