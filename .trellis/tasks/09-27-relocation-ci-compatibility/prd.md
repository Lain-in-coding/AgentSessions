# Relocation CI compatibility

## Goal

Restore the explicit relocation identity PR's hosted CI while preserving its SQLite storage, binary, and fail-closed identity contracts. Correct the reproduced legacy source-lookup bypass without introducing new relocation capabilities.

## Requirements

- Replace the `chunks_exact(4)` conversion in `bytes_to_f32_vec` with an equivalent fixed-size slice conversion accepted by current Clippy.
- Preserve little-endian decoding and discard trailing 1–3 bytes.
- Preserve Rust 1.90 MSRV and all existing schema/API/cursor behavior.
- Use the official rusqlite 0.40.2/libsqlite3-sys 0.38.2 compatibility patch to restore actual Rust 1.90 compilation, retaining bundled SQLite 3.53.2 and existing dependency/security policy.
- Add regression coverage for complete, empty, and partial-tail byte sequences.
- Do not suppress the Clippy lint or change CI policy.
- Keep unrelated review artifacts out of this task and PR #12.
- Correct hosted CLI test fixtures and assertions when platform path representations disagree, while preserving legacy native-session proof, first-run preview, and the original working-directory contract.
- Do not relax identity validation, introduce new relocation behavior, skip failing coverage, or rerun failed CI as a substitute for a root-cause fix.
- Before allocating or reusing a namespace for an unbound legacy source, reject equivalent absolute path spellings that would bypass its existing scan/proof. Preserve exact historical locator text and all catalog state on refusal; registered aliases must retain their existing behavior.

## Acceptance Criteria

- [x] The changed SQLite crate passes fmt, Clippy with `-D warnings`, and its tests offline.
- [x] The final repaired tree passes workspace debug/release and semantic-candle checks locally.
- [x] Rust 1.90 compiles every workspace target and feature with the selected lockfile on Windows, and the SQLite BLOB regression passes on that toolchain.
- [ ] The hosted Ubuntu, Windows, and macOS generic CI jobs pass on PR #12 after the Clippy fix and any directly related cross-platform test corrections.
- [x] Raw Windows legacy locators and equivalent input spellings cannot allocate a new namespace or advance generation; ingest, sync, and discovery share the protection.
- [x] The regression test proves little-endian round-trip and drops every partial-tail length from 1 through 3 bytes.
- [x] `git diff --check` passes and the staged commit contains only the allowlisted source, tests, applicable specs, and task artifacts.

## Notes

- The source failure is `clippy::chunks_exact_to_as_chunks` at `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304`.
- This task updates the existing `fix/session-relocation-identity` PR branch through a fast-forward refspec; it does not force-push or create a second product PR.
