# Hosted Clippy blocker evidence

Observed on PR #12 (`fix/session-relocation-identity`) on 2026-09-27:

- Ubuntu, Windows, and macOS generic `ci` jobs fail during Clippy with `-D warnings`.
- The diagnostic is `clippy::chunks_exact_to_as_chunks` at `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304`, where `bytes_to_f32_vec` uses `chunks_exact(4)`.
- Local Rust 1.97.1 does not emit the lint; hosted moving stable/Clippy 1.98 does.
- Installer smoke, core-beta evidence, cargo-deny, and security audit passed on the same run.
- The source fix must use the equivalent fixed-size slice conversion and must not suppress the lint.
