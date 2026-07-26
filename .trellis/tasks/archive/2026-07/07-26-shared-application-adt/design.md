# Design: shared Application ADT (cursor / budget / branch / evidence)

Normative: CONTRACT-cli-robot-mcp-draft §1-3 §7. Grounded in code as of
2026-07-26 (children 1-2 landed: spans, session/document entities, v6).

## 0. Documented deviations from the Draft contract

1. **Cursor "signing"**: v1 uses a keyless BLAKE3 integrity digest over the
   token payload (tamper-evident, not authenticated). Rationale: local-first
   single-user surface; no key-management/storage exists and inventing one
   here would force a v7 schema migration unrelated to ADT semantics. The
   digest field is versioned inside the token so a keyed scheme can replace
   it without breaking the wire shape. Recorded here per PRD.
2. **line_start/line_end** in EvidenceSpan DTO: emitted only when derivable;
   v1 stores byte spans, so line fields are `null` with `precision: "byte"`
   (contract allows explicit degradation; we degrade the *line* fields, not
   the whole span).

## 1. Module layout (parallel-agent file ownership)

```
crates/agentsessions-domain/src/thread.rs      [agent B]  branch selection
crates/agentsessions-application/src/cursor.rs [agent A]  CursorToken + errors
crates/agentsessions-application/src/budget.rs [agent A]  ResponseBudget + truncation
crates/agentsessions-application/src/evidence.rs [agent C] EvidenceSpan DTO + assembly
crates/agentsessions-application/src/lib.rs    [main]     mod decls pre-added; ADT integration after A/C
crates/agentsessions-cli/src/main.rs,protocol.rs [main]   wiring + error codes
schemas/robot/v1/*                              [main]     catalog additions
```

Main session pre-writes the `mod`/`pub use` lines in application lib.rs and
domain lib.rs BEFORE dispatch so agents never touch shared files.

## 2. cursor.rs (agent A)

```rust
pub struct CursorToken { /* opaque; wire = base64url(json)+"."+digest16hex */ }
pub struct CursorClaims {
    pub contract_major: u32,        // 1
    pub generation: u64,
    pub issued_at_ms: i64,
    pub expires_at_ms: i64,         // issued + TTL (default 15 min, param)
    pub query_digest: String,       // blake3-16hex of canonicalized query
    pub sort_digest: String,        // constant per use case ("wire_id_asc"|"score_desc")
    pub offset: u64,                // resume position within the pinned ordering
}
pub enum CursorError { Invalid(String), Expired(String), GenerationMismatch{cursor:u64,active:u64}, ContractMismatch{cursor:u32,supported:u32} }
pub fn issue(claims) -> CursorToken;             // digest = blake3("as-cursor-v1" || json)
pub fn verify(token,&expect: {now_ms,active_generation,query_digest,sort_digest}) -> Result<CursorClaims,CursorError>;
```

Rules (unit-tested): bad base64/json/digest → Invalid; now ≥ expires →
Expired; claims.generation ≠ active → GenerationMismatch (message tells the
caller to restart the query); contract_major ≠ supported → ContractMismatch;
query/sort digest mismatch → Invalid (a cursor from another query never
silently reused). Time always injected.

## 3. budget.rs (agent A)

```rust
pub struct ResponseBudget { pub max_response_bytes: usize, pub max_items: usize,
    pub max_snippet_chars: usize, pub max_messages: usize, pub max_evidence_spans: usize }
impl Default // generous defaults; CLI flags override
pub enum BudgetError { TooSmall(String) }        // < floor constants → invalid_request upstream
pub struct Truncation { pub truncated: bool, pub reason: Option<String> } // "max_items"|"max_response_bytes"|...
pub fn clamp_items<T>(items: Vec<T>, budget, est_bytes: impl Fn(&T)->usize) -> (Vec<T>, Truncation, consumed: usize)
```

Sort-before-truncate is the CALLER's obligation (documented); clamp applies
max_items then the byte gate greedily in order. Validation floor: enough for
envelope + 1 item (constants with tests).

