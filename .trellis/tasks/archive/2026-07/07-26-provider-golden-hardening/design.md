# Design: provider golden fixtures + property hardening

## Layout (per provider crate, mirrored)

```
crates/agentsessions-provider-{claude,codex}/
  tests/
    golden.rs            # golden + contract integration test
    properties.rs        # deterministic property suite
    golden/
      basic.jsonl        # synthetic fixture (bytes are authoritative)
      basic.expected.json# pinned canonical output
      PROVENANCE.md      # how constructed, format knowledge encoded, policy ref
```

## Byte-exactness

Root `.gitattributes` (new, main-session owned):

```
crates/*/tests/golden/*.jsonl -text
crates/*/tests/golden/*.expected.json -text
```

`-text` disables eol conversion regardless of core.autocrlf. Golden tests
additionally assert a fixture fingerprint (BLAKE3 hex of the on-disk bytes,
pinned in the expected JSON) so any silent byte drift fails loudly with a
clear message instead of a confusing span mismatch.

## Golden expected-output shape

`basic.expected.json`:

```json
{
  "fixture_blake3": "<hex>",
  "session_native_id": "...|null",
  "committed": N, "skipped": N,
  "messages": [
    {"seq":0,"native_id":"...","parent_native_id":null,"role":"user",
     "text":"...","timestamp":"...|null","is_sidechain":false,
     "span":{"start":0,"end":123}}
  ]
}
```

Test: read fixture bytes → assert blake3 matches → parse via adapter with a
collecting sink → serialize collected events to the same shape → compare to
expected JSON **structurally** (serde_json::Value equality). Also assert
span slices: `bytes[span] == the exact source record line`.

Fixture content coverage (claude): sessionId, parentUuid chain, isSidechain
true, tool-role record, non-conversational line (summary), broken JSON line
(skipped), CJK+emoji text, empty line. (codex): session_meta, response_item
messages, event_msg mirrors (ignored), non-message response_item, broken
line, CJK+emoji.

## Property suites (no new deps — repo idiom)

Test-local xorshift64* PRNG (~15 lines) with fixed seeds; each case prints
its seed on failure (`assert!(cond, "seed={seed} ...")`). Precedent: each
provider crate already keeps its own local CollectingSink helper.

Generator: build a transcript as Vec<Record> from a seeded RNG —
valid conversational records (random roles, Unicode text pools incl. CJK/
emoji/RTL/control-adjacent, random large field up to ~256 KiB in one case),
malformed lines, non-conversational records, (claude) random parentUuid
links + sidechain flags, (codex) mirrored event_msg for a random subset of
authoritative messages. Render to bytes with mixed LF/CRLF line endings.

Properties asserted per generated transcript (≥ 64 iterations per suite,
1 large-field iteration):

1. span round-trip: every emitted message's span slices bytes back to its
   exact source record (minus eol);
2. seq contiguous from 0; count == committed;
3. determinism: parse twice → identical collected events + report;
4. (codex) committed == number of authoritative response_item messages,
   regardless of mirror count/interleaving;
5. (claude) parent_native_id/timestamp/sidechain survive verbatim;
6. malformed lines only ever increment skipped/diagnostics — never abort a
   parse that has valid records (matches record_recoverable contract).

## Maturity matrix update

`docs/product/PROVIDER-MATURITY-MATRIX.md`: flip the golden/property gate
cells for both providers to met with evidence pointers (test paths); source
spans already met by child 1. Promotion wording follows the template row
criteria exactly — Beta only if every listed gate is met, else remain
Experimental with the残 blockers named.

## Parallelization

Two agents, disjoint file ownership (one per provider crate). Main session
owns: `.gitattributes`, matrix doc, provenance policy conformance review,
final gates. No shared Cargo.toml/lock edits (zero new dependencies).
