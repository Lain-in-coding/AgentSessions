# Sidechain and subagent facets

## Goal

Expose and filter main Session versus sidechain/subagent history without changing canonical ownership semantics.

## Requirements

### R1 Facets
- Search and context gain an explicit sidechain facet so users can include or exclude subagent/sidechain history.
- Mainline selection remains the deterministic default; the facet is a filter, not a re-ownership.

### R2 Canonical ownership
- Messages remain placed in the Sessions and Sources where the provider recorded them; the facet never moves or re-owns a Message.
- Sidechain placement evidence is preserved and inspectable.

### R3 Honesty
- When a facet is requested but the Source/Provider does not expose sidechain markers, results honestly report that rather than guessing.
- Empty or fully-filtered results are a clean empty result, not an error.

## Acceptance Criteria

- [ ] Search/context filter by mainline versus sidechain.
- [ ] Canonical ownership and placement evidence are unchanged by the facet.
- [ ] Missing sidechain markers are reported honestly, not guessed.
- [ ] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- Sequence after the Resume core stream; depends on existing sidechain placement modeling.
- Additive contract changes only.
- No commit or push without explicit owner authorization.
