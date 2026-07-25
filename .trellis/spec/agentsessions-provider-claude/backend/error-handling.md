# Error Handling — provider-claude

> How the Claude Code adapter handles malformed transcripts, unknown
> records, and parse failures.

---

## Overview

`provider-claude` parses real Claude Code `.jsonl` transcripts into
canonical messages. It touches untrusted, real-world data (a user's
actual session files), so its error handling favors **skip-and-continue
over abort** for individual bad lines, but **hard-fails on structural
violations** it cannot honor safely.

Errors leave this crate as `ProviderError` (from `agentsessions-ports`).
The adapter never surfaces `DomainError` or `PortError`; anything it
cannot classify becomes a `ProviderError` with a safe, path-free message.

---

## Error Types

The only error type this crate returns is `ProviderError`
(defined in `agentsessions-ports`). Do not define a local error enum.

- `probe()` returns a confidence, not an error. A file it does not
  recognize yields `Confidence::Ambiguous`, never a failure — the
  registry decides what to do with a no-match.
- `parse()` returns `PortResult<()>` and emits messages through the
  `CanonicalEventSink`. A structural problem (e.g. a line that claims to
  be a conversation record but has no resolvable role) becomes a
  `ProviderError`.

---

## Error Handling Patterns

**Skip non-conversation records silently.** Claude transcripts contain
many `type` values that are not messages (`mode`, `permission-mode`,
`file-history-snapshot`, `summary`, sidechain bookkeeping). These are
expected, not errors: skip them without emitting and without warning.

**Skip malformed individual lines, do not abort the file.** A single
line that fails JSON parsing is a defect in one record, not proof the
whole file is unreadable. Skip it and continue; the rest of the
transcript is still valuable. Aborting the whole parse on one bad line
would make one corrupt record destroy an entire session's searchability.

**Hard-fail only on structural contract violations** the adapter cannot
represent — for example, a record that parses as a message but carries a
`parentUuid` that cannot be a message identity. That is a
`ProviderError`, because emitting it would corrupt the threading DAG
downstream.

**Native identity is authoritative.** When a record has a `uuid`, use it
(`Stability::Native`). Only fall back to reconstructed path+seq identity
when the native `uuid` is absent. Never silently downgrade a present
`uuid` to reconstructed — that loses cross-session stability.

---

## API Error Responses

This crate has no external API surface. Its "response" is the sequence
of `MessageEvent`s pushed to the sink plus, on failure, a
`ProviderError`. The CLI maps that `ProviderError` to the
`provider_error` canonical code (exit 7); this crate does not decide the
exit code.

---

## Common Mistakes

- **Treating unknown `type` values as errors.** They are the norm in
  Claude transcripts. Only conversation records (`user` / `assistant`)
  are in scope; everything else is skipped, not failed.
- **Aborting the whole file on one bad line.** One malformed record must
  not sink the rest of the session.
- **Dropping the native `uuid`** and re-deriving a reconstructed id. If
  the transcript gives a `uuid`, it is the identity — see the identity
  rules in [index](./index.md).
- **Leaking the source path into the error message.** Provider errors
  cross into logs and Robot output; keep real filesystem paths out of
  the message text.
