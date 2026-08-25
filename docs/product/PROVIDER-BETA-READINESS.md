# Provider Beta Readiness Ledger

> Machine-readable companion to `PROVIDER-MATURITY-MATRIX.md`.
> Authority for maturity/capability values remains
> `crates/agent-session-grep-ports/src/capability.rs`.
> This ledger records **why no provider is Beta yet** and separates
> repository-local gaps from external/owner gates. Do not promote from this
> file alone.

Last updated: 2026-08-25 (adds a `property` local column recording the
seeded randomized property-test wave: kimi-code / openclaw / qoder /
tencent-codebuddy / cline / aider now carry `properties.rs` suites mirroring
the Claude/Codex coverage; the remaining six land in the sibling wave).

## Global external blockers (apply to every promotion)

| Blocker | Owner | Notes |
|---|---|---|
| Named successful cross-target CI run | GitHub billing + CI | `last_certified_targets` stays empty until a green Windows/Linux/macOS run id is recorded |
| ADR-0010 accepted_at | owner/approver | Rollback policy is Proposed only |
| Independent owner promotion decision | owner | RFC-0002 §6 forbids code-existence promotion |

## Per-provider local readiness (implemented 14)

Legend for local columns: `ok` = present with tests; `partial` = present with known holes; `missing` = not implemented / unsupported in capability matrix; `pending` = not yet landed in this wave (sibling wave in progress).

| provider_id | golden | read-only | discover | source_span | tool_activity | resume | incremental | property | local Beta blockers (beyond global) |
|---|---|---|---|---|---|---|---|---|---|
| claude-code | ok | ok | native | ok (native) | partial | derived | derived | ok | richer tool-call extraction; owner promotion still required |
| codex | ok | ok | native | ok (native) | partial | derived | derived | ok | richer tool-call extraction; owner promotion still required |
| grok-build | ok | ok | native | ok (native) | missing | derived | derived | pending | no tool_activity; synthetic msg ids |
| antigravity | ok | ok | native | ok (native) | missing | unknown | derived | pending | no in-file session id; no tool_activity |
| opencode | ok | ok | native | missing | missing | unknown | derived | pending | no span (SQLite source has no in-file byte offsets); no resume template |
| pi | ok | ok | native | ok (native) | missing | derived | derived | pending | no tool_activity |
| hermes | ok | ok | native | missing | missing | unknown | derived | pending | JSON doc; no span; SQLite `state.db` surface not parsed |
| cursor | ok | ok | missing | missing | missing | unknown | derived | pending | no discovery root: the adapter parses the VS Code `workspaceStorage/*/state.vscdb` ItemTable surface, whose per-workspace hash directories sit under a platform-specific application-data path, not a home-relative root this table can express; `~/.cursor/chats/<id>/store.db` is the separate Cursor CLI `meta`/`blobs` schema, which this adapter's probe rejects. No span; multi-gen format layering pending |
| kimi-code | ok | ok | native | ok (native) | missing | unknown | derived | ok | loop events not parsed |
| openclaw | ok | ok | native | ok (native) | missing | unsupported | derived | ok | resume intentionally unsupported |
| qoder | ok | ok | native | ok (native) | missing | unknown | derived | ok | discovery covers the transcript-JSONL surface only (the Electron SQLite store is a separate, unimplemented surface); non-dialogue records skipped |
| tencent-codebuddy | ok | ok | native | ok (native) | missing | unknown | derived | ok | extension variant pending |
| cline | ok | ok | native | missing | missing | unsupported | derived | ok | no session id / span; discovery covers the `~/.cline/data/tasks` tree only (the VS Code extension `globalStorage` tree is not home-relative and is not registered) |
| aider | ok | ok | missing | derived | missing | unsupported | derived | ok | approximate spans; no resume; no discovery root by construction — `.aider.chat.history.md` lives at the root of each user repository, so upstream agentsview discovers it by walking working trees rather than one canonical home directory; no tool_activity (blockquote tool output is folded into assistant text, no structured call/result records) |

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
