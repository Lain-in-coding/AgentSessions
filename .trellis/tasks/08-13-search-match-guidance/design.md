# Design — Search match guidance

## Boundary

Application assembles evidence and suggested calls after search hits have session IDs and text. Storage continues to return ranked IDs/scores and does not generate presentation guidance. CLI/MCP serialize additive fields; human rendering ignores them.

## Evidence

Normalize the user's literal query with the existing CJK/plain-text rules and compare deterministic terms while the full catalog payload is still available. Evidence is assembled in Application before per-hit clamping; it must not depend only on the displayed `text` prefix because a literal match may occur beyond that prefix. Emit a small ordered evidence list naming literal terms or fields, never raw FTS expressions.

## Suggested calls

Generate templates only when required IDs are present. Prefer the integrated `get_message` call with message and session identifiers and a small bounded `around`, then a session context call. The command list has a fixed small maximum. Application includes the JSON-serialized/escaped size of both guidance collections in each hit's byte estimate before `clamp_items`; CLI/MCP never append unbudgeted guidance after clamping.

## Compatibility

Guidance is assembled after rank order is fixed. Filter normalization remains part of cursor binding, but guidance does not alter query digest, scores, sorting, pagination, or human output.

## Rollback

Remove the two additive hit fields and their serializer projections; storage and ranking stay untouched.
