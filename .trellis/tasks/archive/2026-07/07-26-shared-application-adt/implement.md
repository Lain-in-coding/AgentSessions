# Implement: shared Application ADT

Gates after each phase: fmt --check / clippy -D warnings / workspace tests.
Defender hazard: verify survival after every write.

- [x] S0 (main): pre-add `mod`/`pub use` stubs — domain lib.rs (`mod thread`),
      application lib.rs (`mod cursor; mod budget; mod evidence;`) with empty
      module files so parallel agents never edit shared files.
- [x] S1 (agent A): application `cursor.rs` + `budget.rs` per design §2-3,
      unit tests inline.
- [x] S2 (agent B): domain `thread.rs` per design §4, unit tests inline.
- [x] S3 (agent C): application `evidence.rs` per design §5, unit tests.
- [x] S4 (main): ADT integration — AppRequest/AppResponse extension,
      App::handle wiring, sqlite search ORDER BY tiebreak, offset support.
- [x] S5 (main): CLI flags + `context` command + protocol codes +
      schemas/robot/v1 catalog + cross-validation tests.
- [x] S6 (main): e2e per design §7; full gates + cargo deny; spec update;
      commit plan.

Rollback: S1-S3 are additive modules (revert = drop files + stub lines);
S4-S6 single integration batch, revert restores children-1/2 green state.
