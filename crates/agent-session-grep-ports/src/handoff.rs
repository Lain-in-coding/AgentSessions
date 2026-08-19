//! Handoff Pack v1 契约类型（handoff-pack/v1）。
//!
//! JSON 是权威结构；Markdown 是 deterministic projection。
//! evidence（原文证据）与 inference（推断摘要）严格分栏，不混写。
//! 本模块只定义契约类型；生成实现在 Application 层（`handoff_pack.rs`）。
//!
//! `RetrievalMode`、`RedactionStatus`、`RedactionMode`、`RedactionState`
//! 定义在 crate root（`lib.rs`），本模块 re-export 以保持 handoff 契约自洽。

pub use crate::{RedactionMode, RedactionState, RedactionStatus, RetrievalMode};

/// Handoff pack v1 权威结构。JSON 序列化为 `schemas/handoff/v1/pack.schema.json`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandoffPack {
    pub schema_version: String,
    pub pack_id: String,
    pub catalog_generation: u64,
    /// 生成方式：当前恒为 `Deterministic`（PRD Q44 默认不调用任何模型）。
    pub generation_mode: GenerationMode,
    pub query: HandoffQuery,
    /// 确定性时间戳：由 pinned catalog generation 派生（固定 base + generation），
    /// 同 generation/query/budget 下字节可复现。
    pub created_at: String,
    pub matched_sessions: Vec<MatchedSession>,
    pub mainline: Vec<MainlineEntry>,
    pub evidence: Vec<EvidenceEntry>,
    /// 推断摘要。**在 `generation_mode: Deterministic` 下恒为空** —— 该模式
    /// 不调用任何模型，因此没有任何可推断的内容，空列表是唯一诚实的取值
    /// （不是"待填充"）。当前所有发布构建只产出 Deterministic 模式，所以
    /// 实际发出的 pack 里这个列表总是空的。非空只可能来自把
    /// `generation_mode` 标为 `LocalLlm` 的本地模型生成器，而仓库尚未提供。
    pub inference: Vec<InferenceEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<HandoffTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_window: Option<TimeWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    pub budget: HandoffBudget,
    pub truncation: TruncationStatus,
    pub redaction: RedactionStatus,
    pub confidence: PackConfidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// 产出证据的那些消息的结构化工具活动（store schema v12 的 `tool_activities`
    /// 投影，由调用方经 [`crate::ContextGraphStore::tool_activities_for_messages`]
    /// 批量解析后传入）。这些消息没有活动记录时为空并从 JSON 中省略
    /// （v12 之前建的库、或本来就没有工具调用的会话）。
    pub tool_activity: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_locators: Vec<SourceLocator>,
}

impl HandoffPack {
    pub const SCHEMA_VERSION: &'static str = "1.0";
}

/// 生成方式：deterministic（默认，不调用模型）或 local LLM（显式 opt-in）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationMode {
    Deterministic,
    LocalLlm,
}

/// 查询应用的时间窗（半开区间 `[since, until)`，与 filters 的 since/until 对齐）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimeWindow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

/// Pack 的出处：pack 来自的那个 provider/session。
///
/// 只在**恰好一个会话命中**时填充：那时两个值都是已持有的事实。跨多个会话的
/// 搜索型 pack 确实没有单一出处，保持 `None`（绝不臆造）。`provider_id` 取自
/// 该会话的 resume claim；store 没有 claim 时省略——报会话、不猜 provider。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandoffQuery {
    pub terms: Vec<String>,
    pub retrieval_mode: RetrievalMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<HandoffFilters>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandoffFilters {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

/// 声明目标 provider/agent。asg 只输出 pack 与建议命令，不静默注入。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandoffTarget {
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MatchedSession {
    pub session_id: String,
    /// store 的 resume claim 为该会话认领的 provider。无 claim（claim 之前建的
    /// 行、冲突 claim）时为 `None` 并从 JSON 省略——绝不猜。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub relevance_score: f64,
    pub occurrences: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MainlineEntry {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub role: String,
    pub ordinal: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
    #[serde(default)]
    pub is_sidechain: bool,
}

/// 原文证据：来自 Catalog 的 source span，不混入推断。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceEntry {
    pub message_id: String,
    pub source_document_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_start: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_end: Option<u64>,
    pub text: String,
}

/// 推断摘要：local LLM 或 deterministic 派生，永远标记 inference。
///
/// 注意：目前仓库内没有任何生产路径构造本类型 —— 默认生成器是
/// [`GenerationMode::Deterministic`]（不调用模型），因此
/// [`HandoffPack::inference`] 恒为空。本类型是 `local_llm` 模式的契约占位，
/// 存在的意义是保证真的接上本地模型时 evidence/inference 分栏在类型层面
/// 已经强制成立（见 `evidence_and_inference_types_are_distinct`）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InferenceEntry {
    pub kind: InferenceKind,
    pub text: String,
    pub source: InferenceSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceKind {
    Summary,
    Decision,
    Risk,
    Recommendation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceSource {
    /// 用户显式启用的本地 LLM 摘要。
    LocalLlm,
    /// 默认确定性生成（不调用模型）。
    Deterministic,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SourceLocator {
    pub source_document_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandoffBudget {
    pub max_tokens: u64,
    pub max_bytes: u64,
    pub used_tokens: u64,
    pub used_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_evidence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_lines: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TruncationStatus {
    pub truncated: bool,
    pub reason: TruncationReason,
    #[serde(default)]
    pub dropped_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped_locators: Vec<SourceLocator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruncationReason {
    BudgetExceeded,
    MaxItems,
    MaxEvidence,
    MaxBytes,
    None,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PackConfidence {
    pub overall: ConfidenceLevel,
    pub per_session: Vec<SessionConfidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionConfidence {
    pub session_id: String,
    pub confidence: ConfidenceLevel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieval_mode_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&RetrievalMode::LexicalFallback).unwrap(),
            "\"lexical_fallback\""
        );
        assert_eq!(
            serde_json::to_string(&RetrievalMode::Hybrid).unwrap(),
            "\"hybrid\""
        );
    }

    #[test]
    fn redaction_default_is_none_redacted() {
        let r = RedactionStatus::default();
        assert_eq!(r.mode, RedactionMode::Default);
        assert_eq!(r.status, RedactionState::None);
        assert_eq!(r.redacted_count, 0);
        assert!(r.audit_id.is_none());
    }

    #[test]
    fn redaction_round_trip_preserves_revealed_audit() {
        let r = RedactionStatus {
            mode: RedactionMode::Revealed,
            status: RedactionState::Applied,
            ruleset_version: "v1.0".into(),
            redacted_count: 3,
            audit_id: Some("audit-xyz".into()),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: RedactionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn evidence_and_inference_types_are_distinct() {
        // 编译期保证：evidence 和 inference 是不同类型，不能混写。
        let _evidence: Vec<EvidenceEntry> = Vec::new();
        let _inference: Vec<InferenceEntry> = Vec::new();
    }
}