## 4. domain thread.rs (agent B — pure, no I/O)

```rust
pub enum ContextPolicy { Mainline, Full }        // serde snake_case
pub struct BranchSelection<'a> { pub leaf: &'a Message, pub messages: Vec<&'a Message> }
pub fn select_mainline<'a>(msgs: &'a [Message]) -> BranchSelection<'a>;
pub fn select_full<'a>(msgs: &'a [Message]) -> Vec<&'a Message>;      // seq order
```

Mainline rule (deterministic, documented in rustdoc): candidates = non-
sidechain messages; leaf = highest seq candidate; walk `parent` links to
root collecting the chain (parent lookups by id map); messages missing from
the map end the walk (cross-file parents tolerated); result ordered
root→leaf. Forks: siblings not on the walked chain are excluded. If ALL
messages are sidechain, fall back to highest-seq message overall (honest
edge case, tested). Full = all messages by seq. Property-style unit tests:
chain, fork (edited retry), sidechain interleave, all-sidechain, orphan
parent.

## 5. evidence.rs (agent C)

```rust
pub struct EvidenceSpanDto {   // serde snake_case, versioned by contract major
  pub occurrence_id: String,   // deterministic: blake3-16hex(message_wire||ordinal)
  pub message_id: String, pub source_document_id: Option<String>,
  pub generation: u64, pub source_fingerprint: Option<String>,
  pub byte_start: Option<u64>, pub byte_end: Option<u64>,
  pub line_start: Option<u32>, pub line_end: Option<u32>,
  pub record_ordinal: Option<u32>,
  pub snippet_char_start: Option<u32>, pub snippet_char_end: Option<u32>,
  pub precision: Precision }   // Byte|Line|Record|Unknown, snake_case
pub fn assemble(message_wire:&str, payload:&[u8], document_payload: Option<&[u8]>, generation:u64, seq_hint: Option<u32>) -> EvidenceSpanDto
```

Reads the canonical message JSON (fields `span{start,end}`, `session`) and
document JSON (`fingerprint`); span present → precision Byte with byte
range; absent → precision Unknown with all location fields null. Never
invents values; no absolute paths. Unit tests: post-v6 payload, legacy
payload (no span), non-JSON payload (Unknown), fingerprint passthrough.

## 6. Integration (main session, after A/B/C)

- `AppRequest::Search { query, limit, cursor: Option<String>, budget: ResponseBudget }`
  (limit folded into budget.max_items when smaller), `List` likewise,
  `Context { session_id: StableId, policy: ContextPolicy, budget }`.
- `AppResponse::Search { hits, next_cursor, generation, truncation }`,
  `List { entries, next_cursor, generation, truncation }`,
  `Context { session: Value-ish struct, branch_leaf: String, messages: Vec<(id,payload)>, evidence: Vec<EvidenceSpanDto>, truncation, generation }`.
- Pagination model: offset-based within a pinned ordering (catalog list =
  wire-id ASC via existing stable sort; search = score DESC then id — needs
  deterministic tiebreak; store query already orders by bm25, add id
  tiebreak in adapter query ORDER BY). Cursor claims carry offset; the
  store re-executes the query at the SAME generation or the cursor errors
  (generation mismatch) — consistent with the no-state contract model.
- CLI: `--cursor <token>`, `--max-items`, `--max-bytes` flags; new `context
  <ses_v1_...> [--policy mainline|full]` command; `page{next_cursor,
  has_more}` filled from ADT result; outcome partial when truncated
  (exit 10 per catalog — verify existing partial mapping).
- protocol.rs + schemas/robot/v1 error catalog: add `cursor_invalid`,
  `cursor_expired`, `generation_mismatch` (+ exit/robot mappings per
  contract §5); include_str! cross-validation updated.

## 7. Test plan

Unit per module (agents) + integration (main): cursor round-trip via CLI
(search page1 → next_cursor → page2, disjoint union == unpaged), tamper/
expiry/generation-bump e2e (ingest again to bump generation → old cursor
errors), context over real-format fixture (branch + evidence + budget
truncation), list pagination stability.
