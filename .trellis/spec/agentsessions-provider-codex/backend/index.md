# agentsessions-provider-codex — Backend Guidelines

> Adapter that probes and parses Codex CLI rollout JSONL into canonical
> messages. Isolated behind the provider contract; never touches search/catalog.

---

## Role in the architecture

`agentsessions-provider-codex` implements the `ProviderAdapter` port for Codex
rollout files. Like the Claude adapter, format differences stay isolated here
(RFC-0002).

Real file: `src/lib.rs` (exports `CodexAdapter`, variant `codex/rollout-jsonl-v1`).

---

## The real rollout format (source of truth)

Unlike Claude, the conversation is **not** at the top level. Each line is an
envelope `{timestamp, type, payload}`:

- The authoritative conversation record is `type: "response_item"` **and**
  `payload.type: "message"` — it carries a native `payload.id`, `payload.role`
  (`developer` / `user` / `assistant` / `system`), and `payload.content[]`
  blocks.
- A `response_item/message` with an unknown or empty role cannot be emitted; it
  is counted as a recoverable skip (`report.skipped += 1`) with a diagnostic,
  exactly like the null-content path — never dropped silently.
- `timestamp` lives on the **outer** envelope, not inside the payload. Verified
  copied-source behavior shows that this value is occurrence-local: the same
  native message can appear under different outer timestamps. Keep the envelope
  field available for probe/classification, but emit `MessageEvent.timestamp`
  as `None`; it must not enter the stable canonical `Message`.
- There is **no** `parentUuid` — Codex is a linear sequence; threading is by seq
  order only, so `parent_native_id` is always `None`.

**Deduplication (critical)**: every conversation message appears twice — once as
`response_item/message` (authoritative, has id) and once as `event_msg`
(`user_message` / `agent_message`, a UI mirror, no id, same text). Parse **only**
`response_item/message`; ignore the `event_msg` mirror or you double-count.

`session_meta.payload.id` is the **current thread ID**, while `session_id` is
its **root thread ID** and can legitimately differ. Only this envelope treats id
as Session identity; prefer its trimmed nonempty value and retain session_id-only
legacy input compatibility. Do not reject differing fields as conflicting aliases,
merge child threads under the root, or count root identity as an additional Session.
Missing IDs do not authorize cwd-only pairing, and malformed types do not gain
fallback authority. Multiple distinct current-thread headers still report the
existing multi-session ambiguity. Pin: openai/codex
`c5d242fa7907bff1b7a7e26e95febc548c0a6963`, protocol `SessionMeta` /
`SessionMetaLine::deserialize` and rollout `builder_from_session_meta`.

Other record types (`session_meta`, `turn_context`, `world_state`,
`response_item/reasoning`, tool calls, `token_count`) emit no conversation messages;
metadata, tool activity and usage keep their separate existing handling.
`response_item/reasoning` may carry `content: null`; classify the payload type
before interpreting message content so this expected non-conversation shape
does not increment `ParseReport.skipped`. A `response_item/message` still
requires a content-block array, and null/missing content is a recoverable skip.

This occurrence-local timestamp rule does not weaken stable conflict semantics:
native `payload.id` remains the Message identity, and differing stable role or
text under one native Message ID still fails loudly. `event_msg` mirrors remain
excluded rather than becoming a second occurrence source.

The adapter remains **Experimental** and the provider contract remains
**Draft**. This verified parsing rule is not a maturity or governance promotion.

---

## Pre-Development Checklist

- [ ] Only emit `response_item` + `payload.type == "message"`. Never emit
      `event_msg` conversation mirrors.
- [ ] Classify non-message `response_item` payloads before validating message
      content. In particular, `reasoning` with `content: null` is ignored,
      while a message without a content array remains a recoverable skip.
- [ ] Use the outer `timestamp` only as part of the envelope shape for
      probe/classification; emit `timestamp: None` for the canonical event.
- [ ] Message identity uses native `response_item/message.payload.id`
      (`StableId::native`). Do not truncate
      ids when comparing — session-seeded prefixes are shared.
- [ ] Preserve strict stable conflicts for native identity, role, and text;
      occurrence-local envelope timestamps do not participate.
- [ ] `parent_native_id` is always `None` (linear). Do not fabricate threading.
- [ ] Rollout files are large (484KB–8MB); parse streaming, never load whole.
- [ ] Read-only sources; synthetic redacted fixtures only. Never expose paths,
      native ids, transcript text, or other source content in diagnostics.

---

## Quality Check

- [ ] `cargo fmt --all --check` clean, `cargo clippy ... -D warnings` clean.
- [ ] `cargo test -p agentsessions-provider-codex` green.
- [ ] Dedup assertion holds: N conversation messages → N stored entries, not 2N.

---

**Language**: write all guideline docs in **English**.
