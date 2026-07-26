# PRD: Human/Robot protocol completion against the shared ADT

Parent: `07-24-advance-integration-beta` child 4. Normative source:
`docs/contracts/CONTRACT-cli-robot-mcp-draft.md` §4 (envelope/JSONL frames)
and §6 (output truth table). Prerequisite (landed): child 3 shared ADT
(cursor/budget/context, partial=exit 10, envelope `page`).

## Problem

The CLI's protocol surface is incomplete against the Draft contract:

1. Human mode still prints the compact JSON envelope ("degenerate" mode) —
   there is no human renderer.
2. JSONL mode emits only `response`/`error` frames; the contract requires the
   frame vocabulary to at least distinguish response/diagnostic/progress/error,
   with `--robot` forbidding progress frames.
3. `warnings[]` is always empty — no population path exists even where the
   system knows something worth surfacing (e.g. evidence spans degraded to
   `unknown` precision on legacy rows).
4. Error envelope `details` is always `{}` even for errors with typed fields
   (generation mismatch carries both generations).
5. Broken pipe: `println!` panics on EPIPE, polluting stderr and exiting 101 —
   violating the §6 truth table (stdout protocol-clean, controlled exit).
6. Robot callers cannot correlate requests: `request_id` is always generated
   internally; there is no `--request-id` passthrough.

## Requirements

- R1 Human renderer: `search`/`list`/`get`/`show`/`context`/`status`/`ingest`/
  `sync`/`index`/`doctor`/`config paths` produce human-readable text (no JSON
  envelope) in Human mode. Zero-result, partial/truncation, and pagination
  hints are explicit. No color (so `--no-color` stays honest); no new deps.
- R2 Frame vocabulary: `frame_type` supports `response`/`error`/`progress`/
  `diagnostic`. Progress frames are emitted ONLY in `--output jsonl` mode
  (never Json/`--robot`, never Human stdout). v1 emits per-source progress
  frames during `sync`.
- R3 Warnings plumbing: success envelope carries caller-supplied warnings;
  v1 populates one genuine source — `context` warns when evidence spans
  degraded to `unknown` precision. Human mode prints warnings to stderr.
- R4 Error details: cursor generation/contract mismatches populate bounded
  structured `details`; all other errors keep `{}`.
- R5 Broken pipe: protocol writes map EPIPE to a silent successful exit
  (stdout stays clean, no panic/backtrace).
- R6 `--request-id <id>` global flag: valid ids (envelope pattern
  `^[A-Za-z0-9._:-]+$`, 1-128 chars) are echoed in every frame; invalid ids
  are a usage error, not silently replaced.
- R7 Envelope schema stays 1.x-compatible: additions only (`progress`/
  `diagnostic` frames are NEW frame kinds in the schema); existing
  response/error shapes unchanged. Cross-validation tests updated.

## Acceptance criteria

- [ ] Human `search`/`status`/`context` output contains no `schema_version`/
      envelope braces; shows counts, truncation, and cursor hint; empty result
      states are worded, not blank.
- [ ] `--output jsonl sync <2 files>` emits progress frames + final response
      frame, each line a complete JSON object; same command under `--robot`
      emits exactly one response envelope and no progress.
- [ ] `context` warning on unknown-precision evidence is unit-verified at the
      projection layer; e2e verifies the warnings array is present (and `[]`)
      on a modern store, proving the plumbing end-to-end (see design §0.2).
- [ ] `generation_mismatch` error envelope carries
      `details: {cursor_generation, active_generation}`.
- [ ] `--request-id abc.123` echoes in the envelope; `--request-id "bad id"`
      exits 2 with `invalid_request`.
- [ ] Existing e2e suite stays green; new truth-table e2e covers the modes
      matrix (human/json/jsonl × success/partial/error/zero-result).
- [ ] Full gates: fmt / clippy -D warnings / workspace tests / cargo deny.

## Out of scope

- MCP transport (child 5); TUI (child 6).
- Color/TTY detection, spinners, interactive progress.
- New AppRequest/AppResponse variants or ADT semantics changes.
- Marking the CONTRACT draft Accepted (owner action, R0).
