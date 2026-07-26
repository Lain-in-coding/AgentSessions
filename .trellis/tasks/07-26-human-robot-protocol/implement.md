# Implement: Human/Robot protocol completion

Gates after each phase: fmt --check / clippy -D warnings / workspace tests.
Defender hazard: verify survival after every write.

- [x] S0 (main): pre-add `mod human;` + stub `human.rs`; freeze APIs in
      design §2-3; dispatch agents.
- [x] S1 (agent A): `human.rs` renderer per design §2, unit tests inline.
- [x] S2 (agent B): `protocol.rs` per design §3 + envelope.schema.json frame
      defs + mechanical emit call-site fixes in main.rs; unit +
      cross-validation tests.
- [x] S3 (main, after S1+S2): main.rs real wiring — --request-id, mode branch
      (human vs envelope), warnings from Context evidence, sync progress
      frames (jsonl only), render() 4-tuple; unit test for warning computation.
- [x] S4 (main): truth-table e2e per design §5; full gates + cargo deny;
      spec update; commit plan.

Rollback: S1 is an isolated new module; S2+S3 are one protocol-surface batch —
revert restores child-3 green state.
