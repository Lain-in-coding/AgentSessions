# Final candidate quality review

Date: 2026-10-15. Status: **Phase 2.2 passed; no pre-freeze blockers.**
Static review, local quality gates and the strict differential oracle passed.
Formal three-run performance acceptance remains separate and pending.

## Reviewed identity

- Base: `de2c95d2c93f7453ce5b5ccc169614d3aa7892ee`.
- Clean frozen S2: `e8e4975817f71647c158f6d802a7b98cee2f1b15`.
- S2 tree: `f1a99f12e093fd669958102a23e470c37a2ad784`.
- O6 source commit: `b8578b2d213b8153f1d4a6778c1daf0598c53128`, followed
  by S2's invariant-test/comment commit.
- Current adapter `src/lib.rs` Git blob:
  `629d5b2c5de35782fb3f816bb53fcd65482d517a`.
- Current file SHA-256:
  `2ec45e89d3c10e58690d60a1816b62779bd4263624893b4388530fb39f9c97a1`.
- Full base-to-working-tree diff SHA-256:
  `6efca04683c81e233ca2f0c2cb10ee49a43fda18310fd858cdbd043ad29a2559`
  (40,098 bytes). The receipt captured the working tree before atomic commits;
  S2's committed adapter blob now matches the tested blob exactly. The original
  pre-commit receipt has not been overwritten.
- The sole changed product file is
  `crates/agent-session-grep-adapters-sqlite/src/lib.rs`.
- Final scope is O1/O2/O4/O6. O3 and O5 were rejected and are absent.
  Schema remains 19; no dependency, provider, public API, maintenance,
  harness, CI, or generated platform asset changes are present.
- Harness SHA-256 remains
  `aeb23f909de805f507f45c1c61a20cf940a111ee962a0c6d6cd4ae37a49f39f8`.

Complete private receipts are under the task runtime's `final-quality/`:
`candidate-identity.json` and `candidate-full.diff`. Raw test logs remain there
and are not product/publication artifacts.

## Findings (fixed)

### F1 — Durable O6 compatibility lacked a typed regression fixture

- File: adapter `src/lib.rs`, existing `cfg(test)` module.
- Issue: the initial helper test checked only an already-ordered `Vec<Value>`;
  it did not cover the changed typed sort path or operation digest. The 50k
  benchmark also leaves Activity/Usage data empty.
- Fix: added `typed_manifest_streaming_preserves_legacy_json_and_digest`.
  Mixed Placement/Edge/Activity/Usage upserts and deletes, source membership,
  source replacements, resume claims and installation assignment are compared
  against the pre-O6 collected-array algorithm. Permutations, empty relations,
  nulls, Unicode and escaped strings are included. Original length-framed
  digest input is assembled independently of `batch_manifest`/`hash_field`.
- Fix: added `streamed_manifest_rejects_invalid_relations_before_durable_intent`.
  Duplicate relation upserts/deletes, upsert/delete overlap, duplicate source
  replacements and invalid membership IDs retain exact errors and leave the
  outbox, generation, catalog, indexes, relations and source tables unchanged.
- Verification: both new tests passed within the final 304-unit-test run.

### F2 — Chunk-boundary assertions omitted edge tails and source claims

- File: adapter `src/lib.rs`,
  `bulk_commit_spanning_chunk_boundary_lands_every_row`.
- Issue: 513 messages yielded only 512 edges, and assertions did not inspect
  the two per-source membership tables. Standard 250x200 benchmark sources
  do not cross the new 512-row per-source boundary.
- Fix: use 514 messages / 513 edges; require exact entity/document memberships
  and placement-claim sets, plus full table counts.
- Verification: strengthened test passed in the final adapter suite.

### F3 — Identity-tamper regression did not prove complete state preservation

- File: adapter `src/lib.rs`,
  `tampered_identity_sidecar_fails_closed_without_state_change`.
- Issue: the earlier assertions checked generation and new-source membership
  but did not cover the same-source no-op entry or outbox/FTS side effects.
- Fix: snapshot every SQLite table, including FTS shadow tables. First prove
  that a valid unchanged request preserves the exact snapshot; after deliberate
  identity-sidecar tampering, test both unchanged same-source and changed
  new-source requests and require no further table changes.
- Verification: strengthened test passed in the final adapter suite.

### F4 — Stale bulk-size comments

- File: adapter `src/lib.rs`, bulk-write comments.
- Issue: comments still described 100-row chunks and old parameter counts.
- Fix by main: update to 512 rows and 4096/2048/1536 bind variables under the
  bundled 32766-variable ceiling. No executable production behavior changed.

The reviewer initially had read-only product access, then explicit ownership
of only `cfg(test)` additions/adjustments. All reviewer code edits stayed in
that module. The main agent owned the production optimization and comment
updates; neither agent's changes were reverted.

## Findings (not fixed)

