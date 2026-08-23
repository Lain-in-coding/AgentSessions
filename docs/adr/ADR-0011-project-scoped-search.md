# ADR-0011: Project-scoped search uses trusted working-directory claims

> Governance record — implementation decision for M3-3.

- decision_id: ADR-0011
- status: Accepted for the 0.2 candidate
- owner: QIN

## Decision

Search is global by default. `--project <path-or-name>` and the equivalent MCP/Web
filters are explicit restrictions; `--exclude-project` removes matching trusted
projects.

Project attribution comes only from a resolved, pair-observed Original Working
Directory claim associated with the canonical Session. Transcript/source paths
are never project facts and are never returned as attribution. Missing,
ambiguous, conflicting, or unpaired claims do not satisfy a positive project
filter; an unknown session remains searchable when only an exclusion filter is
present.

A value containing a path separator matches the normalized path exactly or a
path descendant with a separator boundary. A separator-free value matches the
final path component (case-insensitive). Values within one filter dimension are
OR-ed; positive and exclusion dimensions are AND-ed.

Human output may show the trusted local working directory only where an existing
human-output decision permits it. Robot JSON, MCP, HTTP/Web, and handoff output
carry only the final project component or `null`; they never carry an absolute
source path.

## Consequences

- The SQL predicate is applied before relevance `LIMIT`, so pagination does not
  discard rows and filter afterward.
- Continuation cursors bind the normalized project dimensions; changing scope
  invalidates the old cursor.
- Semantic requests with an explicit project scope use the lexical path that
  supports trusted metadata predicates and report the effective mode honestly.
- A project with no trustworthy claim cannot be discovered by guessing from a
  transcript filename or source directory.
