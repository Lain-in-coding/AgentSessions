# Implement and check

1. Read parent decisions/review evidence and applicable provider/port contracts.
2. Add failing synthetic tests for missing/invalid session ownership and bounded exact accounting.
3. Make the smallest parser/accounting change; preserve existing shape/identity/cap rules.
4. Run `cargo --offline --locked test -p agent-session-grep-provider-hermes` and targeted clippy with `-D warnings`; format only this package.
5. Return changed files, test results, spec notes and any integration caveat to the coordinator. Do not commit/archive, edit shared specs, or spawn agents.
