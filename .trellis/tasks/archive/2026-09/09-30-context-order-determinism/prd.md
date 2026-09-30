# Deterministic context ordering

## Goal
Make full-context ordering and mainline leaf selection independent of the input placement permutation, including malformed provider timestamps.

## Requirements
- Apply one total order: missing timestamps first, invalid timestamps next ordered by original bytes, valid timestamps last ordered by their existing parsed UTC instant.
- Preserve document, source ordinal and placement-ID tie-breaks; both full and mainline use the same comparator.
- Preserve timestamp text, stored identity, valid timestamp parsing/precision, graph validation, branch/edge/sidechain/cycle behavior, and public APIs.

## Acceptance Criteria
- [x] All six permutations of the review's three-message graph yield one full order and one leaf.
- [x] Missing/invalid/valid mixtures, offset-equivalent times, precision and deterministic randomized permutations satisfy the new contract.
- [x] Existing valid-only ordering and graph/branch tests remain green.
- [x] Domain tests and targeted lint pass offline; no schema/dependency/API change.

## Scope
Domain thread ordering and its tests only. Main owns Domain spec updates; other children own providers/CLI/storage. Owner explicitly chose separate invalid-time grouping on 2026-09-30 and then approved implementation.
