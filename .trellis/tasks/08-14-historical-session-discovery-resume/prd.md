# Historical Session Discovery and Resume

## Goal

Enable users and agents to discover historical Sessions across providers and obtain trustworthy, read-only Resume Metadata without confusing Canonical Session identity with provider-native resume identity.

## Requirements

### R1 Resume Metadata contract

- Keep canonical `session_id` (`ses_v1_*`) as the Catalog association and navigation identity.
- Represent the exact, nullable Provider Session ID separately and always interpret it with `provider_id`.
- Return only the Original Working Directory recorded by an authoritative Provider field; never derive it from a Source path, Transcript directory, repository root, or Data Root.
- Search exposes whether selected Session metadata is resolvable; an explicit resolver exposes the fixed nullable Resume Metadata fields and unavailable reason.
- Missing, damaged, legacy, or ambiguous metadata never makes searchable history disappear and is never guessed.

### R2 Safety and disclosure

- All capabilities remain local-first and read-only with respect to Provider Sources.
- The first version never constructs or executes a shell command, changes directory, opens a terminal, launches a Provider, or copies a command automatically.
- Ordinary responses never expose Transcript or Source paths.
- Provider Session IDs and Original Working Directories are explicit Resume Metadata disclosures; they are not inserted into FTS text, opaque Session payloads, diagnostics, progress frames, or errors.

### R3 User experience

- Human results use one horizontal table with columns `日期 | Provider | 会话标题 | 工作目录 | Session ID`.
- Human dates use local `YYYY-MM-DD`; machine surfaces retain full timestamps.
- Provider and Provider Session ID are never truncated. Title may truncate at the end and working directory may collapse in the middle. Missing values render as `—`.
- All matched Sessions remain obtainable in relevance order, with recent activity as a tiebreak; pagination transports bounded blocks but does not impose a hidden top-N product limit.
- Robot and MCP return complete structured values without Markdown; the project Skill renders those values with the same table semantics.

### R4 Integrity prerequisites

- Canonical native Session identity includes Provider and stable installation namespace so equal native IDs from different providers or installations cannot collide.
- A Source containing multiple native Session IDs is split only when association is reliable; otherwise Resume Metadata fails closed.
- Robot schema evolution is versioned instead of silently changing the closed `1.0` schema.
- Cursors are bound to the exact result set that minted them.
- MCP validates JSON-RPC envelopes, initialization state, IDs, params, budget floors, bounded errors, and the chosen byte-budget meaning before Resume tools are added.

### R5 Historical discovery roadmap

- Add Session metadata search without exposing Source paths.
- Add explicit Provider source auto-discovery with honest complete/incomplete-root semantics.
- Add sidechain/subagent facets and structured Tool Activity.
- Add Gemini and OpenCode first, then evaluate later Providers only when source-format evidence exists.

### R6 Provider maturity

- Search support and Resume support are separate Provider capabilities.
- A Provider can remain searchable with unavailable Resume Metadata.
- Provider maturity is never promoted without golden fixtures, span evidence, privacy checks, and full quality gates.

## Acceptance Criteria

- [ ] Existing 08-13 integration is verified as a recorded baseline before new shared contracts land.
- [ ] Protocol, cursor, schema, and Session identity prerequisites are implemented and independently verified.
- [ ] Claude Code and Codex extract authoritative, associated Resume Metadata and fail closed on ambiguity.
- [ ] Resume Metadata is persisted source-scoped, resolved in batches, budgeted, and exposed through Application, CLI, Robot, MCP, TUI, and Skill surfaces.
- [ ] Metadata search, discovery, facets, Tool Activity, and Provider-expansion children are completed or explicitly deferred with their task status truthful.
- [ ] Cross-layer E2E, privacy/read-only checks, release build, and full-corpus Gate D are green before the umbrella is considered complete.
- [ ] Contracts, ADRs, glossary, schemas, Skill, and operational documentation agree with runtime behavior.
- [ ] No commit, push, merge, or Provider Source modification occurs without the required owner action.

## Constraints

- The repository remains private until the owner makes a separate release decision.
- Existing Canonical Session grouping and message ownership semantics remain stable.
- Changes are additive where compatibility permits; migrations fail closed and preserve rollback paths.
- Parent completion does not imply every speculative Provider is supported; unsupported Providers remain honestly marked.