No unresolved product-code correctness defect or Phase 2.2 blocker remains.
The 23 existing ignored Rust helpers, limited synthetic oracle coverage and
unexecuted cursor continuation page are disclosure items, not newly skipped
acceptance tests. Formal validators, R4 arithmetic and publication/hosted gates
remain later work. This review does not claim performance acceptance.

## Static correctness conclusions

- **O1:** touched Message IDs are exactly the union of the former per-ID
  lookups. Ordinary catalog/relation writes still occur after the chunked
  queries. Old placement owners, old source claims and resume sessions remain
  collected, and new placement sessions are inserted directly. Empty input
  produces no message lookup.
- **O2:** all source entries retain identity-sidecar validation before the
  early no-op. The merged map contains only those entries, cross-source
  StableId conflicts are rejected, and stored merging changes payload/text
  rather than identity. No intervening catalog/sidecar write invalidates the
  proof that the removed second sweep was redundant.
- **O4:** all eight column-count assertions fit the bundled ceiling; the
  full 512x8 prepare/execute smoke test passes. Per-row iteration, transaction,
  CAS and durable-intent behavior remain intact. The conservative 100-row
  constant is a manual build-time option, not an automatic runtime fallback.
- **O6:** validation precedes sorting and serialization. Unique pure canonical
  keys make cached sorting order-equivalent. Canonical value builders and
  digest framing/domains are unchanged; writing one value at a time preserves
  the original compact JSON bytes. The typed regression now tests that claim.
  This is value streaming plus cached sorting, not streaming hashing: final
  JSON strings are still retained and subsequently hashed.

### Physical FTS rowids and equality scope

FTS rowid allocation starts after old rows in the first message-containing
chunk are deleted. A 200-message full rewrite can therefore allocate from 201
with 100-row chunks and from 1 with 512-row chunks. Existing tests already
treat these rowids as internal physical addresses. Search breaks BM25 ties by
canonical wire ID, and sidecar mappings must remain correct.

Main retained O4 and explicitly narrowed the cross-binary FTS comparison to
wire/typed identity/content join equivalence, with a bijective, orphan-free
sidecar check. The completed 200-message rewrite shows exactly 200 physical
message-FTS rowid differences, with matching complete joins, postings, query
ordering/scores and retrieval. Same-side no-op comparisons remain exact.
Do not generalize a
specific corpus's raw-dump match to all physical fields on every workload,
collapse namespaces, or silently weaken original operation-digest checks.

## Verification

