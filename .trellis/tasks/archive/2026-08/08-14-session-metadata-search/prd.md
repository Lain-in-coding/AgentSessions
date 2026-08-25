# Session Metadata Search

## Goal

Search resolved Provider Session ID, the current title-like first user request projection, and pair-observed Original Working Directory without indexing Source paths. Provider custom title and structural summary remain deferred until an authoritative Canonical contract exists.

## Requirements

### R1 Queryable metadata
- Query tokens may match the resolved Provider Session ID, the deterministic first chronological valid user request (the current title-like projection), and the pair-observed Original Working Directory.
- Provider-specific custom title and structural summary fields are not yet part of the Canonical provider contract; they remain an explicit follow-up and are never invented from opaque records.
- Original Working Directory is the authoritative provider-recorded value; it is metadata, not a liveness guarantee.

### R2 Source-path safety
- Source and Transcript paths are never indexed or returned in ordinary responses.
- A metadata hit never exposes the Source locator.

### R3 Result semantics
- Metadata candidates are deduplicated against matching non-system Message hits at the adapter boundary; the existing default search surface remains Message-grained.
- `group_by_session=true` deduplicates the final result by Canonical Session and reports `occurrences`; metadata-only Sessions without a non-system Message remain discoverable as canonical Session hits.
- Search hits can join with Resume Metadata availability.

## Acceptance Criteria

- [x] Metadata-only queries return Sessions by resolved Provider Session ID, first chronological user request, and pair-observed working directory; provider custom title/summary remain explicitly deferred.
- [x] No Source path is indexed, returned, or diagnosable.
- [x] Session hits deduplicate against matching non-system Message hits and join with `resume_available`; final canonical Session dedup remains available through `group_by_session`.
- [x] Budgets enforce JSON-escaped byte accounting for returned search fields.
- [x] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- Depends on the Resume Metadata claim projection; sequence after the core task.
- Additive contract changes only.
- No commit or push without explicit owner authorization.
