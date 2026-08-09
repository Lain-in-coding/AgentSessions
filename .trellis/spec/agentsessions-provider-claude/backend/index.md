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
Claude Code also emits event-only `type: "system"` rows without `message`;
these are metadata and must not increment `ParseReport.skipped`. A
message-bearing `system` row remains readable for compatibility.

### Content normalization

- Ordinary block-array content joins every text-bearing block in source order
  with `\n`; non-text tool blocks remain ignored.
- A local-command envelope is recognized only when the first text block starts
  with `<command-name>` and contains `<command-message>` and `<command-args>`.
- Claude Code may copy that stable message as an enriched block array: the
  first block is the original envelope plus one trailing LF,
  followed by generated stdout/command blocks. For this recognized shape only,
  canonical text is the first block with that one trailing LF removed.
- Claude-generated meta prompts are marked `isMeta: true`. Their top-level
  timestamp is copy-local rather than stable and therefore emits as unknown.
  A copied/enriched meta prompt is collapsed to its original final text block
  only when `sessionKind` is present and the exact block sequence is
  `text, text, image, text`. Similar ordinary or differently-shaped multimodal
  content keeps every text block.
- Do not trim ordinary messages, concatenate the generated blocks into the
  stable payload, select content by length, or weaken downstream stable-message
  conflict checks. Genuine role/text/timestamp differences remain conflicts.

---

## Pre-Development Checklist

- [ ] Identity uses the native `uuid` (`StableId::native`), **not** a path+seq
      derivation. Native identity survives migration/rename; only fall back to
      Reconstructed when a native id is genuinely absent.
- [ ] Emit through `MessageEvent` (seq, native_id, parent_native_id, role, text,
      timestamp, is_sidechain). Do not collapse fields back to `(role, text)`.
- [ ] `probe` returns a confidence, never panics on malformed input. Unknown
      shapes return a low/ambiguous confidence, not an error.
- [ ] `user`/`assistant` rows without `message` remain recoverable skips.
      Event-only `system` rows are ignored; message-bearing `system` rows emit.
- [ ] Local-command normalization is limited to the recognized envelope shape.
      Ordinary multi-block text and meaningful whitespace remain byte-stable.
- [ ] Meta-prompt normalization requires `isMeta`, `sessionKind`, and the exact
      enriched block shape; ordinary multimodal messages are never shortened.
- [ ] Parsing is streaming and read-only. Never write to or mutate the source
      file (RFC-0002 §7 — sources are strictly read-only).
- [ ] Fixtures are synthetic and redacted (see `docs/security/FIXTURE-REDACTION-POLICY.md`).
      Never commit a real user's transcript.

---

## Quality Check

- [ ] `cargo fmt --all --check` clean, `cargo clippy ... -D warnings` clean.
- [ ] `cargo test -p agentsessions-provider-claude` green.
- [ ] Synthetic tests prove string/enriched local-command equivalence, ordinary
      multi-block preservation, genuine command differences, and system-event
      classification without using real transcript content. Meta tests also
      prove copy-local timestamp removal, exact-shape enrichment collapse, and
      preservation of similar non-meta/differently-shaped content.
- [ ] Native identity + threading assertions still hold (msg id = native uuid,
      parent edge preserved through storage to `show`).

---

**Language**: write all guideline docs in **English**.
