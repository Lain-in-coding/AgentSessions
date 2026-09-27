# Relocation CI compatibility design

## Scope

The change is limited to the SQLite adapter's private f32 BLOB decoding helper and its unit test. The helper is used by semantic vector reads, so the implementation must remain byte-compatible and must not alter model IDs, dimensions, generations, or query behavior.

## Design

`bytes_to_f32_vec` will use the stable fixed-size slice API:

```rust
let (chunks, _) = bytes.as_chunks::<4>();
chunks.iter().map(|chunk| f32::from_le_bytes(*chunk)).collect()
```

`as_chunks::<4>()` returns only complete four-byte arrays and a remainder slice. The remainder is intentionally ignored, preserving the old `chunks_exact(4)` behavior without relying on indexing each chunk or triggering the hosted Clippy lint.

The existing serializer remains unchanged and continues to emit little-endian bytes. No schema migration, public API, cursor version, or relocation logic changes are required.

## Compatibility and failure behavior

- Empty input returns an empty vector.
- A byte length divisible by four decodes every element.
- A byte length with a remainder drops the remainder.
- The function cannot panic on a short remainder because the remainder is never indexed.
- Invalid vector dimension and non-finite-value validation remain owned by the surrounding semantic indexing code.

## Rollback

Revert the single implementation/test commit if hosted validation reveals a compiler/MSRV issue. Do not add a lint allow or weaken warnings.
