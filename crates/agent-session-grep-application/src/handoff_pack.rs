//! Handoff Pack v1 generator (deterministic default, #4).
//!
//! Assembles a `HandoffPack` from search hits, session context, and catalog
//! data. The default generator is fully deterministic — no LLM calls. Evidence
//! (original text spans) and inference (derived summaries) are strictly
//! separated. Pack is preview-only: `asg` emits the pack and suggested
//! commands, never injects into another agent.
//!
//! Implementation reference: this is original work; the pack structure is
//! defined in `schemas/handoff/v1/pack.schema.json` and the Rust contract
//! types in `agent_session_grep_ports::handoff`.

use agent_session_grep_domain::StableId;
use agent_session_grep_ports::SearchHit;
use agent_session_grep_ports::handoff::{
    ConfidenceLevel, EvidenceEntry, HandoffBudget, HandoffFilters, HandoffPack, HandoffQuery,
    HandoffTarget, MainlineEntry, MatchedSession, PackConfidence, RedactionStatus, RetrievalMode,
    SessionConfidence, SourceLocator, TruncationReason, TruncationStatus,
};

/// 生成参数：搜索结果 + catalog generation + 预算。
pub struct HandoffInput<'a> {
    pub query_terms: &'a [String],
    pub retrieval_mode: RetrievalMode,
    pub filters: HandoffFilters,
    pub hits: &'a [SearchHit],
    pub catalog_generation: u64,
    pub max_tokens: u64,
    pub max_bytes: u64,
    pub max_evidence: usize,
    pub target: Option<HandoffTarget>,
}

/// 生成一个 deterministic handoff pack。
///
/// pack_id 由 generation + query + budget 派生（确定性）。
/// evidence 来自 search hits 的 text；inference 为空（deterministic 默认）。
/// 预算超限时显式记录 truncation 原因和被丢弃的 locator。
pub fn generate_deterministic(input: HandoffInput<'_>) -> HandoffPack {
    let pack_id = derive_pack_id(input.catalog_generation, input.query_terms);
    let created_at = utc_now_iso8601();

    // Matched sessions: 从 hits 的 session_id 去重，按首次出现顺序排列。
    let mut seen_sessions: Vec<String> = Vec::new();
    let mut matched_sessions: Vec<MatchedSession> = Vec::new();
    for hit in input.hits {
        let session_id = hit
            .session_id
            .clone()
            .unwrap_or_else(|| hit.id.as_str().to_string());
        if let Some(pos) = seen_sessions.iter().position(|s| s == &session_id) {
            matched_sessions[pos].occurrences += 1;
        } else {
            seen_sessions.push(session_id.clone());
            matched_sessions.push(MatchedSession {
                session_id: hit.id.clone(),
                provider_id: None,
                title: None,
                relevance_score: hit.score as f64,
                occurrences: 1,
            });
        }
    }

    // Evidence: 从 hits 的 text 提取（原文证据，不混入推断）。
    let mut evidence: Vec<EvidenceEntry> = Vec::new();
    let mut used_tokens: u64 = 0;
    let mut dropped_locators: Vec<SourceLocator> = Vec::new();
    let mut truncated = false;
    let mut truncation_reason = TruncationReason::None;

    for (idx, hit) in input.hits.iter().enumerate() {
        if evidence.len() >= input.max_evidence {
            truncated = true;
            truncation_reason = TruncationReason::MaxEvidence;
            for h in &input.hits[idx..] {
                dropped_locators.push(SourceLocator {
                    source_document_id: h.id.clone(),
                    cursor: None,
                });
            }
            break;
        }
        let text = hit.text.as_deref().unwrap_or("");
        if text.is_empty() {
            continue;
        }
        let text_bytes = text.len() as u64;
        if used_tokens.saturating_add(text_bytes) > input.max_tokens {
            truncated = true;
            truncation_reason = TruncationReason::BudgetExceeded;
            dropped_locators.push(SourceLocator {
                source_document_id: hit.id.clone(),
                cursor: None,
            });
            continue;
        }
        used_tokens = used_tokens.saturating_add(text_bytes);
        evidence.push(EvidenceEntry {
            message_id: hit.id.clone(),
            source_document_id: hit.id.clone(),
            session_id: hit
                .session_id
                .as_ref()
                .map(|s| StableId::from_wire(s).unwrap_or_else(|| hit.id.clone())),
            span_start: None,
            span_end: None,
            text: text.to_string(),
        });
    }

    // Mainline: 当前用 evidence 的 message_id 作为简化 mainline（完整 mainline
    // 展开需要 ContextGraphStore，由后续迭代补齐）。
    let mainline: Vec<MainlineEntry> = evidence
        .iter()
        .enumerate()
        .map(|(ord, ev)| MainlineEntry {
            message_id: ev.message_id.clone(),
            session_id: ev.session_id.clone(),
            role: "unknown".to_string(),
            ordinal: ord as u64,
            text_preview: Some(ev.text.chars().take(256).collect()),
            is_sidechain: false,
        })
        .collect();

    // Confidence: 有 evidence → medium；无 → low。
    let overall = if evidence.is_empty() {
        ConfidenceLevel::Low
    } else if matched_sessions.len() > 1 {
        ConfidenceLevel::Medium
    } else {
        ConfidenceLevel::High
    };
    let per_session: Vec<SessionConfidence> = matched_sessions
        .iter()
        .map(|s| SessionConfidence {
            session_id: s.session_id.clone(),
            confidence: overall,
        })
        .collect();

    let used_bytes = used_tokens; // simplified: bytes ≈ tokens for estimate

    HandoffPack {
        schema_version: HandoffPack::SCHEMA_VERSION,
        pack_id,
        catalog_generation: input.catalog_generation,
        query: HandoffQuery {
            terms: input.query_terms.to_vec(),
            retrieval_mode: input.retrieval_mode,
            filters: Some(input.filters.clone()),
        },
        created_at,
        matched_sessions,
        mainline,
        evidence,
        inference: Vec::new(), // deterministic default: no LLM inference
        target: input.target.clone(),
        budget: HandoffBudget {
            max_tokens: input.max_tokens,
            max_bytes: input.max_bytes,
            used_tokens,
            used_bytes,
            max_evidence: Some(input.max_evidence as u64),
            context_lines: None,
        },
        truncation: TruncationStatus {
            truncated,
            reason: truncation_reason,
            dropped_count: dropped_locators.len() as u64,
            dropped_locators,
        },
        redaction: RedactionStatus::default(), // default = no redaction applied at pack level
        confidence: PackConfidence {
            overall,
            per_session,
        },
        tool_activity: Vec::new(),
        source_locators: Vec::new(),
    }
}

