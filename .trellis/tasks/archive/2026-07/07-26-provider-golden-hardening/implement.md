# Implement: provider golden fixtures + property hardening

Validation after each phase: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`. Defender hazard: verify file survival after writes.

- [ ] P0 (main): `.gitattributes` with `-text` rules for golden paths;
      commit-independent, lands before fixtures.
- [ ] P1 (agent claude-hardening): provider-claude `tests/golden/` fixture +
      expected JSON + PROVENANCE.md; `tests/golden.rs`; `tests/properties.rs`
      per design (seeded xorshift, 6 properties, ≥64 iters).
- [ ] P2 (agent codex-hardening): provider-codex same structure; mirror
      duplication property is the codex-specific核心.
- [ ] P3 (main): review both provenance manifests against
      `docs/security/FIXTURE-REDACTION-POLICY.md`; update
      `docs/product/PROVIDER-MATURITY-MATRIX.md` with met gates + evidence.
- [ ] P4 (main): full gates + `cargo deny check` (expect no change);
      commit plan per repo convention.

Rollback: each phase is an independent commit batch; fixtures/tests are
additive — reverting any single commit restores previous green state.
