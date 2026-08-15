# Resume Metadata core and surfaces — Implementation

## Preconditions

- `task.py start` this task before editing.
- Work in the integration worktree. Depends on provider observation, namespaced identity, and protocol hardening being present. Sequence this task after the three prerequisites to avoid editing the same shared contract types concurrently.

## Steps

1. **ports**: add `SessionResumeMetadata` + batched availability types (fixed nullable).
2. **adapters-sqlite**: add `source_session_resume_claims` table; write in the same transaction as source replacement; clear on source removal; bump schema for backfill; add batched read (chunked IN, statement-count bounded).
3. **application**: `GetSessionResume`/`SessionResume`; search hits gain `resume_available`; charge new fields to byte budget (JSON-escaped length).
4. **cli human**: horizontal table `日期 | Provider | 会话标题 | 工作目录 | Session ID` with compression rules and `—`.
5. **cli robot/MCP**: full structured values; new `get_session_resume` tool (kind validation); tool list 7 → 8; TUI detail panel.
6. **Skill**: table rendering + two-step auto-resolution docs.

## Validation

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --release`
- Manual: `search` on a small corpus shows the table; `get_session_resume` returns fixed nullable fields; no Source path leaks.

## Review gates

- `trellis-check`; verify no-N+1 bounds, budget accounting, privacy (no path leak), table truncation semantics, MCP kind validation.
- Do NOT commit/push.
