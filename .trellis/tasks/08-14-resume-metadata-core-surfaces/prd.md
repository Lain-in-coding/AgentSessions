# Resume Metadata core and surfaces

## Goal

Persist source-scoped Resume Metadata, resolve it in the application and search, and expose read-only CLI, Robot, MCP, TUI, and Skill surfaces without executing any command or exposing a Source path.

## Requirements

### R1 Persistence
- Store Resume Metadata claims source-scoped, keyed by Source, so a Source's removal clears its claims and a Source replacement atomically replaces its claims in the same transaction.
- Claims preserve the ID/working-directory association and never pair independently observed values.
- A normal directory change within a Session is the earliest authoritative working directory, not a conflict.

### R2 Resolution and budget
- A batched read path resolves availability across search-page hits without N+1.
- All new fields and arrays are charged to the JSON-escaped serialized `max_response_bytes`, not raw character count.

### R3 Application contract
- Add `AppRequest::GetSessionResume` and `AppResponse::SessionResume` with fixed nullable fields: canonical `session_id`, `provider_id`, `provider_session_id`, `original_working_directory`, `resume_available`, and `unavailable_reason`.
- Search hits gain `resume_available`; history stays searchable even when not resumable.
- Fields are always present; unknown or ambiguous values are `null`, never guessed or omitted.

### R4 Entry surfaces
- Human CLI renders one horizontal table `日期 | Provider | 会话标题 | 工作目录 | Session ID`; Provider and Session ID are never truncated; missing values render `—`.
- TUI shows the same fields in adaptive columns with a detail panel.
- Robot JSON/JSONL and MCP carry complete structured values with no Markdown.
- MCP adds a read-only `get_session_resume` tool that validates the canonical `ses_v1_*` kind and returns the fixed nullable metadata.
- The project Skill instructs agents to render MCP/Robot results as the same table and to auto-resolve the batched page metadata.

### R5 Disclosure safety
- Ordinary responses never expose Transcript or Source paths.
- Resume Metadata is not inserted into FTS text, opaque Session payloads, diagnostics, progress frames, or errors.

## Acceptance Criteria

- [ ] Source-scoped claims persist, replace atomically, and clear on Source removal.
- [ ] Batched availability resolution has no N+1 and satisfies the byte budget.
- [ ] `get_session_resume` returns the fixed nullable contract; search hits carry `resume_available`.
- [ ] Human table renders with complete Provider/Session ID and `—` for missing values.
- [ ] Robot and MCP return complete structured values; MCP validates the Session ID kind.
- [ ] Skill documents the two-step auto-resolution and the table rendering.
- [ ] No Source path leaks into any ordinary surface.
- [ ] fmt, clippy `-D warnings`, test workspace, and release build are green.

## Constraints

- The first version returns structured data only; it never constructs or executes a shell command, changes directory, opens a terminal, or auto-copies.
- Existing canonical grouping and cursor behavior remain stable.
- No commit or push without explicit owner authorization.
