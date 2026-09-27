# Independent Relocation Storage Review

Date: 2026-09-27. Scope: SQLite relocation and application policy. Product/test files remain owned by the main implementer and test worker. The following findings are from direct current-source traces; the main session should add the proposed synthetic regressions before fixing.

## P1: Receipt replay ignores newly owned installations inside the mapped roots

- Source: `crates/agent-session-grep-adapters-sqlite/src/relocation.rs`, `applied_relocation` (currently lines 1081-1128), called before generation checking by `relocation_preview` and `apply_relocation`.
- The receipt verifies only namespaces and locations in its previous manifest. It never checks whether new current installations or unresolved indexed sources have appeared elsewhere under `manifest.from_key` or `manifest.to_key`.
- Counterexample: for a parent-grouped provider, relocate `old/group-a/source` to `new/group-a/source`; then successfully index a new installation at `old/group-b/source`. This does not overlap the retired `old/group-a` location. Repeating the original `old -> new` preview returns `unchanged`; replaying the still-live original token also returns `unchanged` before the generation mismatch. The new source remains at the old root despite the requested root mapping, and a fresh preview cannot be obtained for it through this command.
- Contract: design section 6 explicitly allows no-op only when ownership agrees and says changed ownership cannot reuse a receipt.
- Fix direction: validate the complete current ownership/source set under both selected roots before the receipt fast path. An old receipt must not suppress a changed scope, even when its own namespaces remain intact. Preserve no-op after unrelated catalog writes outside these roots.
- Regression: extend the generic multi-parent fixture with a newly committed sibling installation (and separately an unbound source / newly occupied target subtree); assert preview is not `unchanged` and the old apply token fails without a backup or generation change.

## P2: Expired destination alias passes preview but is permanently rejected on apply

- Source: `relocation.rs`, `prepare_relocation` (currently lines 834-850) and `apply_relocation_in_tx` (currently lines 1395-1400).
- Preview ignores a retired destination location after `retired_until_ms`, as intended. Phase 2 reads only `(namespace_id, state)` and rejects any different namespace even when the conflicting location is retired and expired.
- Counterexample: installation A relocates from root X to Y; after its alias expires, independently registered installation B previews a move from Z to X. Preview returns a valid plan. Apply successfully publishes the backup and creates its building intent, then fails with `relocation target location became occupied` forever.
- The existing alias-expiry test only reserves a new namespace for ordinary ingest at X; it does not exercise relocation into an expired alias.
- Fix direction: make transactional destination ownership validation use the same expiry policy as preview, with the injected clock/revalidated operation time. Expired historical aliases of another namespace must be replaceable without touching A's current Y ownership or IDs.
- Regression: use two independent same-provider fixtures, expire X's alias, then relocate B into X; assert all B claims move, A's current identity remains unchanged, and the operation advances generation exactly once.

## P2: A retired location blocks unrelated providers sharing that root

- Source: `relocation.rs`, `installation_for_source_commit` (currently lines 424-435).
- The retired-root query is global and omits provider identity. In contrast, registry ownership keys and namespace resolution are provider-scoped. Therefore moving provider A from a shared root retires only A in the registry but prevents future normal commits for provider B that remains current there.
- Counterexample: index two parent-grouped providers into different files under the same directory. Relocate A to another directory, leaving B's file and current namespace unchanged. Re-stage B at its current root: namespace resolution succeeds, but source activation fails with `source location is retired` because it matches A's retired root.
- Fix direction: constrain the retired-location check to the source's verified provider/installation assignment; retain fail-closed behavior when provenance is genuinely unknown.
- Regression: relocate only A, then append/re-sync B. Verify B preserves identity, commits normally and A's old location still cannot be scanned as A.

## Verification status

No product or test files modified. Full gates are owned by the main session. These three counterexamples still need executable Rust regression coverage by the test owner.

Scoped checks completed successfully:

- `cargo clippy -p agent-session-grep-adapters-sqlite -p agent-session-grep-application --lib --offline -- -D warnings` (pass).
- `cargo check -p agent-session-grep-adapters-sqlite -p agent-session-grep-application --lib --offline` (pass).
- `cargo test -p agent-session-grep-application --lib relocation::tests --offline` (21 passed, 0 failed).

The three source conditions above were rechecked after these commands and remain present. No additional blocker was identified in the examined frozen-legacy namespace checks, relocation's eight live locator updates, final snapshot revalidation, backup verification/no-clobber publication, or current-schema read-open path. This does not replace the main session's full workspace/release/failure-injection gates.