/// Derive a deterministic pack_id from generation + query terms.
fn derive_pack_id(generation: u64, terms: &[String]) -> String {
    let mut input = format!("gen={generation}\0");
    for term in terms {
        input.push_str(term);
        input.push('\0');
    }
    let hash = blake3::hash(input.as_bytes());
    format!("pack_v1_{}", &hash.to_hex()[..16])
}

/// Current UTC time as ISO-8601 (for audit, not reproducibility).
///
/// Hand-rolled from Unix epoch seconds (no chrono/time dependency): the
/// previous implementation embedded the epoch seconds into the minute field,
/// producing a non-conforming timestamp.
fn utc_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_utc_iso8601(secs)
}

/// Format Unix seconds as `YYYY-MM-DDTHH:MM:SSZ` in UTC using the
/// civil-from-days algorithm (Howard Hinnant, public domain).
fn format_utc_iso8601(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_session_grep_domain::StableId;

    #[test]
    fn utc_iso8601_known_epochs() {
        assert_eq!(format_utc_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc_iso8601(86_400), "1970-01-02T00:00:00Z");
        // 2026-08-16T00:00:00Z = 1786838400 (leap years included)
        assert_eq!(format_utc_iso8601(1_786_838_400), "2026-08-16T00:00:00Z");
        // Leap day: 2024-02-29T12:34:56Z
        assert_eq!(format_utc_iso8601(1_709_210_096), "2024-02-29T12:34:56Z");
    }

    fn hit(id: &str, score: f32, text: &str) -> SearchHit {
        SearchHit {
            id: StableId::from_wire(id).unwrap(),
            score,
            session_id: Some("ses_v1_abc".to_string()),
            text: Some(text.to_string()),
            why_matched: Vec::new(),
            suggested_next_commands: Vec::new(),
            occurrences: 1,
            resume_available: false,
        }
    }

    fn default_input<'a>(hits: &'a [SearchHit]) -> HandoffInput<'a> {
        static EMPTY: Vec<String> = Vec::new();
        HandoffInput {
            query_terms: &EMPTY,
            retrieval_mode: RetrievalMode::Lexical,
            filters: HandoffFilters::default(),
            hits,
            catalog_generation: 1,
            max_tokens: 10000,
            max_bytes: 100000,
            max_evidence: 100,
            target: None,
        }
    }

    #[test]
    fn generates_pack_with_evidence() {
        let hits = vec![
            hit("msg_v1_aaa", 1.0, "hello world"),
            hit("msg_v1_bbb", 0.8, "hello there"),
        ];
        let pack = generate_deterministic(default_input(&hits));
        assert_eq!(pack.schema_version, "1.0");
        assert!(!pack.pack_id.is_empty());
        assert_eq!(pack.evidence.len(), 2);
        assert!(pack.inference.is_empty()); // deterministic: no LLM
        assert!(!pack.truncation.truncated);
        assert_eq!(pack.confidence.overall, ConfidenceLevel::High);
    }

    #[test]
    fn empty_hits_produces_low_confidence() {
        let hits: Vec<SearchHit> = vec![];
        let pack = generate_deterministic(default_input(&hits));
        assert_eq!(pack.evidence.len(), 0);
        assert_eq!(pack.confidence.overall, ConfidenceLevel::Low);
    }

    #[test]
    fn truncates_at_max_evidence() {
        let hits: Vec<SearchHit> = (0..10)
            .map(|i| {
                hit(
                    &format!("msg_v1_{i:03}"),
                    1.0 - i as f32 * 0.1,
                    &format!("text {i}"),
                )
            })
            .collect();
        let mut input = default_input(&hits);
        input.max_evidence = 3;
        let pack = generate_deterministic(input);
        assert!(pack.truncation.truncated);
        assert_eq!(pack.truncation.reason, TruncationReason::MaxEvidence);
        assert_eq!(pack.evidence.len(), 3);
        assert_eq!(pack.truncation.dropped_count, 7);
    }

    #[test]
    fn truncates_at_budget() {
        let hits = vec![
            hit("msg_v1_aaa", 1.0, "hello world this is a long text"),
            hit("msg_v1_bbb", 0.8, "another long text that exceeds budget"),
        ];
        let mut input = default_input(&hits);
        input.max_tokens = 20; // very small budget
        let pack = generate_deterministic(input);
        assert!(pack.truncation.truncated);
        assert_eq!(pack.truncation.reason, TruncationReason::BudgetExceeded);
    }

    #[test]
    fn pack_id_is_deterministic() {
        let hits = vec![hit("msg_v1_aaa", 1.0, "hello")];
        let pack1 = generate_deterministic(default_input(&hits));
        let pack2 = generate_deterministic(default_input(&hits));
        assert_eq!(pack1.pack_id, pack2.pack_id);
    }

    #[test]
    fn different_queries_produce_different_pack_ids() {
        let hits = vec![hit("msg_v1_aaa", 1.0, "hello")];
        let terms1 = vec!["hello".to_string()];
        let terms2 = vec!["world".to_string()];
        let mut input1 = default_input(&hits);
        input1.query_terms = &terms1;
        let mut input2 = default_input(&hits);
        input2.query_terms = &terms2;
        let pack1 = generate_deterministic(input1);
        let pack2 = generate_deterministic(input2);
        assert_ne!(pack1.pack_id, pack2.pack_id);
    }

    #[test]
    fn evidence_and_inference_are_separate() {
        let hits = vec![hit("msg_v1_aaa", 1.0, "real evidence text")];
        let pack = generate_deterministic(default_input(&hits));
        // Evidence has the real text; inference is empty (deterministic).
        assert_eq!(pack.evidence.len(), 1);
        assert_eq!(pack.evidence[0].text, "real evidence text");
        assert!(pack.inference.is_empty());
    }

    #[test]
    fn matched_sessions_dedup_by_session() {
        let hits = vec![
            hit("msg_v1_aaa", 1.0, "text1"),
            hit("msg_v1_bbb", 0.8, "text2"),
        ];
        let pack = generate_deterministic(default_input(&hits));
        // Both hits have same session_id → 1 matched session with occurrences=2.
        assert_eq!(pack.matched_sessions.len(), 1);
        assert_eq!(pack.matched_sessions[0].occurrences, 2);
    }
}
