# Error Handling — provider-codex

> How the Codex rollout adapter handles the envelope format, UI-mirror
> deduplication, and parse failures.

---

## Overview

`provider-codex` parses real Codex CLI `rollout-*.jsonl` files into
canonical messages. Like `provider-claude`, it reads untrusted
real-world data and favors **skip-and-continue for individual bad
records** while **hard-failing on structural violations**. Its
defining failure mode is different from Claude's: the danger here is
**double-counting**, not threading corruption.

Errors leave this crate as `ProviderError` (from `agentsessions-ports`).

---

## Error Types

The only error type this crate returns is `ProviderError`
(defined in `agentsessions-ports`). Do not define a local error enum.

- `probe()` returns a confidence. The variant id is
  `codex/rollout-jsonl-v1`; a file that is not a Codex rollout yields
  `Confidence::Ambiguous`, never an error.
- `parse()` returns `PortResult<()>` and emits through the
  `CanonicalEventSink`.

---

## Error Handling Patterns

**The envelope is `{timestamp, type, payload}`; conversation is nested.**
Unlike Claude, the conversation is not at the top level. Read the outer
`type` first, then `payload.type`. The authoritative conversation record
is `type == "response_item"` AND `payload.type == "message"`.

**Ignore the `event_msg` UI mirror — this is the single most important
rule.** Every conversation message appears twice in a rollout: once as
the authoritative `response_item/message` (carries a native `id`) and
once as an `event_msg` (`user_message` / `agent_message`, a UI mirror
with no id and identical text). Emitting both **double-counts** every
message. Only emit `response_item/message`. This is not an error
condition — the mirror is expected — but emitting it is a silent
correctness bug that no exception will catch.

**Timestamp comes from the outer envelope**, not from `payload`. Reading
it from the wrong level yields `None` for every message.

**Skip other record types silently** (`session_meta`, `turn_context`,
`world_state`, `token_count`, `reasoning`, tool-call records). They are
expected non-conversation records, not errors.

**Codex is linear — there is no `parentUuid`.** `parent_native_id` is
always `None`; threading is reconstructed from sequence order. Do not
invent parent edges.

---

## API Error Responses

No external API surface. On failure the crate returns a `ProviderError`;
the CLI maps it to the `provider_error` canonical code (exit 7). This
crate does not choose the exit code.

---

## Common Mistakes

- **Emitting the `event_msg` mirror**, double-counting every message.
  The verified real-rollout fixture asserts N distinct `response_item`
  messages ingest as exactly N, not 2N. See [index](./index.md).
- **Reading `timestamp` from `payload`** instead of the outer envelope
  → every message loses its timestamp.
- **Treating the `developer` role as invalid.** Codex has a `developer`
  role (system/permission layer) that Claude does not; map it, do not
  reject it.
- **Truncating the native `id` prefix before comparing** — rollout ids
  share a session seed prefix; a truncated compare can false-positive as
  a duplicate.
- **Leaking the source path into the error message.**
