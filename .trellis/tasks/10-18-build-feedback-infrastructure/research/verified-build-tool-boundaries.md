# Verified external constraints

Sources reviewed on2026-10-18; implementation has not measured speed gains.

- Cargo Profiles: https://doc.rust-lang.org/cargo/reference/profiles.html ; test inherits dev and selected profile applies to all targets. debug=1 limited is not line-tables-only. Preserve release profile.
- Cargo build performance: https://doc.rust-lang.org/cargo/guide/build-performance.html . Incremental benefits repeated edits in same target; one-shot disable must be separated from daily use.
- MBX1.24.1 managed targets: https://github.com/jdx/mr-boxington/blob/v1.24.1/docs/managed-targets.md . Explicit target-dir/CARGO_TARGET_DIR disables its original check lane; auto adoption can modify .git/info/exclude. This trial uses explicit targets and views off. Windows restores via copies, not reflinks; cached compiler time is not elapsed time.
- MBX1.24.1 config/scheduler: https://github.com/jdx/mr-boxington/blob/v1.24.1/crates/mbx/src/config.rs ; https://github.com/jdx/mr-boxington/blob/v1.24.1/docs/scheduling.md . MBX_GC_AUTO=0 disables auto collection. MBX_INCREMENTAL defaultfalse forces incremental off; set it intentionally per scenario. OnWindows memory measurement/pressure control is unavailable; external Job Object attribution required. Logical GC budget is not actual physical quota.
- sccache0.18 Rust: https://github.com/mozilla/sccache/blob/v0.18.0/src/compiler/rust.rs . -Cincremental bypasses; metadata/check/clippy paths exist, cwd hash may prevent workspace cross-path reuse. Cannot assumeSCCACHE_BASEDIRS fixes Rust.
- sccache local: https://github.com/mozilla/sccache/blob/v0.18.0/docs/Local.md ; https://github.com/mozilla/sccache/blob/v0.18.0/src/lru_disk_cache/mod.rs . One server per store, LRU permanently deletes; isolate and prevent threshold reaching.
- Windows allocation: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getcompressedfilesizew ; actual allocation accounts sparse/compressed; ordinary noncompressed result may be logical size, so FileStandardInfo AllocationSize/appropriate compression handle data must be reconciled. Dedup volume+FileIdInfo before aggregating.
- Windows jobs: https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information ; JobMemoryLimit is committed virtual memory limit and PeakJobMemoryUsed must not be labeled RSS. Job-limit failure is separate from elapsed performance.

Release asset availability verified using GitHubAPI for MBXv1.24.1 and sccachev0.18.0: x86_64-pc-windows-msvc zip plus SHA256 checksums. Do not setup or globally install; isolated prebuilt tools only after budget admission.


Additional official source verification during implementation:
- MBX cargo invocation reads CARGO for the actual executable: https://github.com/jdx/mr-boxington/blob/v1.24.1/crates/mbx/src/cli/cargo.rs . Windows global config resolves under APPDATA/mbx/config.toml; isolated child APPDATA keeps ambient config out: https://github.com/jdx/mr-boxington/blob/v1.24.1/docs/configuration.md . No global PATH edits.
- sccache local READ_ONLY mode suppresses cache puts: https://github.com/mozilla/sccache/blob/v0.18.0/docs/Local.md ; https://github.com/mozilla/sccache/blob/v0.18.0/src/cache/disk.rs . The chosen warm-store candidate pins READ_ONLY and a very high internal ceiling; no new cache action inserts during timing. This is an explicit tested variant, not general everyday RW cache qualification.
- QueryFullProcessImageName may fail for reasons other than exited processes; do not ignore error31 alone. Check terminal process status before skipping: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getexitcodeprocess . Unknown active identity still blocks capture.
