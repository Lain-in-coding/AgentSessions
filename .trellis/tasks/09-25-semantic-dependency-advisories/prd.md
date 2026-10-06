# Remove optional semantic dependency advisories

## Goal

Remove RUSTSEC-2024-0436 from the optional semantic stack through compatible upstream releases, preserving model behavior and audit policy.

## Requirements

- Track maintained replacements for `paste` in Candle/gemm/tokenizers and their transitive macro dependencies.
- Prefer released upstream fixes. Any vendored or forked replacement requires a separate reviewed compatibility and supply-chain design.
- Preserve Rust 1.90 support, offline inference, verified model bundles, and the existing zero-egress dependency policy.
- Do not add advisory ignores or remove the semantic feature to make audit output appear clean.

## Acceptance Criteria

- [ ] `cargo audit --file Cargo.lock` no longer reports RUSTSEC-2024-0436.
- [ ] `cargo tree --all-features -i paste` has no matching package and semantic-candle tests pass.
- [ ] Model manifest validation, tokenizer behavior and local inference are checked after any semantic-stack upgrade.
- [ ] Workspace checks and cargo-deny pass with the published MSRV.

## Notes

- Registered follow-up from `09-25-comprehensive-review-repairs`. Ratatui 0.30.2 / Crossterm 0.29 / lru 0.18.5 removed both lru unsound advisories and the default-build paste dependency.
- `cargo tree -i paste --all-features --offline` still identifies Candle 0.10.2 -> gemm 0.19 and tokenizers 0.22.2 (including macro_rules_attribute/pulp). The default feature tree has no paste.
- Verified on 2026-09-25: the [latest published Candle manifest, 0.11.0](https://docs.rs/crate/candle-core/0.11.0/source/Cargo.toml) still requires gemm 0.19.0 and tokenizers 0.22. A Candle minor upgrade alone does not resolve the warning.
- [RustSec RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html) is an unmaintained-package warning. The remaining warning is reported openly, not treated as removed.

## Rechecked on 2026-09-27

- Official crates.io metadata still identifies Candle Core 0.11.0 as the newest
  stable published version (published 2026-06-26; not yanked).
- Its normal, mandatory dependencies include `gemm ^0.19.0` and
  `tokenizers ^0.22.0`. The only non-yanked 0.19.x gemm release is 0.19.0,
  whose normal, mandatory dependency includes `paste ^1.0`.
- The current `cargo tree --all-features --offline -i paste` confirms multiple
  paths through gemm, gemm-common/pulp and tokenizers/macro_rules_attribute.
  A Candle-only version bump therefore cannot satisfy this task's acceptance
  criteria. No dependency version or audit policy was changed in this check.
- Evidence was read directly from the public crates.io API endpoints for
  `candle-core`, `candle-core/0.11.0/dependencies`, `gemm` and
  `gemm/0.19.0/dependencies`. This dated finding does not predict future releases.
- The task remains a released-upstream follow-up. A fork/vendor substitution
  still requires the separately reviewed compatibility/supply-chain design
  already specified above.
