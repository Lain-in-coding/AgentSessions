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
  (`developer` / `user` / `assistant`), and `payload.content[]` blocks.
- `timestamp` lives on the **outer** envelope, not inside the payload.
- There is **no** `parentUuid` — Codex is a linear sequence; threading is by seq
  order only, so `parent_native_id` is always `None`.

**Deduplication (critical)**: every conversation message appears twice — once as
`response_item/message` (authoritative, has id) and once as `event_msg`
(`user_message` / `agent_message`, a UI mirror, no id, same text). Parse **only**
`response_item/message`; ignore the `event_msg` mirror or you double-count.

Other record types (`session_meta`, `turn_context`, `world_state`,
`response_item/reasoning`, tool calls, `token_count`) are ignored.

---

## Pre-Development Checklist

- [ ] Only emit `response_item` + `payload.type == "message"`. Never emit
      `event_msg` conversation mirrors.
- [ ] Take `timestamp` from the outer envelope, not the payload.
- [ ] Identity uses native `payload.id` (`StableId::native`). Do not truncate
      ids when comparing — session-seeded prefixes are shared.
- [ ] `parent_native_id` is always `None` (linear). Do not fabricate threading.
- [ ] Rollout files are large (484KB–8MB); parse streaming, never load whole.
- [ ] Read-only sources; synthetic redacted fixtures only.

---

## Quality Check

- [ ] `cargo fmt --all --check` clean, `cargo clippy ... -D warnings` clean.
- [ ] `cargo test -p agentsessions-provider-codex` green.
- [ ] Dedup assertion holds: N conversation messages → N stored entries, not 2N.

---

**Language**: write all guideline docs in **English**.
