# Error Handling — agentsessions-cli

> How the CLI normalizes every failure into the Robot protocol.

---

## Overview

The CLI is the composition root and the only layer that decides wire-facing
error semantics. Every failure — domain, port, provider, or argument — is
normalized to a single `ProtocolError` (`code` + safe `message`), then projected
onto both an exit code and a JSON error envelope through one mapping. No command
invents its own error shape.

Canonical rules:

- stdout carries protocol data only; human-facing diagnostics go to stderr.
- The error envelope carries a stable `code`, a safe `message`, `retryable`, and
  bounded `details`.
- `schema_version` is major.minor; an unknown major must be rejected by callers.

---

## Error Types

- `ProtocolError { code: CanonicalCode, message }` — the single normalized CLI
  error.
- `CanonicalCode` is the error catalog: `invalid_request`, `not_found`,
  `source_io`, `source_changed`, `snapshot_failed`, `catalog_error`,
  `provider_error`, `writer_busy`, `schema_incompatible`,
  `cursor_invalid`, `cursor_expired`, `generation_mismatch`, `internal`.
  Each code fixes its exit code and `retryable` flag — a new error must be
  registered here before any entry point may return it. The three cursor
  codes are produced by the pagination layer (bad token, expired token,
  generation changed mid-paging).
- `CliError` is a thin wrapper for argument/usage validation (maps to
  `invalid_request` → exit 2).

---

## Error Handling Patterns

- Convert at the composition root: `From<DomainError>`, `From<PortError>`,
  `From<ProviderError>`, and `From<AppError>` all funnel into `ProtocolError`.
  `AppError` fans out by provenance (`Domain` / `Port` / `Provider`) to keep the
  category correct.
- Exit codes follow the catalog (`exit_code()`), e.g. invalid_request→2,
  not_found→4, source_io/source_changed/snapshot_failed→5,
  catalog_error/writer_busy→6, provider_error→7, schema_incompatible→9,
  internal→70.
- `retryable()` is true only for transient categories (`writer_busy`,
  `source_changed`); everything else is false.
- The published catalog (`schemas/robot/v1/error-catalog.json`) and envelope
  schema are cross-checked against the runtime mapping by tests in
  `protocol.rs`. If you change a code, exit code, or `retryable`, update the
  schema in the same change or the drift test fails.

---

## API Error Responses

Error envelope shape (single stdout object):

```
{"schema_version":"1.0","frame_type":"error","command":"<cmd>",
 "request_id":"...","ok":false,"outcome":"failure",
 "error":{"code":"<canonical>","message":"<safe>","retryable":<bool>,"details":{}},
 "warnings":[],"page":{...},"meta":{"duration_ms":0,"generation":null}}
```

Unknown or missing `--output` values are themselves `invalid_request` errors —
never silently downgrade to a different protocol.

---

## Right / Wrong

Surfacing a failure from a command:

```rust
// Wrong — ad-hoc string to stdout, hand-picked exit code, no envelope,
// no stable code for machine callers.
if id_invalid {
    println!("error: bad id");
    std::process::exit(1);
}

// Right — construct a ProtocolError with a registered CanonicalCode; the
// single mapping projects it onto exit code + JSON envelope.
if id_invalid {
    return Err(ProtocolError::new(
        CanonicalCode::InvalidRequest,
        "id is not a valid entity id",
    ));
}
```

Preserving error provenance through `AppError`:

```rust
// Wrong — collapse everything to a generic internal error, losing the
// category (a provider parse failure becomes exit 70 instead of 7).
let out = app.run(req).map_err(|_| {
    ProtocolError::new(CanonicalCode::Internal, "operation failed")
})?;

// Right — let From<AppError> fan out by provenance so Domain/Port/Provider
// each map to their correct canonical code.
let out = app.run(req)?; // AppError -> ProtocolError via From, category intact
```

---

## Common Mistakes

- **Bypassing the catalog.** Do not print an ad-hoc error string or pick an exit
  code inline; construct a `ProtocolError` with a registered `CanonicalCode`.
- **Writing errors to stdout in human mode without the envelope**, or writing
  diagnostics to stdout in JSON mode — keep the stdout/stderr split.
- **Adding a code without updating the published schema** — the drift tests will
  (correctly) fail.
- **Marking a non-transient error retryable.** Only `writer_busy` /
  `source_changed` are retryable.