| Check | Final result | Private log |
| --- | --- | --- |
| `cargo fmt --all --check` | Pass, exit 0 | `fmt-check-02.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass, exit 0; includes type checking | `clippy-02.log` |
| `cargo test -p agent-session-grep-adapters-sqlite` | Pass, exit 0; 340 passed, 0 failed, 0 ignored | `adapter-tests-02.log` |
| Typed manifest focused replay | Pass, 1 passed | `typed-manifest-02.log` |
| Full workspace tests | Pass, exit 0; 2012 passed, 0 failed, 23 existing ignored | `workspace-tests-01.log` |
| Evidence helper suite | Pass, exit 0; 143 tests, no skips | `evidence-tests-01.log` |
| Governance helper suite | Pass, exit 0; 93 tests, no skips | `governance-tests-01.log` |

The reviewer independently summed 95 Rust suite summaries and counted 23
ignored test lines. Every ignore attribute matches the base by path/content:
20 manual fixture-regeneration helpers, 2 raw-timing helpers and 1 enlarged
maintenance-evidence measurement. Both Python suites end in plain `OK`, with
no skipped summary. The full-gate receipt records three zero exit codes and
the unchanged tested source SHA-256. No suites were rerun during this audit.

The full adapter run includes 304 unit tests, 3 failure-semantics tests,
2 projection-consistency tests, 13 journal-retention tests, 14 maintenance-queue
tests and 4 process-evidence tests. Helper-binary and documentation targets
contain zero tests. Targeted coverage is included in these counts, not added
again to inflate the total.

First-attempt disclosure: `adapter-tests-01.log` has 303 unit passes and one
failure in the new typed fixture. Its hand-written legacy namespace seed was
rejected by the existing provenance guard before JSON/digest comparison.
The fixture now calls the real `legacy_installation_namespace` derivation;
the production guard was not weakened. A focused replay and then the complete
adapter suite passed. Both attempts and both lint rounds are retained.

## Strict oracle: completed independent audit

Final verifier SHA-256:
`74019400d458a7d8bd97ba13b1a39fae33504b8b6b25d845016fc240c3fcf9e6`.
Final `strict-oracle-01/recompare-01/result.json` SHA-256:
`4b9a3cf97959e822178605248c9c359dc8f1a87da2580a818d9f8ff9c676973d`.
Frame-normalization receipt SHA-256:
`13edc085f2a103ed6565f97478817954ff5f00bbda994e6a2427a9a7f3198e61`.
These hashes were independently recomputed and matched the worker's pins.

Independent receipt/table comparisons confirm all three cases have zero
findings: 18 real sync phases, 26 user tables each, 12 original digests matching
their independently reconstructed preimages and 6 exact same-side raw no-ops.
Each side has 15 distinct invocation mappings. All 24 state/proof controls and
15 frame/cursor controls individually pass and match their recorded counts.

The normalization rules were read directly. Only four named SQLite accounting
columns and the validated installation assignment's creation time may vary;
NULL remains distinct from an integer. Namespace IDs/seeds have bijective
provider/root/source anchors, and operation IDs map actual generation pairs.
Original digests must match retained exact pre-commit inputs before a
normalized digest is admitted. Historical rows and preimage archives must
remain unchanged. No arbitrary payload/domain-time replacement is allowed.

Only the ten named FTS5 shadow tables are omitted across sides; their schema,
counts and original hashes remain in raw snapshots. Every declared user
column remains compared. The additionally observed implicit ordinary-table
rowid is physical metadata omitted across sides and retained for raw no-op
checks. FTS addresses require complete identity/content/bijection validation.

Public frames map only `request_id` and `meta.duration_ms` at explicit paths.
A cursor first passes original checksum, independent query binding, generation,
ordering, offset, exact 900000 ms TTL and invocation-clock checks. Original
tokens and absolute times remain in receipts. Re-signed invalid cursor claims
are rejected by controls; arbitrary domain `*_ms` fields stay compared.

AST comparison confirmed all 13 reviewed data/snapshot/proof components are
unchanged between the retained run-02 verifier and final verifier. The offline
recomparison adds invocation metadata handling without weakening the executed
database/digest checks or invoking a product CLI again.

Failed originals remain present. The first shrink proof used post-commit
payloads; retained original payloads and a restricted pre-commit alias union
now reproduce the raw digest. Run-02 then stopped on varying request/duration/
cursor fields. Explicit validated mappings recompare those retained frames
in a new directory. Preparation failures, both failed runs and the reproof
are preserved. Tool activity, usage and relocation tables are empty in these
corpora; typed manifest tests and existing suites cover adjacent contracts.
Cursor continuation was not executed by this supplementary oracle.

## Formal driver and R4 comparator audit

`compare_r4.py` SHA-256:
`4811fa6a4034f9f46824bf6c8ae7514e5e7047b01914e19078fadb345e6854b3`.
`run-formal.ps1` SHA-256:
`73a4f26856f9850a30f6a1f9fd4d007e4fef102e7bd2b6528d8dac8a9ee69e34`.

The comparator uses Decimal raw observations, verifies rederived P50 values
against report summaries, and requires 48 primary plus 8 legacy series with
matching inventories across three runs. Target improvements use strict
`< 0.85 * baseline`; other series use `<= 1.15 * baseline`. Both
`primary_r4_pass` and `legacy_guard_pass` must pass for all latency guards.
Official validator success is not R4.

The driver requires clean S2 and unchanged harness bytes, a new suite-owned
Cargo target, no build-directory/target/jobs overrides, an uncached first-run
receipt, serial runs 1..3 and matching receipt/binary/source identities. It
adds successful three-phase trace and shared-deadline checks, retains failures
without auto-retry, restores the previous target environment, and checks that
source commit/tree stay unchanged. No blocking defect was found. Python
assertions were enabled in the reviewed launch environment.

## Performance and final acceptance boundary

Main reports O6 E3 retained, with manifest savings of 193.460 / 209.8635 ms,
wall medians of 8162.855 / 7585.289 ms, and 16 successful phase census checks.
The reported E3 binary SHA-256 is
`f74bffc939d0a0a81f933a09bfdd9d6aa58b3c3c46aa6464df9416d48ad283ef`.
Those measurements have not yet been independently audited here and do not
replace G4. O5 rejection remains consistent with its predeclared stage gate;
its invalid cache-contaminated first attempt is disclosed in research section 8.

R4 uses unrounded thresholds: 50k initial must be below **8604.462790 ms** and
shrink below **8451.728755 ms**, comparing the three-run P50 medians. Every
other required series must also stay within its allowed regression bound.
`validate-report`/`validate-baseline` success alone does not establish R4 or
the separate trace check. Future audit must bind these results to the final
clean source identity and examine all original samples and failed attempts.

No schema migration or generated-template/detection update is required. Main
synchronized the SQLite changed-sync spec with the actual proof obligations,
manual limit fallback, typed tests, rowid comparison and formal constraints.
The added spec text was reviewed; this reviewer made no spec edit.

Formal suite launched by main after the gate:
`evidence-output/opt50k-e8e4975817f7-20261015T121711`.
The reviewer has stopped commands and awaits three runs plus trace before
auditing actual R4 results. No performance-success conclusion is made yet.
