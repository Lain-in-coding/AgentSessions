# agentsessions-provider-claude — Backend Guidelines

> Adapter that probes and parses Claude Code JSONL transcripts into canonical
> messages. Isolated behind the provider contract; never touches search/catalog.

---

## Role in the architecture

`agentsessions-provider-claude` implements the `ProviderAdapter` port
(`probe` + `parse`) for Claude Code transcripts. Per RFC-0002, adapter format
differences are isolated here — adding or changing a provider must **not**
require edits to the search core, catalog, or application layer.

Real file: `src/lib.rs` (exports `ClaudeCodeAdapter`).

---

## The real transcript format (source of truth)

Claude Code writes one JSON object per line. Conversation rows have
`type: "user" | "assistant"` with these load-bearing top-level fields:

- `uuid` — provider-native message id → maps to `Stability::Native`
- `parentUuid` — parent message uuid (null on root) → threading edge
- `timestamp` — ISO-8601 UTC, ordering key
- `message.role` + `message.content` (string or block array)
- `isSidechain` (bool) — marks subagent/branch records

Metadata rows (`type` = `mode` / `permission-mode` / `summary` /
`file-history-snapshot`, etc.) are **not** conversation and must be ignored.

---

## Pre-Development Checklist

- [ ] Identity uses the native `uuid` (`StableId::native`), **not** a path+seq
      derivation. Native identity survives migration/rename; only fall back to
      Reconstructed when a native id is genuinely absent.
- [ ] Emit through `MessageEvent` (seq, native_id, parent_native_id, role, text,
      timestamp, is_sidechain). Do not collapse fields back to `(role, text)`.
- [ ] `probe` returns a confidence, never panics on malformed input. Unknown
      shapes return a low/ambiguous confidence, not an error.
- [ ] Parsing is streaming and read-only. Never write to or mutate the source
      file (RFC-0002 §7 — sources are strictly read-only).
- [ ] Fixtures are synthetic and redacted (see `docs/security/FIXTURE-REDACTION-POLICY.md`).
      Never commit a real user's transcript.

---

## Quality Check

- [ ] `cargo fmt --all --check` clean, `cargo clippy ... -D warnings` clean.
- [ ] `cargo test -p agentsessions-provider-claude` green.
- [ ] Native identity + threading assertions still hold (msg id = native uuid,
      parent edge preserved through storage to `show`).

---

**Language**: write all guideline docs in **English**.
