# Hosted Clippy blocker evidence

Observed on PR #12 (`fix/session-relocation-identity`) on 2026-09-27:

- Ubuntu, Windows, and macOS generic `ci` jobs fail during Clippy with `-D warnings`.
- The diagnostic is `clippy::chunks_exact_to_as_chunks` at `crates/agent-session-grep-adapters-sqlite/src/lib.rs:8304`, where `bytes_to_f32_vec` uses `chunks_exact(4)`.
- Local Rust 1.97.1 does not emit the lint; hosted moving stable/Clippy 1.98 does.
- Installer smoke, core-beta evidence, cargo-deny, and security audit passed on the same run.
- The source fix must use the equivalent fixed-size slice conversion and must not suppress the lint.

## Second hosted run (verified 2026-09-28)

- PR head: `805735cfac88dc7c3330880e0ab3454f831ac63d`; workflow run `36335148416`.
- Ubuntu generic job `108664472865`: 143 CLI e2e cases passed; `migrated_v6_catalog_stays_readable_until_complete_reingest_enables_context` failed at the re-ingest assertion with `legacy source must explicitly prove every native session before identity registration`.
- macOS generic job `108664472791`: 142 CLI e2e cases passed; the same legacy case failed, plus `resume_yes_first_run_forced_preview_then_spawns_in_original_cwd` compared `/private/var/.../original-workspace` with `/var/.../original-workspace` as unequal strings. The spawned process did run; the failed assertion concerns path representation.
- Clippy passed before these tests. Windows generic and the three installer smoke jobs passed in this run.
- Complete logs were retrieved read-only through GitHub after an intermittent download EOF. Raw logs stay in local temporary storage; this record omits runner paths, transient request IDs, and signed download URLs.
- Required checks are not green; PR #12 remains open and must not be merged yet.
