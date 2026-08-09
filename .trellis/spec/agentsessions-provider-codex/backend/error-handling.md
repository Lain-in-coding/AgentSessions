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

**The outer-envelope timestamp is occurrence-local, not stable Message
content.** Copied sources can contain the same native message under different
outer timestamps. Continue recognizing the `{timestamp, type, payload}` shape
for probe/classification, but emit `timestamp: None` for
`response_item/message`. Do not copy the outer timestamp into the canonical
event or use it in stable Message conflict checking.

This exclusion is narrow. Native `payload.id` remains the stable Message
identity, and role/text differences under one native ID still fail loudly and
atomically downstream. The rule does not make conflicting stable content
mergeable.

**Skip other record types silently** (`session_meta`, `turn_context`,
`world_state`, `token_count`, `reasoning`, tool-call records). They are
expected non-conversation records, not errors.
Some `response_item/reasoning` payloads explicitly carry `content: null`.
Payload classification must happen before message-content validation, so this
shape is ignored rather than reported as a malformed record. By contrast,
`response_item/message` with null or missing content remains a recoverable
skip because an authoritative conversation occurrence could not be emitted.
The same applies to a `response_item/message` with an unknown or empty role:
it is counted as a recoverable skip with a diagnostic (never silently
dropped), because the conversation occurrence cannot be classified.

**Codex is linear — there is no `parentUuid`.** `parent_native_id` is
always `None`; threading is reconstructed from sequence order. Do not
invent parent edges.

The adapter remains **Experimental**, and RFC-0002 remains **Draft**. Handling
the verified timestamp shape does not promote either status.

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
- **Deserializing every `response_item` as a message before checking
  `payload.type`**. Expected reasoning payloads may have `content: null`; do
  not count those as parse loss.
- **Persisting the outer-envelope `timestamp` on stable Message**. It is a
  copied-source occurrence fact; emit `None` while retaining the envelope
  field for probe/classification.
- **Treating the `developer` role as invalid.** Codex has a `developer`
  role (system/permission layer) that Claude does not; map it, do not
  reject it.
- **Truncating the native `id` prefix before comparing** — rollout ids
  share a session seed prefix; a truncated compare can false-positive as
  a duplicate.
- **Leaking the source path, native id, transcript text, or envelope contents
  into the error message.**
