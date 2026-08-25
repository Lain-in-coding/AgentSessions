# Provider Beta Readiness Ledger

> Machine-readable companion to `PROVIDER-MATURITY-MATRIX.md`.
> Authority for maturity/capability values remains
> `crates/agent-session-grep-ports/src/capability.rs`.
> This ledger records **why no provider is Beta yet** and separates
> repository-local gaps from external/owner gates. Do not promote from this
> file alone.

Last updated: 2026-08-25 (property-test wave A: adds the `property` column —
seeded randomized property suite `tests/properties.rs` — now ok for
`claude-code`, `codex`, `grok-build`, `antigravity`, `opencode`, `pi`,
`hermes`, and `cursor`; guarded both directions against the file's existence).

## Global external blockers (apply to every promotion)

| Blocker | Owner | Notes |
|---|---|---|
| Named successful cross-target CI run | GitHub billing + CI | `last_certified_targets` stays empty until a green Windows/Linux/macOS run id is recorded |
| ADR-0010 accepted_at | owner/approver | Rollback policy is Proposed only |
| Independent owner promotion decision | owner | RFC-0002 §6 forbids code-existence promotion |

## Per-provider local readiness (implemented 14)

Legend for local columns: `ok` = present with tests; `partial` = present with known holes; `missing` = not implemented / unsupported in capability matrix. `property` = seeded randomized property suite (`tests/properties.rs`, mirroring claude-code/codex).

| provider_id | golden | read-only | property | discover | source_span | tool_activity | resume | incremental | local Beta blockers (beyond global) |
|---|---|---|---|---|---|---|---|---|---|
| claude-code | ok | ok | ok | native | ok (native) | partial | derived | derived | richer tool-call extraction; owner promotion still required |
| codex | ok | ok | ok | native | ok (native) | partial | derived | derived | richer tool-call extraction; owner promotion still required |
| grok-build | ok | ok | ok | native | ok (native) | missing | derived | derived | no tool_activity; synthetic msg ids |
| antigravity | ok | ok | ok | native | ok (native) | missing | unknown | derived | no in-file session id; no tool_activity |
| opencode | ok | ok | ok | native | missing | missing | unknown | derived | no span (SQLite source has no in-file byte offsets); no resume template |
| pi | ok | ok | ok | native | ok (native) | missing | derived | derived | no tool_activity |
| hermes | ok | ok | ok | native | missing | missing | unknown | derived | JSON doc; no span; SQLite `state.db` surface not parsed |
| cursor | ok | ok | ok | missing | missing | missing | unknown | derived | no discovery root: the adapter parses the VS Code `workspaceStorage/*/state.vscdb` ItemTable surface, whose per-workspace hash directories sit under a platform-specific application-data path, not a home-relative root this table can express; `~/.cursor/chats/<id>/store.db` is the separate Cursor CLI `meta`/`blobs` schema, which this adapter's probe rejects. No span; multi-gen format layering pending |
| kimi-code | ok | ok | missing | native | ok (native) | missing | unknown | derived | loop events not parsed |
| openclaw | ok | ok | missing | native | ok (native) | missing | unsupported | derived | resume intentionally unsupported |
| qoder | ok | ok | missing | native | ok (native) | missing | unknown | derived | discovery covers the transcript-JSONL surface only (the Electron SQLite store is a separate, unimplemented surface); non-dialogue records skipped |
| tencent-codebuddy | ok | ok | missing | native | ok (native) | missing | unknown | derived | extension variant pending |
| cline | ok | ok | missing | native | missing | missing | unsupported | derived | no session id / span; discovery covers the `~/.cline/data/tasks` tree only (the VS Code extension `globalStorage` tree is not home-relative and is not registered) |
| aider | ok | ok | missing | missing | derived | missing | unsupported | derived | approximate spans; no resume; no discovery root by construction — `.aider.chat.history.md` lives at the root of each user repository, so upstream agentsview discovers it by walking working trees rather than one canonical home directory; no tool_activity (blockquote tool output is folded into assistant text, no structured call/result records) |

## Deferred providers (not Beta candidates)

| provider_id | status | blocker |
|---|---|---|
| deepseek-harness | Unsupported | no transcript evidence |
| zcode | Unsupported | no transcript evidence |

## Honest promotion rule

A provider may be advertised as **Beta** only when:

1. every local Beta column above is `ok` (or an explicit, owner-approved exception is recorded);
2. a named cross-target CI success is written into `AdapterManifest.last_certified_targets`;
3. ADR-0010 is Accepted; and
4. the owner records the promotion decision with evidence paths.

Until then the public matrix stays **Experimental** for all 14 implemented providers.
