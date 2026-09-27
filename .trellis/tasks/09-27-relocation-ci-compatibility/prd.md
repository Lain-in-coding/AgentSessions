# Relocation CI compatibility

## Goal

Make the explicit relocation identity change compatible with the current hosted Clippy stable toolchain without changing the SQLite storage contract, binary format, or relocation behavior.

## Requirements

- Replace the `chunks_exact(4)` conversion in `bytes_to_f32_vec` with an equivalent fixed-size slice conversion accepted by current Clippy.
- Preserve little-endian decoding and discard trailing 1–3 bytes.
- Preserve Rust 1.90 MSRV and all existing schema/API/cursor behavior.
- Add regression coverage for complete, empty, and partial-tail byte sequences.
- Do not suppress the Clippy lint or change CI policy.
- Keep unrelated review artifacts out of this task and PR #12.

## Acceptance Criteria

- [x] The changed SQLite crate passes fmt, Clippy with `-D warnings`, and its tests offline.
- [x] The workspace debug/release and semantic-candle checks pass locally.
- [ ] The hosted Ubuntu, Windows, and macOS generic CI jobs pass on PR #12.
- [x] The regression test proves little-endian round-trip and drops every partial-tail length from 1 through 3 bytes.
- [x] `git diff --check` passes and the staged commit contains only the allowlisted source, tests, and task artifacts.

## Notes

- The source failure is `clippy::chunks_exact_to_as_chunks` at `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304`.
- This task updates the existing `fix/session-relocation-identity` PR branch through a fast-forward refspec; it does not force-push or create a second product PR.
