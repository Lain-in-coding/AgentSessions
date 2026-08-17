# Semantic model bundle (offline import)

The optional `semantic-candle` feature loads a **local** multilingual-e5-small
bundle. Default builds do **not** enable this feature and continue to use the
honest `bigram-hash-v1` fuzzy-lexical vectorizer.

## Bundle layout

```text
<bundle>/
  config.json
  tokenizer.json
  model.safetensors
  MODEL-MANIFEST.json
```

`MODEL-MANIFEST.json` example:

```json
{
  "model_id": "intfloat-multilingual-e5-small@614241f6-candle-f32-meanpool-l2-qpass-v1",
  "dimension": 384,
  "license": "MIT (intfloat/multilingual-e5-small model weights)",
  "files": [
    { "name": "config.json", "sha256": "<hex>", "size_bytes": 0 },
    { "name": "tokenizer.json", "sha256": "<hex>", "size_bytes": 0 },
    { "name": "model.safetensors", "sha256": "<hex>", "size_bytes": 0 }
  ]
}
```

The `model_id` is part of the storage contract. Changing pooling, prefixes,
precision, truncation, or weights **must** change the id so old vectors stay
inert under `message_vec.model_id` scoping.

## Import (never downloads)

Build a semantic-capable binary:

```text
cargo build --release --features semantic-candle -p agent-session-grep-cli
```

Import a verified local bundle:

```text
asg model import --dir <bundle>
asg model status
asg --db <path> index embeddings
asg --db <path> search "query" --mode semantic
```

Import verifies every declared SHA-256, then atomically publishes under
`{cache}/models/<sanitized-model-id>/` from `config paths`. Search/sync/index
paths never open a network socket; missing weights fall back to bigram-hash
with the existing explicit warning / `lexical_fallback` behavior.

## What is still deferred

- Official weight packaging / redistribution decision (license notice in release
  materials).
- Frozen CJK/English/code recall + latency benchmark gate before any marketing
  claim that semantic quality exceeds lexical.
- Default-on semantic in release binaries (explicitly not planned for 0.1.0).
