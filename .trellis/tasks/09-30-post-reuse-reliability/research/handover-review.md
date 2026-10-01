# Review evidence and limits (2026-09-30)

Baseline: `bbb7c79533c13db584fa5ca2e1e05990d990c8da`. All reproductions used synthetic inputs and a current debug CLI built with `cargo --offline --locked build -p agent-session-grep-cli`. Original product files were unchanged during review.

## Confirmed findings
- F1/P1: CLI lib.rs:4656-4660 applies jsonl_health to any changed cached source; jsonl_health at 4453-4486 parses every line as JSON. A valid Hermes SQLite message update returned success, retained=1, generation stayed 1, and search found no new text. Whole-source routing, not the Hermes parser, caused the stale index.
- F2/P1: Hermes sqlite_state.rs:660-665 diagnoses orphan messages without accounting them in skipped; CLI lib.rs:4353 consequently declares the source complete. Initial ingest of one message, deleting only its session row, then ingest again returned committed=0/skipped=0, advanced generation 1->2 and removed the hit even though the message row remained in the source.
- F3/P2: A standalone file first synced at zero bytes persisted installation provider `empty`, while source_scans.provider_id stayed NULL. Filling it with valid Claude JSONL then failed invalid_request. An existing valid source cleared and re-ingested also failed through the same variant-to-provider confusion (CLI lib.rs:3277,4395-4399,4731-4738; relocation.rs:330-334).
- F4/P2: Domain thread.rs:378-388 mixes UTC and raw ordering depending on the compared pair. The valid timestamps `2026-01-01T00:00:00+10:00`, `2025-12-31T20:00:00Z` and invalid `2026`, with fixed IDs/ordinals but six placement permutations, yielded three full orders and three leaves.

## Important negative controls and accepted choices
- Fresh nonempty source -> clear -> repeat sync -> restore passed with generations 1/2/2/3, both canonical and standalone paths, and all four native-session/native-message presence combinations. The archived performance report's generic repeat-empty failure was NOT reproduced; do not treat it as a current generic defect or a throughput regression.
- A first-empty canonical Claude-root source worked because the root supplied real provider ownership. Standalone initial-empty input is the proven failing case.
- Existing Hermes partial semantics were demonstrated using two valid messages then making one role invalid: committed=1/skipped=1, generation 1->2, search returned old second message plus two document-scoped copies of the unchanged valid message. Owner accepted temporary coexistence, provided it is explained and complete recovery converges.
- Domain baseline tests: 67 passed. Embedded Web client baseline: 11 passed. These green suites did not cover the new counterexamples. Earlier remote checks are historical evidence only.
- Initial parallel review agents were rate-limited. A resumed storage reviewer identified the empty-ingest/first-empty candidates; the coordinator reproduced them. Independent implementation checks remain required.

## Preserved handover
All six previous phase tasks remain archived/completed; PRs 13-16 merged. Main and PR16 checks were independently read back successful, and repository visibility was PRIVATE. The stale relocation runtime pointer is not this task. Root worktree's unrelated local changes remain outside scope. Prior 1M paired improvement remains approximately 37.5%, not >=50%; provider variants remain experimental.

## Technical reference
Rust standard library Ord contract: https://doc.rust-lang.org/std/cmp/trait.Ord.html . Context7 `/rust-lang/rust` slice/RELEASES docs independently confirmed comparator consistency requirements. No private code was sent to documentation/search services.
