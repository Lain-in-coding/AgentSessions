# Implement and check

1. Read the current Domain spec and parent reviewed choices; the task intentionally supersedes the invalid-time pairwise fallback.
2. Add failing tests for the exact three-message permutation counterexample and mixed timestamp classes.
3. Introduce the smallest private total-order key/comparator adjustment; do not change branch topology logic or timestamp storage.
4. Run `cargo --offline --locked test -p agent-session-grep-domain` and targeted clippy with `-D warnings`; format only the Domain package. Check Rust 1.90 compatibility.
5. Return changed files, exact verification, and spec notes. Do not edit shared specs, other packages, task lifecycle, git commits, or spawn subagents.
