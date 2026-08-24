# Provider Beta Readiness Ledger

> Machine-readable companion to `PROVIDER-MATURITY-MATRIX.md`.
> Authority for maturity/capability values remains
> `crates/agent-session-grep-ports/src/capability.rs`.
> This ledger records **why no provider is Beta yet** and separates
> repository-local gaps from external/owner gates. Do not promote from this
> file alone.

Last updated: 2026-08-25 (HEAD after the capability-claim honesty wave; adds the
`discover` column, which was previously absent and therefore unguarded).

## Global external blockers (apply to every promotion)

| Blocker | Owner | Notes |
|---|---|---|
| Named successful cross-target CI run | GitHub billing + CI | `last_certified_targets` stays empty until a green Windows/Linux/macOS run id is recorded |
| ADR-0010 accepted_at | owner/approver | Rollback policy is Proposed only |
| Independent owner promotion decision | owner | RFC-0002 §6 forbids code-existence promotion |

## Per-provider local readiness (implemented 14)

Legend for local columns: `ok` = present with tests; `partial` = present with known holes; `missing` = not implemented / unsupported in capability matrix.

| provider_id | golden | read-only | discover | source_span | tool_activity | resume | incremental | local Beta blockers (beyond global) |
|---|---|---|---|---|---|---|---|---|
| claude-code | ok | ok | native | ok (native) | partial | derived | native | richer tool-call extraction; owner promotion still required |
| codex | ok | ok | native | ok (native) | partial | derived | native | richer tool-call extraction; owner promotion still required |
| grok-build | ok | ok | missing | ok (native) | missing | derived | missing | no discovery root; no incremental/tool_activity; synthetic msg ids |
| antigravity | ok | ok | native | ok (native) | missing | unknown | missing | no in-file session id; no tool_activity/incremental |
| opencode | ok | ok | native | missing | missing | unknown | missing | no span (SQLite source has no in-file byte offsets); no resume template |
| pi | ok | ok | native | ok (native) | missing | derived | missing | no tool_activity/incremental |
| hermes | ok | ok | missing | missing | missing | unknown | missing | JSON doc; no span; no discovery root |
| cursor | ok | ok | missing | missing | missing | unknown | missing | no discovery root (the `workspaceStorage` layout is unverified on any dev machine, and guessing it would tombstone the index); no span; multi-gen format layering pending |
| kimi-code | ok | ok | missing | ok (native) | missing | unknown | missing | no discovery root; loop events not parsed |
| openclaw | ok | ok | native | ok (native) | missing | unsupported | missing | resume intentionally unsupported |
| qoder | ok | ok | missing | ok (native) | missing | unknown | missing | no discovery root; non-dialogue records skipped |
| tencent-codebuddy | ok | ok | native | ok (native) | missing | unknown | missing | extension variant pending |
| cline | ok | ok | missing | missing | missing | unsupported | missing | no session id / span; no discovery root |
| aider | ok | ok | missing | derived | missing | unsupported | missing | approximate spans; no resume; no discovery root; no tool_activity (blockquote tool output is folded into assistant text, no structured call/result records) |

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
