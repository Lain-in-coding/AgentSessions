# R0 Closure Design

## Boundary

This task prepares the R0 governance package and fixes reproducible evidence drift. It does not self-approve Governance Records. Status transitions to `Accepted`, assignment of owner/approver, and product-scope decisions remain project-owner actions.

## Work streams

1. **Evidence integrity** — make every checked-in spike program reproduce its evidence or clearly record a known failure.
2. **Contract reconciliation** — remove contradictions among RFCs, ADRs, the CLI/Robot/MCP contract, schemas, maturity matrix, and implemented first slice.
3. **Decision packets** — for each open question, record a recommended option, alternatives, compatibility impact, validation required, and approval field.
4. **Gate accounting** — distinguish `implemented`, `locally verified`, `cross-platform configured`, `externally blocked`, and `Accepted`.

## Constraints

- Preserve historical evidence; correct it with explicit current-result sections rather than rewriting history without trace.
- Do not claim Linux/macOS validation from CI YAML alone.
- Do not mark private transcript hand-testing as reproducible promotion evidence.
- Do not change application behavior in this child task except where a spike must be repaired to match its own documented experiment.

## Validation strategy

- Run every spike's executable scenario, not only `cargo test`, because several spike crates have zero unit tests.
- Run workspace fmt, clippy, tests, and cargo-deny after tracked changes.
- Cross-check protocol runtime constants against schema and error catalog.

## Rollback

Each governance document and spike correction is a separate logical edit. A failed spike correction is reverted independently without changing governance status.
