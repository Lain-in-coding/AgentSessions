//! Application：跨前端（CLI / Robot-JSON / MCP / TUI）共享的用例与请求/结果 ADT。
//!
//! 本层不知道任何具体前端或后端——只依赖 domain 的类型与 ports 的抽象。
//! 所有前端把各自的输入归一为 [`AppRequest`]，把 [`AppResponse`] 渲染成各自的输出格式。
//! 分层依赖不变量：domain ← ports ← application ← adapters。

use agent_session_grep_domain::{
    ContextPolicy, DomainError, IdKind, Message, MessagePlacement, Role, SourceDocument, StableId,
    ToolActivity, select_full, select_mainline,
};
use agent_session_grep_ports::{
    CanonicalEventSink, CatalogStore, Confidence, ContextGraphStore, HistoryStats, MessageEvent,
    NoResumeClaims, NoSemanticIndex, ParseReport, PortError, PortResult, ProbeResult,
    ProviderAdapter, ProviderError, ReadOnlySource, ResumeClaimsStore, RetrievalMode,
    SQLITE_MAGIC_HEADER, SearchFacets, SearchFilters, SearchHit, SearchIndex, SearchInstant,
    SearchQuery, SemanticIndex, SessionResumeMetadata, SourceFormatFamily, ToolActivityEvent,
    read_source_head, source_format_family_for,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub mod budget;
#[cfg(feature = "semantic-candle")]
pub mod candle_embedding;
pub mod cjk;
pub mod cursor;
pub mod embedding;
pub mod evidence;
pub mod guidance;
pub mod handoff_markdown;
pub mod handoff_pack;
pub mod hybrid;
pub mod resume;

pub use budget::{ResponseBudget, Truncation};
pub use cjk::bigram_cjk;
pub use evidence::EvidenceSpanDto;

/// 排序方案标识：catalog 列表的钉住排序（wire id 升序，见 sqlite `ORDER BY id ASC`）。
pub const SORT_WIRE_ID_ASC: &str = "wire_id_asc";
/// 排序方案标识：检索结果的钉住排序（bm25 降序 + id tiebreak，全序确定）。
pub const SORT_SCORE_DESC: &str = "score_desc";
/// 排序方案标识：会话最近活动降序（recency 浏览；缺时间戳者末尾 + wire id 收尾）。
pub const SORT_RECENCY_DESC: &str = "recency_desc";

/// Cursor 结果集判别器（resume-protocol-prerequisites R2）：`list` 全部实体。
pub const RESULT_SET_ALL: &str = "all";
/// Cursor 结果集判别器：`list_sessions` 仅会话实体。
pub const RESULT_SET_SESSIONS_ONLY: &str = "sessions_only";

/// `resume_available` 恒序列化进 searchHit 的字节开销（`, "resume_available":false`）。
const RESUME_AVAILABLE_FIELD_BYTES: usize = 24;

/// `list` 一条 JSON 条目的固定结构开销：
/// `{"id":,"payload":,"latest_activity":}` 的键名、引号、逗号与花括号
/// （三个值各自的序列化长度另算）。
const LIST_ENTRY_ENVELOPE_BYTES: usize = 37;

/// clamp 前从 `max_response_bytes` 扣除的 envelope 预留（budget.rs 声明预留是调用方义务）。
/// 预算下限 4096 保证扣除后仍为正。
const ENVELOPE_RESERVE_BYTES: usize = 1024;

/// Maximum number of Session IDs exposed by a message-resolution ambiguity.
/// The total count remains available separately while the candidate list stays
/// bounded for protocol and privacy safety.
pub const MAX_MESSAGE_AMBIGUITY_CANDIDATES: usize = 8;

/// Fixed metadata for a derived context view. Duplicated message payloads are
/// charged per retained occurrence so clamping can still preserve a prefix.
const STRUCTURAL_METADATA_RESERVE_BYTES: usize = 320;

/// Upper bound on a single fetch window. Cursor offsets are tamper-evident
/// but not unforgeable; capping the window keeps a forged huge offset from
/// overflowing into a negative SQL LIMIT (SQLite treats -1 as "no limit").
const MAX_FETCH_WINDOW: u64 = 1 << 20;

/// group_by_session（R3）模式下相对分页窗口的扫描放大倍数：归并需要把命中先
/// 汇到会话级再切页，扫描窗口取 `页窗口 × GROUP_SCAN_FACTOR`（仍被
/// MAX_FETCH_WINDOW 封顶），使 `occurrences` 覆盖更有意义的命中样本。
const GROUP_SCAN_FACTOR: u64 = 16;

/// 一条 JSON 字符串字面量的序列化长度（含两端引号与转义）。
fn json_string_len(value: &str) -> usize {
    2 + value
        .bytes()
        .map(|b| match b {
            b'"' | b'\\' => 2,
            0x00..=0x1F | 0x7F => 6, // \uXXXX
            _ => 1,
        })
        .sum::<usize>()
}

/// 估算 `payload` 经 `String::from_utf8_lossy` 转为字符串、再做 JSON 字符串
/// 转义后的序列化长度（含两端引号）。
///
/// 字节闸的估算必须按**序列化后**长度计，而不是原始字节数：`Vec<u8>` 渲染为
/// 字符串时，控制字节会膨胀为 `\uXXXX`（6 字符），无效 UTF-8 序列替换为一个
/// U+FFFD（3 字节），与 CLI 的 lossy 渲染一致。
fn lossy_payload_json_len(payload: &[u8]) -> usize {
    fn utf8_width(byte: u8) -> usize {
        match byte {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 0, // 续字节或越界头字节：无效
        }
    }
    let mut len = 2usize; // 两端引号
    let mut i = 0;
    while i < payload.len() {
        let width = utf8_width(payload[i]);
        if width == 0
            || i + width > payload.len()
            || (1..width).any(|k| payload[i + k] & 0xC0 != 0x80)
        {
            // 无效/截断序列 → 一个 U+FFFD（3 字节，无需转义）。
            len += 3;
            i += 1;
        } else {
            // 多字节序列的续字节不会是引号/控制字符，只有单字节需要转义计。
            len += if width == 1 {
                match payload[i] {
                    b'"' | b'\\' => 2,
                    0x00..=0x1F | 0x7F => 6,
                    _ => 1,
                }
            } else {
                width
            };
            i += width;
        }
    }
    len
}

/// Application 边界错误：保留 Domain、Port、Provider、Cursor 与 Budget 的原始分类，
/// 供各前端统一映射协议（cursor/budget 错误在 protocol 层有专属 canonical code）。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Cursor(#[from] cursor::CursorError),
    #[error(transparent)]
    Budget(#[from] budget::BudgetError),
    #[error(transparent)]
    MessageAmbiguous(#[from] MessageAmbiguity),
}

/// 列表用例的排序维度（唯一真源；前端只映射，不各自定义排序语义）。
///
/// 两个维度都是**全序**，因此都能安全承载 cursor 的 offset 语义；
/// [`ListSort::digest`] 把维度绑进令牌，跨排序复用令牌显式失败
/// （见 [`cursor::verify`]，绝不静默从第一页继续）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListSort {
    /// wire id 升序：wire id 是 BLAKE3 摘要，与时间无关（对用户等于随机顺序），
    /// 但**稳定**——依赖稳定分页的调用方保持默认走这里。
    #[default]
    WireIdAsc,
    /// 会话最近活动降序（recency 浏览）：缺时间戳的会话排在末尾，
    /// 见 [`agent_session_grep_ports::CatalogStore::sessions_by_recency`]。
    RecencyDesc,
}

impl ListSort {
    /// 绑进 cursor 的排序方案标识。
    pub fn digest(self) -> &'static str {
        match self {
            Self::WireIdAsc => SORT_WIRE_ID_ASC,
            Self::RecencyDesc => SORT_RECENCY_DESC,
        }
    }
}

/// 应用层请求 ADT：所有前端的统一入口。
///
/// 每个变体是一个用例。前端负责解析各自语法后构造本枚举，
/// 从而保证 CLI / Robot / MCP / TUI 行为一致（见 CONTRACT-cli-robot-mcp-draft）。
///
/// `Search` 变体比其余变体大得多（约 347 vs 112 字节），因为 M3-3/M3-8 把
/// provider / role / project / exclude 四组过滤维度都放进了 `SearchFilters`。
/// 这里**刻意不 box**：`AppRequest` 每次请求只构造一个、立即被 `handle` 消费，
/// 不进集合也不排队，所以枚举大小不影响任何热路径；而 box 化会给 21 处调用点
/// 加上一层间接，把"前端直接构造用例"这个本层最重要的可读性换成一次分配。
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum AppRequest {
    /// 全文检索：按查询串返回命中列表（分页 + 预算）。
    Search {
        query: String,
        /// Optional normalized metadata predicates. Providers are ORed; the
        /// provider and half-open UTC time dimensions are ANDed.
        filters: SearchFilters,
        /// 结构化 facet 过滤（sidechain / 工具 kind / 工具名）。默认值 = 无过滤，
        /// 行为与旧版完全一致；facet 变化会使已发 cursor 失效（绑定进 digest）。
        facets: SearchFacets,
        /// 最多返回条数；0 视为非法请求。与 `budget.max_items` 取较小者为页大小。
        limit: usize,
        /// 上一页发出的续读令牌；`None` 表示第一页。
        cursor: Option<String>,
        /// 响应预算（CONTRACT §3）；低于下限即校验错误。
        budget: ResponseBudget,
        /// 是否包含系统噪声（competitor-borrowings R2）：`false`（默认）排除
        /// role 为 system/developer 的命中，`true` 显式恢复。
        include_system: bool,
        /// 是否按会话归并（competitor-borrowings R3）：`false`（默认）保持
        /// 逐命中分页；`true` 时每会话只保留最高分命中并附带 `occurrences`。
        group_by_session: bool,
        /// 检索模式（#3）：`Lexical`（默认）走 FTS；`Semantic`/`Hybrid` 需要
        /// 已就绪的语义索引与 `query_embedding`，否则显式降级为
        /// `LexicalFallback` + warning。
        mode: RetrievalMode,
        /// 查询文本的 embedding（调用方经 EmbeddingModel 生成，Application
        /// 保持模型无关）。`Semantic`/`Hybrid` 模式必须提供；缺失即降级。
        query_embedding: Option<Vec<f32>>,
        /// 命中处的行上下文窗口（M3-9，grep 的 `-C N`）：`Some(n)` 时 snippet
        /// 取"命中所在行 ± n 整行"的**连续**区域；`None`（默认）保持既有的
        /// 以命中为中心的字符窗口，输出字节不变。
        ///
        /// 与 `get-message --around n` 不是同一个轴：那个取的是会话主线上的
        /// **邻居消息**，这个取的是命中消息正文内的**邻居行**。两者可组合，
        /// 不可互相替代，所以没有复用它的机器。
        context_lines: Option<usize>,
    },
    /// 按稳定 ID 取回单个实体的原始负载。
    Get { id: StableId },
    /// 按稳定 wire id 展开展示一个实体；当前 Beta 返回规范化 payload。
    Show { id: StableId },
    /// 按稳定 wire id 顺序列出实体（分页 + 预算）；0 视为非法。
    List {
        limit: usize,
        cursor: Option<String>,
        budget: ResponseBudget,
        /// true 时只列出 Session 实体（competitor-borrowings R1.3：`list_sessions`
        /// 不再被 doc/msg 实体淹没）。过滤在存储层做，offset/limit 分页语义
        /// 保持作用在过滤后的集合上。
        sessions_only: bool,
        /// 排序维度（M3-4）：默认 wire id 升序；`RecencyDesc` 是"我昨天干了
        /// 什么"的排序，只对会话有定义，因此要求 `sessions_only`。
        sort: ListSort,
    },
    /// 会话上下文装配：按策略选取分支，返回消息链与证据区间（CONTRACT §1-2）。
    Context {
        session_id: StableId,
        policy: ContextPolicy,
        level: ContextLevel,
        budget: ResponseBudget,
    },
    /// Resolve one stable Message in an authoritative Session mainline and
    /// return a bounded placement window around it.
    Message {
        message_id: StableId,
        session_id: Option<StableId>,
        around: usize,
        budget: ResponseBudget,
    },
    /// Resolve every distinct Session that contains placements for one Message.
    MessageContexts { message_id: StableId },
    /// Resolve read-only Resume Metadata for one canonical Session (ADR-0009).
    GetSessionResume { session_id: StableId },
    /// 返回当前 Catalog 统计状态。
    Status,
    /// 返回历史构成统计（provider / month / project / session size）。
    Stats,
}

/// Detail level for a context response. `Raw` is the compatibility default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextLevel {
    Raw,
    Talks,
    Sessions,
}

impl ContextLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Talks => "talks",
            Self::Sessions => "sessions",
        }
    }
}

/// One placement-aware context response item.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContextMessage {
    /// Compatibility alias retained for existing frontends.
    pub id: String,
    /// Authoritative occurrence identity.
    pub placement_id: String,
    /// Stable Message identity; always equal to `id`.
    pub message_id: String,
    /// Existing canonical Message payload, returned opaquely.
    pub payload: serde_json::Value,
}

/// A structural talk: one user message followed by assistant/tool messages.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContextTalk {
    pub user_message: ContextMessage,
    pub following_messages: Vec<ContextMessage>,
}

/// A bounded structural overview of one selected context branch.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContextSessionSummary {
    pub first_user_message: Option<ContextMessage>,
    pub message_count: usize,
    pub turn_count: usize,
    pub file_references: Vec<String>,
}

/// A deterministic next-call hint. Identifiers are copied from the context.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContextHint {
    pub command: String,
    pub session_id: String,
    pub level: ContextLevel,
}

/// One distinct Session candidate for a stable Message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MessageContextCandidate {
    pub session_id: String,
    pub placement_ids: Vec<String>,
}

/// Bounded explicit ambiguity when a shared Message belongs to several Sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageAmbiguity {
    pub candidate_session_ids: Vec<String>,
    pub candidate_count: usize,
    pub hint: String,
}

impl std::fmt::Display for MessageAmbiguity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("message belongs to multiple sessions; provide session_id")
    }
}

impl std::error::Error for MessageAmbiguity {}

/// One message window selected from an authoritative Session mainline.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageWindow {
    pub message_id: String,
    pub session_id: String,
    pub anchor_placement_id: String,
    pub messages: Vec<ContextMessage>,
    pub truncation: Truncation,
    pub generation: u64,
}

/// 一条列表条目：catalog 实体 + 当前排序维度下可见的排序键。
///
/// `latest_activity` 只有 recency 排序会填：**排序键必须能被看见**，否则
/// "按最近活动排序"在输出里无从验证。`None` 有两种诚实含义——wire-id
/// 排序（本维度不做时间投影）或该会话没有任何带时间戳的消息；两者都不编造时刻。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    pub id: StableId,
    pub payload: Vec<u8>,
    /// 会话内消息时间戳的词法最大值（provider-native ISO-8601，逐字保留）。
    pub latest_activity: Option<String>,
}

/// 应用层结果 ADT：前端据此渲染，不再回到 domain/ports 类型。
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum AppResponse {
    /// 检索结果，按相关性降序（bm25 + id tiebreak 的钉住全序）。
    Search {
        hits: Vec<SearchHit>,
        /// 还有后续页时的续读令牌；`None` 表示已到末尾。
        next_cursor: Option<String>,
        /// 本页对应的活动 generation（cursor 绑定它）。
        generation: u64,
        /// 预算截断标记；`truncated` 时前端应报 partial。
        truncation: Truncation,
        /// 实际生效的检索模式（#3）：请求 semantic/hybrid 但索引未就绪时为
        /// `LexicalFallback`，envelope 必须如实标注。
        retrieval_mode: RetrievalMode,
        /// 降级说明（#3）：semantic/hybrid 降级到 lexical 时的 warning 文本。
        fallback_warning: Option<String>,
        /// 时间窗因缺时间戳而排除的记录数（D11：宽容但报数）。SQL 里 NULL 对
        /// 每个比较都为假，故无 per-message 时间戳的记录（aider 只有 run 级
        /// 头部时间、codex 故意不传播 replay envelope 时间）落不进任何
        /// `[since, until)` 窗口。0 表示无时间窗或无排除——前端此时不得发
        /// warning（输出字节不变）。
        time_filter_excluded: u64,
        /// 生效的行上下文窗口（M3-9）：原样回显请求里的 `context_lines`，让
        /// **所有**入口在同一个 render 投影里拿到它，而不是各自记住自己传了
        /// 什么。`None` 时该键整个省略，默认输出字节不变。
        context_lines: Option<usize>,
    },
    /// 单个实体的原始负载；`None` 表示未找到。
    Get { payload: Option<Vec<u8>> },
    /// 单个实体的规范化展示；`None` 表示未找到。
    Show { payload: Option<Vec<u8>> },
    /// 稳定排序后的 Catalog 条目（wire id 升序）。
    List {
        entries: Vec<ListEntry>,
        next_cursor: Option<String>,
        generation: u64,
        truncation: Truncation,
    },
    /// 会话上下文：选中分支的有序消息链 + 证据区间。
    Context {
        /// 会话 wire id。
        session_id: String,
        /// 会话 canonical payload（`{document, messages}`）。
        session: serde_json::Value,
        /// 选中分支的叶子消息 wire id；会话无消息时为 `None`。
        branch_leaf: Option<String>,
        /// Authoritative leaf occurrence; `branch_leaf` remains its Message-ID alias.
        branch_leaf_placement_id: Option<String>,
        /// root→leaf（mainline）或 deterministic placement order（full）的 occurrences。
        messages: Vec<ContextMessage>,
        /// 与 `messages` 对齐装配的证据区间（可能被 `max_evidence_spans` 截短）。
        evidence: Vec<EvidenceSpanDto>,
        /// Tool activities for messages in this context (schema v12 projection).
        /// Empty when none are stored; never fabricated.
        tool_activities: Vec<serde_json::Value>,
        /// Requested and effective structural response levels.
        requested_level: ContextLevel,
        effective_level: ContextLevel,
        /// Structural talk groups for `talks` responses.
        talks: Vec<ContextTalk>,
        /// Structural overview for `sessions` responses.
        summary: Option<ContextSessionSummary>,
        /// Deterministic next-call hint, when a real session identifier is available.
        hint: Option<ContextHint>,
        truncation: Truncation,
        generation: u64,
    },
    /// One placement-aware mainline window.
    Message { window: MessageWindow },
    /// Distinct-session candidates for a stable Message.
    MessageContexts {
        message_id: String,
        candidates: Vec<MessageContextCandidate>,
    },
    /// Read-only Resume Metadata projection (ADR-0009)：固定可空字段，恒在。
    SessionResume(SessionResumeMetadata),
    /// 当前 Catalog 实体总数、关系统计与活动 generation。
    Status {
        catalog_count: u64,
        active_generation: u64,
        placements: u64,
        source_placement_claims: u64,
    },
    /// 历史构成普查（M3-10）：库里有什么、来自哪里。
    ///
    /// 各维度的桶原样透传端口 DTO——Application 不做排序或合并，那是存储层
    /// 冻结的契约（见 [`agent_session_grep_ports::HistoryStats`]）。项目维度的
    /// 路径是本机绝对路径：human 面可按 ADR-0004 显示，跨边界面必须按 ADR-0009
    /// 只投影 basename（由 CLI 渲染层负责）。
    Stats {
        stats: HistoryStats,
        active_generation: u64,
    },
}

/// 一条经 staging 缓冲、待原子提交的规范化消息（RFC-0002 §5）。
///
/// 承载 parse 产出的语义与 provider-native 身份/threading 元数据，但不含 StableId——
/// id 派生策略（native uuid 优先，缺失时回退 path+seq）由调用方决定，
/// 使 stage 本身与存储无关、可独立单测。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedMessage {
    pub seq: u32,
    /// provider-native 消息 id；空串表示 provider 未提供，调用方回退派生。
    pub native_id: String,
    /// 父消息 native id（threading 边）；`None` 表示根消息或未提供。
    pub parent_native_id: Option<String>,
    pub role: String,
    pub text: String,
    /// provider 原样时间串（ISO-8601 UTC）；`None` 表示缺失。
    pub timestamp: Option<String>,
    /// 是否为 sidechain（subagent/分支）消息。
    pub is_sidechain: bool,
    /// 源记录在已验证快照字节中的区间 `(start, end)`，end 排他；
    /// `None` 表示 provider 无法归因，绝不臆造。
    pub span: Option<(u64, u64)>,
}

/// 一条经 staging 缓冲、待原子提交的工具活动观察（RFC-0002 §5 扩展）。
///
/// 锚点以 provider-native 消息 id 承载；[`StagedBatch`] 的调用方负责把它解析
/// 为本批内稳定消息身份——解析失败（锚点消息未 emit）即丢弃，绝不臆造锚点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedActivity {
    pub message_native_id: String,
    pub activity: ToolActivity,
}

/// 一次 staging 的完整产物：缓冲消息 + provider 的完整解析报告。
///
/// 会话 native id 的唯一权威来源是 [`ParseReport::session_native_id`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedBatch {
    pub messages: Vec<StagedMessage>,
    /// 工具活动观察（设计 R1-R6；provider 未声明工具活动时为空）。
    pub activities: Vec<StagedActivity>,
    /// Authoritative provider parse accounting and diagnostics.
    pub report: ParseReport,
    /// Deprecated compatibility shim: the CLI composition root still
    /// constructs this struct literally, so the field cannot be removed yet.
    /// It mirrors `report.session_native_id` (which is the single source of
    /// truth) and must not be trusted independently — new code must read
    /// `report.session_native_id`.
    pub session_native_id: Option<String>,
}

/// Ingest 编排（RFC-0002 §5 source-level staging）：
///
/// 1. `probe` 判定 variant；ambiguous / 探测失败 → 立即拒绝，不 parse；
/// 2. `parse` 把 Canonical 消息推入**内存缓冲**（不写库）；
/// 3. 仅当 parse 完整成功才返回全部缓冲消息；任何失败返回 Err 且**不产出部分结果**。
///
/// 提交（写 catalog + FTS）由调用方在拿到完整 [`StagedBatch`] 后，
/// 用单事务原子执行（见 `SqliteStore::commit_batch`）。
pub fn stage(adapter: &dyn ProviderAdapter, bytes: &[u8]) -> Result<StagedBatch, AppError> {
    let probe = adapter.probe(bytes)?;
    stage_probed(adapter, bytes, probe)
}

/// Probe 完成后继续 stage（内部步骤）。
///
/// ambiguous 置信度：默认拒绝解析，不做"尽量解析"（RFC-0002 §3）。错误消息只
/// 保留 variant 标识，不携带 `unmatched_evidence`（可能含路径/原文片段）。
fn stage_probed(
    adapter: &dyn ProviderAdapter,
    bytes: &[u8],
    probe: ProbeResult,
) -> Result<StagedBatch, AppError> {
    if matches!(probe.confidence, Confidence::Ambiguous) {
        return Err(
            DomainError::InvalidRequest(format!("ambiguous variant {}", probe.variant_id)).into(),
        );
    }
    let mut sink = StagingSink::default();
    let report = adapter.parse(bytes, &mut sink)?;
    Ok(staged_batch(sink, report))
}

/// Reader-aware variant of [`stage_probed`]: parses via a fresh bounded reader,
/// so no source-sized byte buffer crosses the application boundary.
fn stage_source_probed(
    adapter: &dyn ProviderAdapter,
    source: &dyn ReadOnlySource,
    probe: ProbeResult,
) -> Result<StagedBatch, AppError> {
    if matches!(probe.confidence, Confidence::Ambiguous) {
        return Err(
            DomainError::InvalidRequest(format!("ambiguous variant {}", probe.variant_id)).into(),
        );
    }
    let mut sink = StagingSink::default();
    let report = adapter.parse_source(source, &mut sink)?;
    Ok(staged_batch(sink, report))
}

fn staged_batch(sink: StagingSink, report: ParseReport) -> StagedBatch {
    // 镜像到兼容别名（见 StagedBatch::session_native_id 文档）：唯一权威来源
    // 是 report.session_native_id。
    let session_native_id = report.session_native_id.clone();
    StagedBatch {
        messages: sink.buffered,
        activities: sink.activities,
        report,
        session_native_id,
    }
}

/// Stable classification prefix for "no adapter claims this source".
///
/// Shared by the two construction sites below and by [`source_rejection`], so
/// the predicate cannot drift away from the message it recognizes.
pub const UNRECOGNIZED_SOURCE_MESSAGE: &str = "no provider recognized this source";

/// Stable classification prefix for "several adapters matched equally well".
pub const AMBIGUOUS_SOURCE_MESSAGE: &str = "ambiguous provider selection";

/// Why a single source was rejected at selection time.
///
/// Both are *source-level* verdicts about one file, not failures of the run, so
/// a caller that walked a directory it did not choose (`sync --discover`) can
/// skip the file and keep going. A caller that was handed the path explicitly
/// still treats them as errors. They need different words, though: telling a
/// user that a real transcript "is not a recognized transcript" when several
/// adapters actually claimed it would send them after the wrong problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRejection {
    /// No adapter claimed the source.
    Unclaimed,
    /// Several adapters claimed it equally well; ambiguity is refused, never guessed.
    Ambiguous,
}

/// Classify `error` as a source-level rejection, if it is one.
pub fn source_rejection(error: &AppError) -> Option<SourceRejection> {
    let AppError::Domain(DomainError::InvalidRequest(message)) = error else {
        return None;
    };
    if message.starts_with(UNRECOGNIZED_SOURCE_MESSAGE) {
        Some(SourceRejection::Unclaimed)
    } else if message.starts_with(AMBIGUOUS_SOURCE_MESSAGE) {
        Some(SourceRejection::Ambiguous)
    } else {
        None
    }
}

/// Build the "nothing claimed this source" error, attaching a probe diagnostic
/// only when it is about this source's own format family.
///
/// Adapters are probed in registry order and the *last* failure used to be
/// appended verbatim. The registry ends with the SQLite-backed adapters, so
/// every unrecognized text file was reported as
/// `not a SQLite database (missing magic header)` — an internal probe detail
/// that names the wrong format and gives the user nothing to act on. Filtering
/// by family keeps the diagnostics that locate a repair (a JSONL adapter
/// naming the offending lines) and drops the ones that answer a question the
/// user never asked.
fn unrecognized_source_error(
    family: SourceFormatFamily,
    probe_failures: &[(SourceFormatFamily, ProviderError)],
) -> AppError {
    let relevant = probe_failures
        .iter()
        .filter(|(probe_family, _)| *probe_family == family)
        .map(|(_, error)| error)
        .next_back();
    // `AmbiguousVariant` 的内层文本是 adapter 写给用户的判定（常带行号与修复
    // 方向）；`ambiguous or unknown variant:` 这层 Display 前缀是内部分级词汇，
    // 不外泄。其余变体（过大 / IO）本身就是用户可行动的事实，照原样带出。
    let detail = match relevant {
        Some(ProviderError::AmbiguousVariant(message)) => format!(": {message}"),
        Some(error) => format!(": {error}"),
        None => String::new(),
    };
    DomainError::InvalidRequest(format!(
        "{UNRECOGNIZED_SOURCE_MESSAGE}{detail}. Supported transcript formats are listed by \
         `agent-session-grep providers`; if this file is not an agent transcript, leave it out"
    ))
    .into()
}

/// 从多个 provider adapter 中选出匹配的那个，再 stage（RFC-0002 §3 provider 选择）。
///
/// 对每个 adapter 调 `probe`：跳过报错或 ambiguous 的（不是候选）；在剩余候选里
/// 取置信度最高的（Confirmed > High > Low）。若并列最高有多个不同 variant，视为
/// 无法区分，拒绝（不做"猜一个"）。选中后用该 adapter stage。
///
/// 无任何候选 → `InvalidRequest`（没有 provider 认领此源）。probe 报错的 adapter
/// 若与本源同属一个格式家族，其诊断自带行号定位与修复方向（PRD R2.2）会被附上；
/// 异家族的 probe 内部细节不外泄（见 [`unrecognized_source_error`]）。
/// 组合根（CLI）持有具体 adapter 清单，本函数只负责与格式无关的选择编排。
pub fn select_and_stage(
    adapters: &[&dyn ProviderAdapter],
    bytes: &[u8],
) -> Result<StagedBatch, AppError> {
    let mut candidates: Vec<(u8, usize, ProbeResult)> = Vec::new();
    // probe 报错按格式家族留存：只有与本源同家族的诊断可以外泄给用户。
    let mut probe_failures: Vec<(SourceFormatFamily, ProviderError)> = Vec::new();
    for (idx, adapter) in adapters.iter().enumerate() {
        // probe 报错的 adapter 不是候选——它明确表示"这不是我的格式"。
        let probe = match adapter.probe(bytes) {
            Ok(probe) => probe,
            Err(error) => {
                probe_failures.push((source_format_family_for(adapter.provider_id()), error));
                continue;
            }
        };
        if let Some(r) = confidence_rank(probe.confidence) {
            candidates.push((r, idx, probe));
        }
    }

    let (idx, probe) = choose_probed_candidate(adapters, candidates, None)?.ok_or_else(|| {
        unrecognized_source_error(SourceFormatFamily::of_head(bytes), &probe_failures)
    })?;
    // 复用选中时的 probe 结果，不再对同一字节第二次 probe。
    stage_probed(adapters[idx], bytes, probe)
}

/// 置信度排序键：越大越可信；ambiguous 不参与候选。
fn confidence_rank(c: Confidence) -> Option<u8> {
    match c {
        Confidence::Confirmed => Some(3),
        Confidence::High => Some(2),
        Confidence::Low => Some(1),
        Confidence::Ambiguous => None,
    }
}

/// 在 probe 候选中选出唯一胜者；同分不同 variant 时按 `provider_hint` 消歧。
///
/// 取置信度最高的一组；若组内只有一个 variant，直接胜出。若有多个 variant，
/// 内容本身已无法区分它们 —— 此时**唯一**可用的证据是 `provider_hint`：
/// discover 阶段记下的"这个文件来自哪个 provider 的数据根"。命中同分候选之一
/// 即由它胜出。
///
/// 这不是"猜一个"（RFC-0002 宁拒不猜的边界）：root 是独立于文件内容的外部
/// 事实。pi 与 openclaw 的磁盘格式确实是同一形状（openclaw adapter 自己的
/// 文档就这么写），所以任何内容 probe 都区分不了它们，而目录归属可以。
/// 没有 hint（手动 `sync <file>`）或 hint 不在同分候选里时仍然拒绝。
///
/// `Ok(None)` 表示没有任何候选；调用方据此构造 unrecognized 错误（它需要
/// 家族信息，本函数拿不到）。
fn choose_probed_candidate(
    adapters: &[&dyn ProviderAdapter],
    candidates: Vec<(u8, usize, ProbeResult)>,
    provider_hint: Option<&str>,
) -> Result<Option<(usize, ProbeResult)>, DomainError> {
    let Some(max_rank) = candidates.iter().map(|(r, _, _)| *r).max() else {
        return Ok(None);
    };
    let mut top: Vec<(usize, ProbeResult)> = candidates
        .into_iter()
        .filter(|(r, _, _)| *r == max_rank)
        .map(|(_, idx, probe)| (idx, probe))
        .collect();

    let mut distinct: Vec<&str> = top.iter().map(|(_, p)| p.variant_id.as_str()).collect();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() <= 1 {
        return Ok(Some(top.swap_remove(0)));
    }

    if let Some(hint) = provider_hint {
        // registry 里 provider_id 唯一、每个 adapter 只出一个候选，所以命中
        // 至多一个。0 命中说明 root 与同分候选无关，仍属无法区分。
        if let Some(pos) = top
            .iter()
            .position(|(idx, _)| adapters[*idx].provider_id() == hint)
        {
            return Ok(Some(top.swap_remove(pos)));
        }
    }

    Err(ambiguous_source_error(&distinct, provider_hint))
}

/// 歧义错误:列出**全部**并列 variant 并给出消歧办法。
///
/// 此前只报一个候选(`one candidate was X`),用户既不知道和谁撞了,也不知道
/// 下一步能做什么 —— 而 `AmbiguousVariant` 是刻意的拒绝(RFC-0002:宁拒不猜),
/// 所以这条消息是用户唯一的线索,必须自足。
fn ambiguous_source_error(tied: &[&str], provider_hint: Option<&str>) -> DomainError {
    let mut variants: Vec<&str> = tied.to_vec();
    variants.sort_unstable();
    variants.dedup();
    // 有 hint 却走到这里，说明它指向的 provider 根本不在同分候选中——
    // 这是个独立的事实，不说出来用户会以为 hint 没生效。措辞保持中立：
    // hint 可能来自 discover 的 root 归属，也可能来自显式 `--provider`。
    let hint_note = match provider_hint {
        Some(hint) => format!(
            "; {hint} was named as the expected provider, but {hint} is not among \
             the tied candidates"
        ),
        None => String::new(),
    };
    DomainError::InvalidRequest(format!(
        "{AMBIGUOUS_SOURCE_MESSAGE}: {} variants matched with equal confidence \
         ({}){hint_note}; this source's shape is not distinctive enough to attribute, \
         so it is refused rather than guessed. Re-run with `sync --provider <id> <file>` \
         to name the provider explicitly",
        variants.len(),
        variants.join(", ")
    ))
}

/// Select and stage a repeatable read-only source (RFC-0002 §7 bounded ingest).
///
/// Same selection rules as [`select_and_stage`], but every probe/parse opens a
/// fresh bounded reader, so no source-sized byte buffer crosses the application
/// boundary. Returns `(staged, selected variant_id)`.
///
/// `provider_hint` is the provider whose data root this source was discovered
/// under, when that is known. It is consulted **only** to break a tie between
/// otherwise indistinguishable candidates — see [`choose_probed_candidate`].
pub fn select_and_stage_source(
    adapters: &[&dyn ProviderAdapter],
    source: &dyn ReadOnlySource,
    provider_hint: Option<&str>,
) -> Result<(StagedBatch, String), AppError> {
    let mut candidates: Vec<(u8, usize, ProbeResult)> = Vec::new();
    let mut probe_failures: Vec<(SourceFormatFamily, ProviderError)> = Vec::new();
    for (idx, adapter) in adapters.iter().enumerate() {
        let probe = match adapter.probe_source(source) {
            Ok(probe) => probe,
            Err(error) => {
                probe_failures.push((source_format_family_for(adapter.provider_id()), error));
                continue;
            }
        };
        if let Some(r) = confidence_rank(probe.confidence) {
            candidates.push((r, idx, probe));
        }
    }

    let (idx, probe) =
        choose_probed_candidate(adapters, candidates, provider_hint)?.ok_or_else(|| {
            // 家族判定只看源开头的签名字节，不整读（SQLite 整读上限 128 MiB）。
            let head = read_source_head(source, SQLITE_MAGIC_HEADER.len()).unwrap_or_default();
            unrecognized_source_error(SourceFormatFamily::of_head(&head), &probe_failures)
        })?;
    let variant = probe.variant_id.clone();
    let staged = stage_source_probed(adapters[idx], source, probe)?;
    Ok((staged, variant))
}

/// 内存 staging sink：只缓冲，绝不触库。
#[derive(Default)]
struct StagingSink {
    buffered: Vec<StagedMessage>,
    activities: Vec<StagedActivity>,
}

impl CanonicalEventSink for StagingSink {
    fn emit_message(&mut self, event: MessageEvent<'_>) -> PortResult<()> {
        self.buffered.push(StagedMessage {
            seq: event.seq,
            native_id: event.native_id.to_string(),
            parent_native_id: event.parent_native_id.map(str::to_string),
            role: event.role.to_string(),
            text: event.text.to_string(),
            timestamp: event.timestamp.map(str::to_string),
            is_sidechain: event.is_sidechain,
            span: event.span,
        });
        Ok(())
    }

    fn emit_activity(&mut self, event: ToolActivityEvent<'_>) -> PortResult<()> {
        self.activities.push(StagedActivity {
            message_native_id: event.message_native_id.to_string(),
            activity: event.activity,
        });
        Ok(())
    }
}

/// Parse a timezone-qualified RFC3339/ISO-8601 timestamp into a normalized
/// UTC instant. Naive local times are rejected because the application cannot
/// infer a timezone without introducing host-dependent behavior.
pub fn parse_search_instant(value: &str) -> Option<SearchInstant> {
    let value = value.trim();
    let (date, time) = value.split_once(['T', ' '])?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > days_in_month {
        return None;
    }

    let (clock, offset_minutes) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0_i32)
    } else {
        let sign_at = time.rfind(['+', '-'])?;
        if sign_at == 0 {
            return None;
        }
        let (clock, offset) = time.split_at(sign_at);
        let (sign, digits) = offset.split_at(1);
        let (hours, minutes) = digits
            .split_once(':')
            .unwrap_or_else(|| digits.split_at_checked(2).unwrap_or((digits, "")));
        if minutes.is_empty() || hours.len() != 2 || minutes.len() != 2 {
            return None;
        }
        let hours: i32 = hours.parse().ok()?;
        let minutes: i32 = minutes.parse().ok()?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        let magnitude = hours * 60 + minutes;
        (clock, if sign == "+" { magnitude } else { -magnitude })
    };
    let (clock, mut fraction, has_fraction) = match clock.split_once('.') {
        Some((clock, fraction)) => (clock, fraction, true),
        None => (clock, "", false),
    };
    let (hour, minute, second) = match clock.split_once(':') {
        Some(_) => {
            let mut parts = clock.split(':');
            let hour: i64 = parts.next()?.parse().ok()?;
            let minute: i64 = parts.next()?.parse().ok()?;
            let second: i64 = parts.next()?.parse().ok()?;
            if parts.next().is_some() {
                return None;
            }
            (hour, minute, second)
        }
        None => {
            if clock.len() != 6 || !clock.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            (
                clock[0..2].parse().ok()?,
                clock[2..4].parse().ok()?,
                clock[4..6].parse().ok()?,
            )
        }
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    if has_fraction && (fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    if !has_fraction {
        fraction = "0";
    }
    let mut nanoseconds = 0_u32;
    for digit in fraction.bytes().take(9) {
        nanoseconds = nanoseconds * 10 + u32::from(digit - b'0');
    }
    if fraction.len() > 9 && fraction.bytes().skip(9).any(|digit| digit != b'0') {
        return None;
    }
    for _ in fraction.len().min(9)..9 {
        nanoseconds *= 10;
    }
    let days = days_from_civil(year, month, day)?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour.checked_mul(3_600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?
        .checked_sub(i64::from(offset_minutes) * 60)?;
    Some(SearchInstant {
        unix_seconds: seconds,
        nanosecond: nanoseconds,
    })
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    let year = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = (month + 9) % 12;
    let day_of_year = ((153 * shifted_month + 2) / 5 + day - 1) as i64;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146097 + day_of_era - 719468)
}

/// Resolve the CLI's compact duration syntax against the caller's injected
/// application clock. Absolute timestamps are handled by `parse_search_instant`.
pub fn parse_relative_search_instant(value: &str, now_ms: i64) -> Option<SearchInstant> {
    let value = value.trim();
    let (digits, unit) = value.split_at_checked(value.len().saturating_sub(1))?;
    let amount: i64 = digits.parse().ok()?;
    if amount <= 0 {
        return None;
    }
    let multiplier = match unit {
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 604_800_000,
        _ => return None,
    };
    let delta = amount.checked_mul(multiplier)?;
    Some(SearchInstant::from_unix_millis(now_ms.checked_sub(delta)?))
}

fn search_query_digest(
    query: &str,
    filters: &SearchFilters,
    facets: &SearchFacets,
    include_system: bool,
    group_by_session: bool,
) -> String {
    if filters.is_empty() && facets.is_default() && !include_system && !group_by_session {
        return cursor::digest_query(query);
    }
    let providers = filters
        .providers
        .iter()
        .map(|provider| provider.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let roles = filters
        .roles
        .iter()
        .map(|role| role.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let projects = filters.projects.join("\u{1f}");
    let exclude_projects = filters.exclude_projects.join("\u{1f}");
    let exclude_terms = filters.exclude_terms.join("\u{1f}");
    let since = filters
        .since
        .map(|instant| format!("{}:{}", instant.unix_seconds, instant.nanosecond))
        .unwrap_or_default();
    let until = filters
        .until
        .map(|instant| format!("{}:{}", instant.unix_seconds, instant.nanosecond))
        .unwrap_or_default();
    cursor::digest_query(&format!(
        "search-filter-v2\0{}\0providers={}\0roles={}\0since={}\0until={}\0projects={}\0exclude_projects={}\0exclude_terms={}\0include_system={}\0group_by_session={}\0facets={}",
        query,
        providers,
        roles,
        since,
        until,
        projects,
        exclude_projects,
        exclude_terms,
        include_system,
        group_by_session,
        facets.canonical_binding(),
    ))
}

/// R2 系统噪声判定：canonical message payload 的 `role` 字段为 system 或
/// developer（Codex 的 system/permission 层角色）即视为系统上下文。compaction
/// summary 在 parse 期已跳过（不产生消息）；AGENTS.md/skills/system prompt 以
/// 请求记录的 `system` 数组形式存在而非消息实体，故无需额外标记。payload 非
/// JSON 或无 role 字段（legacy）一律不判为噪声。
fn payload_role_is_system_noise(payload: Option<&[u8]>) -> bool {
    payload
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
        .and_then(|value| {
            value
                .get("role")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .is_some_and(|role| role == "system" || role == "developer")
}

/// 装配一条检索命中（R1/ADR-0008 + guidance）：从 payload 解析 `text` 摘要、
/// 填充归属会话、派生确定性 `why_matched` 与 `suggested_next_commands`。
/// payload 无 text（或非 JSON）→ text None；无 placement → session_id None。
fn assemble_search_hit(
    hit: &mut SearchHit,
    payload: Option<&[u8]>,
    session: Option<&StableId>,
    max_snippet_chars: usize,
    query_terms: &[String],
    context_lines: Option<usize>,
) -> bool {
    let full_text = payload.and_then(|bytes| {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return None;
        };
        value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    });
    // 证据装配期间全量 payload 仍可用：除规范 `text` 字段外，把整棵 JSON 值
    // 交给 guidance（string-leaves 源覆盖 Codex content blocks 等无顶层 text
    // 的 payload），显示前缀作最后一个兜底源（可能是截断 snippet，会漏掉前缀
    // 之后的真实命中——guidance design §2）。
    let payload_value =
        payload.and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok());
    let mut context_clipped = false;
    hit.text = full_text.as_deref().map(|text| match context_lines {
        Some(lines) => {
            let (snippet, clipped) =
                line_context_snippet(text, max_snippet_chars, query_terms, lines);
            context_clipped = clipped;
            snippet
        }
        None => snippet_around_match(text, max_snippet_chars, query_terms),
    });
    // 高亮偏移在**最终 snippet 上**求得，所以它天然与 `…` 标记、行窗裁剪对齐；
    // 绝不用原文偏移去指一个已经被裁过的字符串。
    hit.match_ranges = hit
        .text
        .as_deref()
        .map(|snippet| match_ranges_in(snippet, query_terms))
        .unwrap_or_default();
    // 保留 adapter 提供的 canonical session_id（Session 元数据命中自带归属
    // 会话）；否则才用 placement 解析的归属会话回填。metadata-only Session
    // 命中无 placement，session_of 返回 None，不得把既有值覆盖成 None。
    if hit.session_id.is_none() {
        hit.session_id = session.map(|s| s.as_str().to_string());
    }
    hit.why_matched = guidance::why_matched(
        query_terms,
        full_text.as_deref(),
        None,
        payload_value.as_ref(),
        hit.text.as_deref(),
    );
    hit.suggested_next_commands = guidance::suggested_next_commands(hit);
    context_clipped
}

/// Character offset of the earliest query-term occurrence, case-insensitively.
///
/// `to_lowercase` can change byte length, so the search and the returned index
/// are both in chars. `None` means no term is literally present — the hit came
/// from a payload field other than `text`, or from CJK bigram tokenisation that
/// does not align to a literal substring.
fn first_match_char(text: &str, query_terms: &[String]) -> Option<usize> {
    let lower: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let haystack: String = lower.iter().collect();
    query_terms
        .iter()
        .filter(|term| !term.is_empty())
        .filter_map(|term| {
            let needle = term.to_lowercase();
            haystack
                .find(&needle)
                .map(|byte| haystack[..byte].chars().count())
        })
        .min()
}

/// Highlight offsets for `snippet`: the earliest occurrence of each distinct
/// query term, as half-open `[start, end)` char ranges, sorted and merged.
///
/// Bounded by construction — `query_terms` is already capped at
/// `guidance::MAX_WHY_MATCHED`, so there is no list to cut silently. Ranges are
/// derived from the snippet actually returned, never from the full body, so an
/// offset can never point past the end of the string a consumer receives.
///
/// Reported in chars rather than bytes so the unit matches `max_snippet_chars`
/// and stays meaningful to consumers that do not index UTF-8 by byte.
fn match_ranges_in(snippet: &str, query_terms: &[String]) -> Vec<(usize, usize)> {
    // `to_lowercase` may map one char to several, which would shift every offset
    // after it. Fold per char and keep a char-to-char index map instead.
    let folded: Vec<String> = snippet
        .chars()
        .map(|c| c.to_lowercase().collect::<String>())
        .collect();
    let haystack: String = folded.concat();
    // Byte offset in `haystack` → char index in `snippet`.
    let mut byte_to_char: Vec<(usize, usize)> = Vec::with_capacity(folded.len());
    let mut byte = 0usize;
    for (index, piece) in folded.iter().enumerate() {
        byte_to_char.push((byte, index));
        byte += piece.len();
    }
    let char_at = |byte_offset: usize| -> Option<usize> {
        byte_to_char
            .binary_search_by_key(&byte_offset, |(b, _)| *b)
            .ok()
            .map(|slot| byte_to_char[slot].1)
    };

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for term in query_terms.iter().filter(|term| !term.is_empty()) {
        let needle = term.to_lowercase();
        let Some(found) = haystack.find(&needle) else {
            continue;
        };
        // A term may start mid-way through a multi-char case fold; such a match
        // has no honest char boundary to point at, so it is skipped rather than
        // rounded to a neighbouring char.
        let Some(start) = char_at(found) else {
            continue;
        };
        let Some(end) = char_at(found + needle.len())
            .or_else(|| (found + needle.len() == haystack.len()).then_some(byte_to_char.len()))
        else {
            continue;
        };
        if end > start {
            ranges.push((start, end));
        }
    }
    ranges.sort_unstable();
    // Merge overlapping/touching ranges so a consumer can emphasise each range
    // independently without double-wrapping a character.
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// A contiguous line window around the match: the matched line plus
/// `context_lines` whole lines on each side, grep's `-C N` (M3-9).
///
/// Returns `(snippet, clipped)`. The window is a **contiguous region of the
/// source** — line separators included, nothing reordered or stitched — because
/// the value claim of this tool is that it shows real transcript bytes.
///
/// `clipped` is true when the requested window did not fit `max_snippet_chars`
/// and had to be narrowed. The caller reports that as `outcome: partial`: an
/// explicitly requested region delivered only in part is a partial result, not
/// a success with an ellipsis in it.
///
/// With no literally locatable term there is no line to centre on, so this
/// falls back to [`snippet_around_match`] — the same fallback the default path
/// uses, and never worse than it.
fn line_context_snippet(
    text: &str,
    max_chars: usize,
    query_terms: &[String],
    context_lines: usize,
) -> (String, bool) {
    let Some(at) = first_match_char(text, query_terms) else {
        return (snippet_around_match(text, max_chars, query_terms), false);
    };
    // Line starts in char offsets. `\r` stays inside the line: it is a source
    // byte, and the human renderer already folds control chars for display.
    let chars: Vec<char> = text.chars().collect();
    let mut line_starts: Vec<usize> = vec![0];
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            line_starts.push(index + 1);
        }
    }
    let hit_line = line_starts.partition_point(|start| *start <= at).max(1) - 1;
    let first_line = hit_line.saturating_sub(context_lines);
    let last_line = (hit_line + context_lines).min(line_starts.len() - 1);
    let start = line_starts[first_line];
    let end = line_starts
        .get(last_line + 1)
        .map(|next| next - 1) // drop the trailing newline of the last kept line
        .unwrap_or(chars.len());

    if end.saturating_sub(start) <= max_chars {
        return (chars[start..end].iter().collect(), false);
    }
    // The window overflows the snippet budget. Narrow it around the match
    // instead of taking its head, so the match stays visible, and tell the
    // caller the requested window was not delivered in full. A sub-window of a
    // contiguous region is still contiguous, so the evidence stays verbatim.
    let region: String = chars[start..end].iter().collect();
    (snippet_around_match(&region, max_chars, query_terms), true)
}

/// A bounded snippet that contains the first matching term, not a blind prefix.
///
/// The snippet used to be `text.chars().take(n)`, so a match past character `n`
/// was simply invisible: a hit whose term appeared at character 216 rendered as
/// the first 120 characters of unrelated lead-in, and the user could not tell
/// why the hit matched at all (M3-9). Answering that required two more commands.
///
/// The window is centred on the first term occurrence and clamped to the text,
/// so a match near either end still yields a full-width window. Elision markers
/// are added only on the side actually cut, which keeps a short text
/// byte-identical to the old prefix behaviour.
///
/// **`max_chars` bounds the returned string, markers included.** The budget is a
/// response-size contract (`ResponseBudget::max_snippet_chars`), so spending two
/// characters on markers has to come out of the window rather than be added on
/// top — an e2e test asserts the returned length never exceeds the budget.
///
/// Matching is case-insensitive to mirror the FTS behaviour that produced the hit.
/// When no term is located — the match came from a payload field other than
/// `text`, or from CJK bigram tokenisation that does not align to a literal
/// substring — this falls back to the prefix, which is the old behaviour and is
/// never worse.
fn snippet_around_match(text: &str, max_chars: usize, query_terms: &[String]) -> String {
    let total: usize = text.chars().count();
    if total <= max_chars || max_chars == 0 {
        return text.chars().take(max_chars).collect();
    }

    // Character offset of the earliest term hit. `to_lowercase` can change byte
    // length, so search and index in chars rather than bytes throughout.
    let Some(at) = first_match_char(text, query_terms) else {
        return text.chars().take(max_chars).collect();
    };

    // Reserve budget for the markers this window will actually need. A window
    // that starts at 0 needs no leading marker, and one that reaches the end
    // needs no trailing marker, so decide the two independently.
    let leading = at > max_chars / 2;
    // Provisional body width assuming both markers, then refine: dropping the
    // leading marker frees a character for content.
    let body = max_chars.saturating_sub(usize::from(leading) + 1).max(1);
    let half = body / 2;
    let start = if leading { at.saturating_sub(half) } else { 0 };
    let start = start.min(total.saturating_sub(body));
    let end = (start + body).min(total);

    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(text.chars().skip(start).take(end - start));
    if end < total {
        out.push('…');
    }
    // Belt and braces: never exceed the caller's budget even if the arithmetic
    // above is refined later.
    if out.chars().count() > max_chars {
        return out.chars().take(max_chars).collect();
    }
    out
}

/// Flip the response-level truncation flag when a requested `--context N` line
/// window did not fit `max_snippet_chars` (M3-9).
///
/// The item and byte gates win when they also fired: they decided how many hits
/// came back, and raising `max_snippet_chars` would not bring those hits back.
/// Only called with hits that survived clamping, so the report never describes a
/// hit the caller cannot see.
fn note_context_clipped(truncation: &mut Truncation, clipped: bool) {
    if !clipped {
        return;
    }
    truncation.truncated = true;
    if truncation.reason.is_none() {
        truncation.reason = Some(budget::TRUNCATION_MAX_SNIPPET_CHARS.to_string());
    }
}

/// 检索命中在 `max_response_bytes` 闸内的序列化字节估算（与 CLI 渲染对齐）：
/// id + session_id + text + guidance；`occurrences` 仅当 >1（归并模式）时计入，
/// 与序列化器"occurrences == 1 时省略该键"的约定一致。
fn search_hit_charge(hit: &SearchHit) -> usize {
    let why_matched_len = if hit.why_matched.is_empty() {
        0
    } else {
        hit.why_matched
            .iter()
            .map(|value| json_string_len(value))
            .sum::<usize>()
            + hit.why_matched.len()
            + 15
    };
    let suggested_len = if hit.suggested_next_commands.is_empty() {
        0
    } else {
        hit.suggested_next_commands
            .iter()
            .map(|value| json_string_len(value))
            .sum::<usize>()
            + hit.suggested_next_commands.len()
            + 27
    };
    let occurrences_len = if hit.occurrences > 1 {
        hit.occurrences.to_string().len() + 14
    } else {
        0
    };
    // `match_ranges` 是渲染产物，同样受字节闸约束（snippet 不是免费内容，
    // 它的高亮偏移也不是）。`[[12,18],[40,46]]` 形状：两个十进制数 + 逗号 +
    // 方括号，键名与外层括号 16 字节。
    let match_ranges_len = if hit.match_ranges.is_empty() {
        0
    } else {
        hit.match_ranges
            .iter()
            .map(|(start, end)| start.to_string().len() + end.to_string().len() + 4)
            .sum::<usize>()
            + hit.match_ranges.len()
            + 16
    };
    json_string_len(hit.id.as_str())
        + hit
            .session_id
            .as_ref()
            .map_or(0, |s| json_string_len(s) + 14)
        + hit.text.as_ref().map_or(0, |s| json_string_len(s) + 8)
        + why_matched_len
        + suggested_len
        + occurrences_len
        + match_ranges_len
        + RESUME_AVAILABLE_FIELD_BYTES
        + 32
}

/// 系统时钟（Unix 毫秒）。[`App::new`] 的默认时钟；测试经 [`App::with_clock`] 注入固定值。
fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone)]
struct ContextOccurrence {
    message: ContextMessage,
    evidence: EvidenceSpanDto,
    role: Role,
    source_document_id: String,
    estimated_bytes: usize,
}

struct MessageOccurrence {
    message: ContextMessage,
    estimated_bytes: usize,
    is_anchor: bool,
    payload_truncated: bool,
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
        Role::Developer => "developer",
        Role::Tool => "tool",
    }
}

fn json_escaped_char_len(value: char) -> usize {
    match value {
        '"' | '\\' | '\u{0008}' | '\t' | '\n' | '\u{000c}' | '\r' => 2,
        '\u{0000}'..='\u{001f}' => 6,
        _ => value.len_utf8(),
    }
}

fn project_anchor_payload(
    message: &mut ContextMessage,
    role: Role,
    full_text: &str,
    max_bytes: usize,
) -> Result<bool, budget::BudgetError> {
    if context_message_bytes(message) <= max_bytes {
        return Ok(false);
    }

    let preserves_other_fields = message
        .payload
        .as_object_mut()
        .and_then(|payload| payload.get_mut("text"))
        .is_some_and(|text| {
            if text.is_string() {
                *text = serde_json::Value::String(String::new());
                true
            } else {
                false
            }
        });
    if !preserves_other_fields || context_message_bytes(message) > max_bytes {
        message.payload = serde_json::json!({
            "role": role_name(role),
            "text": "",
        });
    }

    let base_bytes = context_message_bytes(message);
    if base_bytes > max_bytes {
        return Err(budget::BudgetError::TooSmall(
            "max_response_bytes cannot fit the message anchor identity".into(),
        ));
    }

    let mut text = String::new();
    let mut remaining = max_bytes - base_bytes;
    for value in full_text.chars() {
        let escaped_len = json_escaped_char_len(value);
        if escaped_len > remaining {
            break;
        }
        text.push(value);
        remaining -= escaped_len;
    }
    message
        .payload
        .as_object_mut()
        .expect("projected message payload is an object")
        .insert("text".into(), serde_json::Value::String(text));
    debug_assert!(context_message_bytes(message) <= max_bytes);
    Ok(true)
}

fn structural_metadata_reserve(session_id: &StableId, requested_level: ContextLevel) -> usize {
    match requested_level {
        ContextLevel::Raw => 0,
        ContextLevel::Talks | ContextLevel::Sessions => {
            STRUCTURAL_METADATA_RESERVE_BYTES.saturating_add(session_id.as_str().len())
        }
    }
}

fn occurrence_response_bytes(
    occurrence: &ContextOccurrence,
    requested_level: ContextLevel,
    has_user_anchor: &mut bool,
    references: &mut BTreeSet<String>,
) -> usize {
    let derived_copy = match requested_level {
        ContextLevel::Raw => false,
        ContextLevel::Talks => {
            if occurrence.role == Role::User {
                *has_user_anchor = true;
                true
            } else {
                *has_user_anchor && matches!(occurrence.role, Role::Assistant | Role::Tool)
            }
        }
        ContextLevel::Sessions => occurrence.role == Role::User && !*has_user_anchor,
    };
    if occurrence.role == Role::User {
        *has_user_anchor = true;
    }
    occurrence
        .estimated_bytes
        .saturating_add(if derived_copy {
            occurrence.estimated_bytes
        } else {
            0
        })
        .saturating_add(
            if requested_level == ContextLevel::Sessions
                && references.insert(occurrence.source_document_id.clone())
            {
                occurrence.source_document_id.len().saturating_add(4)
            } else {
                0
            },
        )
}

fn clamp_context_occurrences(
    occurrences: Vec<ContextOccurrence>,
    level: ContextLevel,
    max_messages: usize,
    max_bytes: usize,
) -> (Vec<ContextOccurrence>, Truncation, usize) {
    let mut has_user_anchor = false;
    let mut references = BTreeSet::new();
    let costed = occurrences
        .into_iter()
        .map(|occurrence| {
            let cost = occurrence_response_bytes(
                &occurrence,
                level,
                &mut has_user_anchor,
                &mut references,
            );
            (occurrence, cost)
        })
        .collect();
    let (kept, mut truncation, consumed) =
        budget::clamp_items(costed, max_messages, max_bytes, |(_, cost)| *cost);
    if truncation.reason.as_deref() == Some(budget::TRUNCATION_MAX_ITEMS) {
        truncation.reason = Some(budget::TRUNCATION_MAX_MESSAGES.to_string());
    }
    (
        kept.into_iter().map(|(occurrence, _)| occurrence).collect(),
        truncation,
        consumed,
    )
}

fn talks_of(occurrences: &[&ContextOccurrence]) -> Vec<ContextTalk> {
    let mut talks = Vec::new();
    for occurrence in occurrences {
        if occurrence.role == Role::User {
            talks.push(ContextTalk {
                user_message: occurrence.message.clone(),
                following_messages: Vec::new(),
            });
        } else if matches!(occurrence.role, Role::Assistant | Role::Tool)
            && let Some(talk) = talks.last_mut()
        {
            talk.following_messages.push(occurrence.message.clone());
        }
    }
    talks
}

fn summary_of(occurrences: &[&ContextOccurrence]) -> ContextSessionSummary {
    let first_user_message = occurrences
        .iter()
        .find(|occurrence| occurrence.role == Role::User)
        .map(|occurrence| occurrence.message.clone());
    let turn_count = occurrences
        .iter()
        .filter(|occurrence| occurrence.role == Role::User)
        .count();
    let mut file_references = Vec::new();
    for occurrence in occurrences {
        if !file_references.contains(&occurrence.source_document_id) {
            file_references.push(occurrence.source_document_id.clone());
        }
    }
    ContextSessionSummary {
        first_user_message,
        message_count: occurrences.len(),
        turn_count,
        file_references,
    }
}

fn available_level(
    requested_level: ContextLevel,
    occurrences: &[ContextOccurrence],
) -> ContextLevel {
    match requested_level {
        ContextLevel::Raw => ContextLevel::Raw,
        ContextLevel::Talks | ContextLevel::Sessions
            if occurrences
                .iter()
                .any(|occurrence| occurrence.role == Role::User) =>
        {
            requested_level
        }
        ContextLevel::Talks | ContextLevel::Sessions => ContextLevel::Raw,
    }
}

fn assemble_level(
    requested_level: ContextLevel,
    kept_occurrences: &[ContextOccurrence],
) -> (
    ContextLevel,
    Vec<ContextTalk>,
    Option<ContextSessionSummary>,
) {
    let occurrences: Vec<&ContextOccurrence> = kept_occurrences.iter().collect();
    match requested_level {
        ContextLevel::Raw => (ContextLevel::Raw, Vec::new(), None),
        ContextLevel::Talks => {
            let talks = talks_of(&occurrences);
            if talks.is_empty() {
                (ContextLevel::Raw, Vec::new(), None)
            } else {
                (ContextLevel::Talks, talks, None)
            }
        }
        ContextLevel::Sessions => {
            let talks = talks_of(&occurrences);
            if talks.is_empty() {
                (ContextLevel::Raw, Vec::new(), None)
            } else {
                (
                    ContextLevel::Sessions,
                    Vec::new(),
                    Some(summary_of(&occurrences)),
                )
            }
        }
    }
}

fn build_hint(
    session_id: &StableId,
    requested_level: ContextLevel,
    effective_level: ContextLevel,
) -> Option<ContextHint> {
    let next = match (requested_level, effective_level) {
        (ContextLevel::Sessions, ContextLevel::Sessions) => ContextLevel::Talks,
        (ContextLevel::Sessions, ContextLevel::Talks)
        | (ContextLevel::Talks, ContextLevel::Talks) => ContextLevel::Raw,
        _ => return None,
    };
    Some(ContextHint {
        command: "get_session_context".to_string(),
        session_id: session_id.as_str().to_string(),
        level: next,
    })
}

fn structural_fields_bytes(
    talks: &[ContextTalk],
    summary: &Option<ContextSessionSummary>,
    hint: &Option<ContextHint>,
) -> usize {
    let mut bytes = 128usize;
    if let Some(hint) = hint {
        bytes = bytes
            .saturating_add(hint.command.len())
            .saturating_add(hint.session_id.len())
            .saturating_add(hint.level.as_str().len())
            .saturating_add(48);
    }
    for talk in talks {
        bytes = bytes
            .saturating_add(context_message_bytes(&talk.user_message))
            .saturating_add(32);
        for message in &talk.following_messages {
            bytes = bytes
                .saturating_add(context_message_bytes(message))
                .saturating_add(8);
        }
    }
    if let Some(summary) = summary {
        bytes = bytes.saturating_add(96);
        if let Some(message) = &summary.first_user_message {
            bytes = bytes.saturating_add(context_message_bytes(message));
        }
        for reference in &summary.file_references {
            bytes = bytes.saturating_add(reference.len()).saturating_add(4);
        }
    }
    bytes
}

fn context_message_bytes(message: &ContextMessage) -> usize {
    message
        .id
        .len()
        .saturating_add(message.placement_id.len())
        .saturating_add(message.message_id.len())
        .saturating_add(message.payload.to_string().len())
        .saturating_add(48)
}

fn message_occurrence_bytes(message: &ContextMessage) -> usize {
    context_message_bytes(message).saturating_add(48)
}

fn clamp_message_window(
    occurrences: Vec<MessageOccurrence>,
    budget: &ResponseBudget,
    max_bytes: usize,
) -> (Vec<MessageOccurrence>, Truncation) {
    let total = occurrences.len();
    let anchor_index = occurrences
        .iter()
        .position(|occurrence| occurrence.is_anchor)
        .expect("message windows always contain their anchor");
    let anchor_cost = occurrences[anchor_index].estimated_bytes;
    let anchor_payload_truncated = occurrences[anchor_index].payload_truncated;
    let mut start = anchor_index;
    let mut end = anchor_index + 1;
    let mut consumed = anchor_cost;
    let mut byte_limited = anchor_payload_truncated || anchor_cost > max_bytes;
    let mut prefer_left = true;

    if !byte_limited {
        loop {
            let left = start.checked_sub(1);
            let right = (end < total).then_some(end);
            if left.is_none() && right.is_none() {
                break;
            }
            if end - start >= budget.max_items {
                break;
            }
            let next = match (left, right) {
                (Some(left), Some(right)) => {
                    let next = if prefer_left { left } else { right };
                    prefer_left = !prefer_left;
                    next
                }
                (Some(left), None) => left,
                (None, Some(right)) => right,
                (None, None) => unreachable!(),
            };
            let cost = occurrences[next].estimated_bytes;
            if consumed.saturating_add(cost) > max_bytes {
                byte_limited = true;
                break;
            }
            consumed += cost;
            if next < start {
                start = next;
            } else {
                end = next + 1;
            }
        }
    }

    let kept_count = end - start;
    let truncated = kept_count < total || anchor_payload_truncated || anchor_cost > max_bytes;
    let reason = if byte_limited {
        Some(budget::TRUNCATION_MAX_RESPONSE_BYTES.to_string())
    } else if kept_count < total {
        Some(budget::TRUNCATION_MAX_ITEMS.to_string())
    } else {
        None
    };
    let kept = occurrences
        .into_iter()
        .skip(start)
        .take(kept_count)
        .collect();
    (kept, Truncation { truncated, reason })
}

/// 用例执行器：绑定所需端口，串起领域校验与端口调用。
///
/// 泛型而非 trait object——前端在构造期决定后端实现，零动态分发开销。
/// 时钟以 fn 指针注入：cursor 的发行/校验都不读环境时间（可测确定性）。
pub struct App<
    C: CatalogStore + ContextGraphStore,
    S: SearchIndex,
    R: ResumeClaimsStore = NoResumeClaims,
    M: SemanticIndex = NoSemanticIndex,
> {
    catalog: C,
    index: S,
    resume: R,
    semantic: M,
    clock_ms: fn() -> i64,
}

impl<C: CatalogStore + ContextGraphStore, S: SearchIndex, R: ResumeClaimsStore>
    App<C, S, R, NoSemanticIndex>
{
    /// 绑定 Resume 声明存储的构造器（ADR-0009）：生产路径（CLI/Robot/MCP）
    /// 用同一 SqliteStore 实例填充 catalog/index/resume 三个槽；semantic 为
    /// 占位 `NoSemanticIndex`（semantic 请求显式降级 lexical_fallback）。
    pub fn with_resume(catalog: C, index: S, resume: R) -> Self {
        Self {
            catalog,
            index,
            resume,
            semantic: NoSemanticIndex,
            clock_ms: system_now_ms,
        }
    }

    /// 固定时钟 + Resume 声明存储（测试用）。
    pub fn with_resume_and_clock(catalog: C, index: S, resume: R, clock_ms: fn() -> i64) -> Self {
        Self {
            catalog,
            index,
            resume,
            semantic: NoSemanticIndex,
            clock_ms,
        }
    }
}

/// 注入语义索引的构造器（#3）。
impl<C: CatalogStore + ContextGraphStore, S: SearchIndex, R: ResumeClaimsStore, M: SemanticIndex>
    App<C, S, R, M>
{
    pub fn with_resume_semantic(catalog: C, index: S, resume: R, semantic: M) -> Self {
        Self {
            catalog,
            index,
            resume,
            semantic,
            clock_ms: system_now_ms,
        }
    }

    pub fn with_resume_semantic_and_clock(
        catalog: C,
        index: S,
        resume: R,
        semantic: M,
        clock_ms: fn() -> i64,
    ) -> Self {
        Self {
            catalog,
            index,
            resume,
            semantic,
            clock_ms,
        }
    }
}

/// `NoResumeClaims` 固定槽的构造器：函数级泛型默认参数不生效（Rust 限制），
/// 把这些不携带 Resume 实现的构造器放进专属 impl，既有 `App::new` /
/// `App::with_clock` 调用点零改动继续编译。
impl<C: CatalogStore + ContextGraphStore, S: SearchIndex>
    App<C, S, NoResumeClaims, NoSemanticIndex>
{
    pub fn new(catalog: C, index: S) -> Self {
        Self {
            catalog,
            index,
            resume: NoResumeClaims,
            semantic: NoSemanticIndex,
            clock_ms: system_now_ms,
        }
    }

    /// 注入固定时钟的构造器（测试用）；生产路径一律 [`App::new`]。
    pub fn with_clock(catalog: C, index: S, clock_ms: fn() -> i64) -> Self {
        Self {
            catalog,
            index,
            resume: NoResumeClaims,
            semantic: NoSemanticIndex,
            clock_ms,
        }
    }
}

impl<C: CatalogStore + ContextGraphStore, S: SearchIndex, R: ResumeClaimsStore, M: SemanticIndex>
    App<C, S, R, M>
{
    /// Read the injected application clock. Relative search filters use this
    /// value so every frontend shares the same time source.
    pub fn now_ms(&self) -> i64 {
        (self.clock_ms)()
    }

    /// 解析续读偏移：无令牌即第一页（offset 0）；有令牌则完整校验
    /// （结构/摘要/过期/generation/查询与排序绑定），失败显式报错，绝不静默回第一页。
    fn resolve_offset(
        &self,
        token: Option<&str>,
        active_generation: u64,
        query_digest: &str,
        sort_digest: &str,
        result_set: Option<&str>,
    ) -> Result<u64, AppError> {
        match token {
            None => Ok(0),
            Some(t) => {
                let claims = cursor::verify(
                    t,
                    &cursor::CursorExpectations {
                        now_ms: (self.clock_ms)(),
                        active_generation,
                        query_digest: query_digest.to_string(),
                        sort_digest: sort_digest.to_string(),
                        result_set: result_set.map(str::to_string),
                    },
                )?;
                Ok(claims.offset)
            }
        }
    }

    /// 还有后续页时发行续读令牌；`offset_next` 是钉住排序内的下一读取位置。
    fn issue_cursor(
        &self,
        has_more: bool,
        generation: u64,
        query_digest: &str,
        sort_digest: &str,
        result_set: Option<&str>,
        offset_next: u64,
    ) -> Option<String> {
        if !has_more {
            return None;
        }
        let now = (self.clock_ms)();
        Some(
            cursor::issue(&cursor::CursorClaims {
                contract_major: cursor::SUPPORTED_CONTRACT_MAJOR,
                generation,
                issued_at_ms: now,
                expires_at_ms: now + cursor::DEFAULT_TTL_MS,
                query_digest: query_digest.to_string(),
                sort_digest: sort_digest.to_string(),
                result_set: result_set.map(str::to_string),
                offset: offset_next,
            })
            .into_string(),
        )
    }

    /// Resume 可用性批量装配（ADR-0009）：收集本页所有携带 `session_id` 的
    /// 命中，一次 `resume_of` 解析（分块 IN，无 N+1），按 wire id 映射回
    /// `hit.resume_available`。`session_id` 缺失或 wire 无效的命中保持 `false`
    /// ——可用性缺失绝不影响历史可检索。
    fn assemble_resume_availability(&self, hits: &mut [SearchHit]) -> PortResult<()> {
        if hits.is_empty() {
            return Ok(());
        }
        let ids: Vec<StableId> = hits
            .iter()
            .filter_map(|hit| {
                hit.session_id
                    .as_deref()
                    .and_then(StableId::from_wire)
                    .filter(|id| id.kind() == IdKind::Session)
            })
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        let metadata = self.resume.resume_of(&ids)?;
        let availability: HashMap<&str, bool> = metadata
            .iter()
            .map(|meta| (meta.session_id.as_str(), meta.resume_available))
            .collect();
        for hit in hits.iter_mut() {
            if let Some(wire) = hit.session_id.as_deref()
                && let Some(available) = availability.get(wire)
            {
                hit.resume_available = *available;
            }
        }
        Ok(())
    }

    /// 执行一个应用请求。校验错误保持 Domain 分类，端口错误保持 Port 分类，
    /// cursor/budget 错误保持各自分类（protocol 层有专属 canonical code）。
    pub fn handle(&self, req: AppRequest) -> Result<AppResponse, AppError> {
        match req {
            AppRequest::Search {
                query,
                mut filters,
                facets,
                limit,
                cursor: token,
                budget,
                include_system,
                group_by_session,
                mode,
                query_embedding,
                context_lines,
            } => {
                if limit == 0 {
                    return Err(DomainError::InvalidRequest("limit must be > 0".into()).into());
                }
                // R4.2（ADR-0003）：NUL/C0/C1 控制字符在 Application 边界拒绝为
                // invalid_request，绝不清除式净化（删除会拼接 token）。必须发生在
                // 任何索引查询之前。
                if query.chars().any(char::is_control) {
                    return Err(DomainError::InvalidRequest(
                        "query contains control characters".into(),
                    )
                    .into());
                }
                if query.trim().is_empty() {
                    return Err(
                        DomainError::InvalidRequest("query must not be empty".into()).into(),
                    );
                }
                filters.providers.sort_unstable();
                filters.providers.dedup();
                // Roles and exclusion terms are sets: a caller that names the
                // same one twice, or names two in the other order, must get the
                // same cursor digest as well as the same rows.
                filters.roles.sort_unstable();
                filters.roles.dedup();
                for term in &filters.exclude_terms {
                    if term.chars().any(char::is_control) {
                        return Err(DomainError::InvalidRequest(
                            "exclusion term contains control characters".into(),
                        )
                        .into());
                    }
                    if term.trim().is_empty() {
                        return Err(DomainError::InvalidRequest(
                            "exclusion term must not be empty".into(),
                        )
                        .into());
                    }
                }
                filters.exclude_terms.sort();
                filters.exclude_terms.dedup();
                if let (Some(since), Some(until)) = (filters.since, filters.until)
                    && since >= until
                {
                    return Err(DomainError::InvalidRequest(
                        "since must be earlier than until".into(),
                    )
                    .into());
                }
                budget.validate().map_err(AppError::from)?;
                let generation = self.catalog.active_generation()?;
                let query_digest = search_query_digest(
                    &query,
                    &filters,
                    &facets,
                    include_system,
                    group_by_session,
                );
                let offset = self.resolve_offset(
                    token.as_deref(),
                    generation,
                    &query_digest,
                    SORT_SCORE_DESC,
                    None,
                )?;

                // 分页模型：钉住排序（bm25 + id tiebreak 全序）内的 offset 续读。
                // 端口无 offset 参数——超取 offset+page+1（+1 作 has_more 哨兵）后
                // 切片。grouped 模式把扫描窗放大 GROUP_SCAN_FACTOR 倍（仍封顶），
                // 让 occurrences 覆盖更有意义的同会话命中样本。
                let page = limit.min(budget.max_items);
                let fetch = offset
                    .saturating_add(page as u64)
                    .saturating_add(1)
                    .min(MAX_FETCH_WINDOW);
                let scan = if group_by_session {
                    fetch
                        .saturating_mul(GROUP_SCAN_FACTOR)
                        .min(MAX_FETCH_WINDOW)
                } else {
                    fetch
                };
                // 检索模式（#3）：semantic/hybrid 需要已就绪的语义索引与调用方
                // 提供的查询向量；任一缺失时显式降级为 lexical_fallback + warning
                // （PRD Q54：禁止静默切换）。
                let (mut scanned, fallback_warning) = if mode == RetrievalMode::Lexical {
                    (
                        self.index.query_faceted(
                            SearchQuery {
                                text: &query,
                                filters: &filters,
                            },
                            scan as usize,
                            &facets,
                        )?,
                        None,
                    )
                } else if self.semantic.is_ready()
                    && let Some(query_embedding) = query_embedding.as_deref()
                {
                    let semantic_hits = self
                        .semantic
                        .query_semantic(query_embedding, scan as usize)?;
                    if mode == RetrievalMode::Semantic {
                        (semantic_hits, None)
                    } else {
                        let lexical_hits = self.index.query_faceted(
                            SearchQuery {
                                text: &query,
                                filters: &filters,
                            },
                            scan as usize,
                            &facets,
                        )?;
                        (hybrid::fuse(&lexical_hits, &semantic_hits), None)
                    }
                } else {
                    let warning = Some(format!(
                        "semantic search unavailable (mode {}); fell back to lexical",
                        mode.as_str()
                    ));
                    (
                        self.index.query_faceted(
                            SearchQuery {
                                text: &query,
                                filters: &filters,
                            },
                            scan as usize,
                            &facets,
                        )?,
                        warning,
                    )
                };

                // 生效检索模式（#3）：请求 semantic/hybrid 而索引未就绪 → 降级
                // lexical_fallback，warning 已随 fallback_warning 返回。
                let response_mode = if fallback_warning.is_some() {
                    RetrievalMode::LexicalFallback
                } else {
                    mode
                };
                let response_warning = fallback_warning.clone();

                // D11（宽容但报数）：时间窗谓词下推成 `sort_key(timestamp) >= ?`，
                // SQL 里 NULL 对每个比较都为假——无 per-message 时间戳的记录被
                // 静默排除。真查一次同一非时间谓词下的 NULL 计数，让调用方知道
                // 漏了多少；绝不估算、不抽样，也不查忽略其它 filter 的整表计数。
                // 纯 semantic 路径不经 lexical 下推（`query_semantic` 不吃
                // filters），此时无"时间窗排除"事实可报，计数保持 0。
                let time_filter_excluded = if response_mode != RetrievalMode::Semantic
                    && (filters.since.is_some() || filters.until.is_some())
                {
                    self.index.count_time_filter_excluded(
                        SearchQuery {
                            text: &query,
                            filters: &filters,
                        },
                        &facets,
                    )?
                } else {
                    0
                };

                // R2 系统噪声默认排除：role=system/developer 的命中不进入结果，
                // `include_system` 显式恢复。过滤先于 offset 切片，cursor 位置因此
                // 指向"非系统"序列。判定需整窗 payload（分块批量取，无 N+1）；扫描
                // 窗内系统噪声饱和时可能提前终止分页（边界行为，见 GROUP_SCAN_FACTOR）。
                //
                // 显式 `filters.roles` 关掉这层默认策略：它是一条**已下推到 SQL**
                // 的白名单，扫描窗里只剩被点名的那些 role。再叠一层默认噪声过滤会
                // 让 `--role system` 恒定返回空——用户点名要的东西被一条他没要求的
                // 默认规则悄悄减掉。跳过这里绝不会放宽结果集：SQL 谓词已经把范围
                // 收在白名单内。
                if !include_system && filters.roles.is_empty() {
                    let scanned_ids: Vec<StableId> =
                        scanned.iter().map(|hit| hit.id.clone()).collect();
                    let scanned_payloads = self.catalog.get_many(&scanned_ids)?;
                    scanned = scanned
                        .into_iter()
                        .zip(scanned_payloads)
                        .filter(|(_, payload)| !payload_role_is_system_noise(payload.1.as_deref()))
                        .map(|(hit, _)| hit)
                        .collect();
                }

                // R3 按会话归并：整窗装配后每会话只保留最高分命中（钉住顺序中的
                // 首个），occurrences 为该会话在扫描窗内的命中数；offset 语义为会话
                // 组偏移。非归并路径保持逐命中分页不变。
                if group_by_session {
                    let ids: Vec<StableId> = scanned.iter().map(|hit| hit.id.clone()).collect();
                    let payloads = self.catalog.get_many(&ids)?;
                    let sessions = self.catalog.session_of(&ids)?;
                    let max_snippet_chars = budget.max_snippet_chars;
                    let query_terms = guidance::literal_terms(&query);
                    // 命中与"它的行窗是否被预算裁过"成对流动：clamp 之后才知道哪些
                    // 命中留在响应里，而截断只应报**留下来的**那些的事实。
                    let mut assembled: Vec<(SearchHit, bool)> =
                        scanned.into_iter().map(|hit| (hit, false)).collect();
                    for ((hit, clipped), ((_id, payload), (_mid, session))) in
                        assembled.iter_mut().zip(payloads.into_iter().zip(sessions))
                    {
                        *clipped = assemble_search_hit(
                            hit,
                            payload.as_deref(),
                            session.as_ref(),
                            max_snippet_chars,
                            &query_terms,
                            context_lines,
                        );
                    }
                    let mut grouped: Vec<(SearchHit, bool)> = Vec::new();
                    let mut group_index: HashMap<String, usize> = HashMap::new();
                    for (hit, clipped) in assembled {
                        let key = hit
                            .session_id
                            .clone()
                            .unwrap_or_else(|| hit.id.as_str().to_string());
                        match group_index.get(&key).copied() {
                            Some(index) => grouped[index].0.occurrences += 1,
                            None => {
                                group_index.insert(key, grouped.len());
                                grouped.push((hit, clipped));
                            }
                        }
                    }
                    let grouped_kept: Vec<(SearchHit, bool)> = grouped
                        .into_iter()
                        .skip(usize::try_from(offset).unwrap_or(usize::MAX))
                        .take(page + 1)
                        .collect();
                    let has_more = grouped_kept.len() > page;
                    let slice: Vec<(SearchHit, bool)> =
                        grouped_kept.into_iter().take(page).collect();
                    let net_bytes = budget
                        .max_response_bytes
                        .saturating_sub(ENVELOPE_RESERVE_BYTES);
                    let (kept, mut truncation, _) =
                        budget::clamp_items(slice, page, net_bytes, |(hit, _)| {
                            search_hit_charge(hit)
                        });
                    let context_clipped = kept.iter().any(|(_, clipped)| *clipped);
                    let mut hits: Vec<SearchHit> = kept.into_iter().map(|(hit, _)| hit).collect();
                    note_context_clipped(&mut truncation, context_clipped);
                    // Resume 可用性（ADR-0009）：只对保留的命中批量解析一次（无 N+1）。
                    self.assemble_resume_availability(&mut hits)?;
                    let consumed = offset + hits.len() as u64;
                    // 与逐命中/List 分支相同的守卫：字节 clamp 把本页清空（首条组
                    // 超预算）时 cursor 停在原 offset，must 终止分页而非死循环。
                    let has_more = has_more && !hits.is_empty();
                    let next_cursor = self.issue_cursor(
                        has_more,
                        generation,
                        &query_digest,
                        SORT_SCORE_DESC,
                        None,
                        consumed,
                    );
                    return Ok(AppResponse::Search {
                        hits,
                        next_cursor,
                        generation,
                        truncation,
                        retrieval_mode: response_mode,
                        fallback_warning: response_warning,
                        time_filter_excluded,
                        context_lines,
                    });
                }

                let scanned_len = scanned.len() as u64;
                let slice: Vec<SearchHit> = scanned
                    .into_iter()
                    .skip(usize::try_from(offset).unwrap_or(usize::MAX))
                    .take(page)
                    .collect();

                // R1/ADR-0008 装配：对页内命中一次性批量取 payload（分块 IN，
                // 无 N+1），解析 `text` 字段按 `max_snippet_chars` 截取前缀；
                // 再一次性批量解析归属会话（session_of，同序）。payload 无 text
                // （或非 JSON）→ text None，不臆造正文；无 placement → session_id
                // None。不做任何脱敏（ADR-0004 所有者决定，本地优先工具接受屏显）。
                let ids: Vec<StableId> = slice.iter().map(|hit| hit.id.clone()).collect();
                let payloads = self.catalog.get_many(&ids)?;
                let sessions = self.catalog.session_of(&ids)?;
                let max_snippet_chars = budget.max_snippet_chars;
                let query_terms = guidance::literal_terms(&query);
                let mut assembled: Vec<(SearchHit, bool)> =
                    slice.into_iter().map(|hit| (hit, false)).collect();
                for ((hit, clipped), ((_id, payload), (_mid, session))) in
                    assembled.iter_mut().zip(payloads.into_iter().zip(sessions))
                {
                    *clipped = assemble_search_hit(
                        hit,
                        payload.as_deref(),
                        session.as_ref(),
                        max_snippet_chars,
                        &query_terms,
                        context_lines,
                    );
                }
                let net_bytes = budget
                    .max_response_bytes
                    .saturating_sub(ENVELOPE_RESERVE_BYTES);
                let (kept, mut truncation, _) =
                    budget::clamp_items(assembled, page, net_bytes, |(hit, _)| {
                        search_hit_charge(hit)
                    });
                let context_clipped = kept.iter().any(|(_, clipped)| *clipped);
                let mut hits: Vec<SearchHit> = kept.into_iter().map(|(hit, _)| hit).collect();
                note_context_clipped(&mut truncation, context_clipped);
                // Resume 可用性（ADR-0009）：只对保留的命中批量解析一次（无 N+1）。
                self.assemble_resume_availability(&mut hits)?;
                let consumed = offset + hits.len() as u64;
                // A truncated page with zero kept hits cannot advance the
                // cursor offset; terminate paging instead of looping forever.
                let has_more = scanned_len > consumed && !hits.is_empty();
                let next_cursor = self.issue_cursor(
                    has_more,
                    generation,
                    &query_digest,
                    SORT_SCORE_DESC,
                    None,
                    consumed,
                );
                Ok(AppResponse::Search {
                    hits,
                    next_cursor,
                    generation,
                    truncation,
                    retrieval_mode: response_mode,
                    fallback_warning: response_warning,
                    time_filter_excluded,
                    context_lines,
                })
            }
            AppRequest::Get { id } => {
                let payload = self.catalog.get(&id)?;
                Ok(AppResponse::Get { payload })
            }
            AppRequest::Show { id } => {
                let payload = self.catalog.get(&id)?;
                Ok(AppResponse::Show { payload })
            }
            AppRequest::List {
                limit,
                cursor: token,
                budget,
                sessions_only,
                sort,
            } => {
                if limit == 0 {
                    return Err(DomainError::InvalidRequest("limit must be > 0".into()).into());
                }
                if sort == ListSort::RecencyDesc && !sessions_only {
                    // recency 只对会话有定义：document 实体没有任何时间戳，
                    // message 的时间戳是另一个粒度。与其把三种实体按一个
                    // 编造出来的规则混排，不如显式拒绝并给出可照抄的命令。
                    return Err(DomainError::InvalidRequest(
                        "recency sort is defined for sessions only; \
                         request sessions_only (CLI: `list --sessions --sort recency`)"
                            .into(),
                    )
                    .into());
                }
                budget.validate().map_err(AppError::from)?;
                let generation = self.catalog.active_generation()?;
                // list 无查询串；令牌以空串摘要 + 排序维度标识绑定用例；
                // result_set 判别器把 `list` 与 `list_sessions` 的续读序列隔开。
                // 排序维度进 sort_digest：recency 令牌不能在 wire-id 序列上续读，
                // 反之亦然（cursor::verify 显式拒绝）。
                let query_digest = cursor::digest_query("");
                let sort_digest = sort.digest();
                let result_set = if sessions_only {
                    RESULT_SET_SESSIONS_ONLY
                } else {
                    RESULT_SET_ALL
                };
                let offset = self.resolve_offset(
                    token.as_deref(),
                    generation,
                    &query_digest,
                    sort_digest,
                    Some(result_set),
                )?;

                let page = limit.min(budget.max_items);
                // Cursors are tamper-evident but not unforgeable: a forged
                // huge offset must not overflow into a negative SQL LIMIT
                // (SQLite treats -1 as "no limit", which would load the whole
                // catalog into memory). Cap the fetch window instead.
                let fetch = offset
                    .saturating_add(page as u64)
                    .saturating_add(1)
                    .min(MAX_FETCH_WINDOW);
                let skip = usize::try_from(offset).unwrap_or(usize::MAX);
                let (fetched_len, slice) = match sort {
                    ListSort::WireIdAsc => {
                        let fetched = if sessions_only {
                            self.catalog.list_sessions(fetch as usize)?
                        } else {
                            self.catalog.list(fetch as usize)?
                        };
                        let fetched_len = fetched.len() as u64;
                        let slice: Vec<ListEntry> = fetched
                            .into_iter()
                            .skip(skip)
                            .take(page)
                            .map(|entry| ListEntry {
                                id: entry.id,
                                payload: entry.payload,
                                // 本维度不做时间投影：不填一个"看起来像事实"的值。
                                latest_activity: None,
                            })
                            .collect();
                        (fetched_len, slice)
                    }
                    ListSort::RecencyDesc => {
                        let ordered = self.catalog.sessions_by_recency(fetch as usize)?;
                        let fetched_len = ordered.len() as u64;
                        let window: Vec<(StableId, Option<String>)> =
                            ordered.into_iter().skip(skip).take(page).collect();
                        // 一次批量取本页 payload（分块 IN，无 N+1）。
                        let ids: Vec<StableId> = window.iter().map(|(id, _)| id.clone()).collect();
                        let payloads = self.catalog.get_many(&ids)?;
                        let mut slice = Vec::with_capacity(window.len());
                        for ((id, latest_activity), (_, payload)) in
                            window.into_iter().zip(payloads)
                        {
                            let payload = payload.ok_or_else(|| {
                                DomainError::InvariantViolation(
                                    "recency list referenced a session missing from the catalog"
                                        .into(),
                                )
                            })?;
                            slice.push(ListEntry {
                                id,
                                payload,
                                latest_activity,
                            });
                        }
                        (fetched_len, slice)
                    }
                };
                let net_bytes = budget
                    .max_response_bytes
                    .saturating_sub(ENVELOPE_RESERVE_BYTES);
                let (entries, truncation, _) =
                    budget::clamp_items(slice, page, net_bytes, |entry| {
                        // 最终 JSON 形态
                        // `{"id":"<id>","payload":"<lossy utf-8>","latest_activity":<str|null>}`：
                        // id 按转义计长，payload 按序列化后长度计（不是原始字节数），
                        // 排序键按其序列化形态计（null 为 4 字节）。
                        json_string_len(entry.id.as_str())
                            + lossy_payload_json_len(&entry.payload)
                            + entry.latest_activity.as_deref().map_or(4, json_string_len)
                            + LIST_ENTRY_ENVELOPE_BYTES
                    });
                let consumed = offset + entries.len() as u64;
                // A truncated page with zero kept entries means the first
                // entity already exceeds the byte budget: the next cursor
                // would claim the same offset and loop forever. Terminate
                // paging instead — the page semantics stay honest via
                // `truncation`.
                let has_more = fetched_len > consumed && !entries.is_empty();
                let next_cursor = self.issue_cursor(
                    has_more,
                    generation,
                    &query_digest,
                    sort_digest,
                    Some(result_set),
                    consumed,
                );
                Ok(AppResponse::List {
                    entries,
                    next_cursor,
                    generation,
                    truncation,
                })
            }
            AppRequest::Context {
                session_id,
                policy,
                level,
                budget,
            } => self.handle_context(session_id, policy, level, budget),
            AppRequest::Message {
                message_id,
                session_id,
                around,
                budget,
            } => self.handle_message(message_id, session_id, around, budget),
            AppRequest::MessageContexts { message_id } => self.handle_message_contexts(message_id),
            AppRequest::GetSessionResume { session_id } => {
                // 会话必须有 `ses_v1_*` 种类（protocol 层已校验，这里是纵深防御）。
                if session_id.kind() != IdKind::Session {
                    return Err(DomainError::InvalidRequest(
                        "session id must be a ses_v1_* id".into(),
                    )
                    .into());
                }
                let mut metadata = self
                    .resume
                    .resume_of(std::slice::from_ref(&session_id))?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        AppError::from(PortError::Backend(
                            "resume resolver returned no metadata".into(),
                        ))
                    })?;
                metadata.session_id = session_id;
                Ok(AppResponse::SessionResume(metadata))
            }
            AppRequest::Status => {
                let catalog_count = self.catalog.count()?;
                let active_generation = self.catalog.active_generation()?;
                let context_stats = self.catalog.context_stats()?;
                Ok(AppResponse::Status {
                    catalog_count,
                    active_generation,
                    placements: context_stats.placements,
                    source_placement_claims: context_stats.source_placement_claims,
                })
            }
            AppRequest::Stats => Ok(AppResponse::Stats {
                stats: self.catalog.history_stats()?,
                active_generation: self.catalog.active_generation()?,
            }),
        }
    }

    /// 会话上下文装配（CONTRACT §1-2）：
    ///
    /// 1. 通过 [`ContextGraphStore`] 加载一个 session-scoped typed graph；
    /// 2. 由 Domain selector 在 placement graph 上选择 mainline/full；
    /// 3. 从每个 placement 的 exact document/span 装配 occurrence evidence；
    /// 4. 预算：`max_messages` + 字节闸裁剪 placement occurrences，
    ///    `max_evidence_spans` 裁剪证据；
    ///    任何裁剪都在 [`Truncation`] 里如实报告对应旋钮名。
    fn handle_context(
        &self,
        session_id: StableId,
        policy: ContextPolicy,
        requested_level: ContextLevel,
        budget: ResponseBudget,
    ) -> Result<AppResponse, AppError> {
        budget.validate().map_err(AppError::from)?;
        let generation = self.catalog.active_generation()?;
        let graph = self.catalog.load_session_graph(&session_id)?;
        if graph.session_id.as_str() != session_id.as_str() {
            return Err(DomainError::InvariantViolation(
                "context store returned a different session than requested".into(),
            )
            .into());
        }

        let (selected, branch_leaf, branch_leaf_placement_id): (
            Vec<&MessagePlacement>,
            Option<String>,
            Option<String>,
        ) = match policy {
            ContextPolicy::Mainline => match select_mainline(&graph)? {
                Some(selection) => (
                    selection.placements,
                    Some(selection.leaf.message_id.as_str().to_string()),
                    Some(selection.leaf.id.as_str().to_string()),
                ),
                None => (Vec::new(), None, None),
            },
            ContextPolicy::Full => {
                let placements = select_full(&graph)?;
                let leaf = placements.last().copied();
                (
                    placements,
                    leaf.map(|placement| placement.message_id.as_str().to_string()),
                    leaf.map(|placement| placement.id.as_str().to_string()),
                )
            }
        };

        let session_bytes = self.catalog.get(&session_id)?.ok_or_else(|| {
            DomainError::InvariantViolation("context session is missing from the catalog".into())
        })?;
        let session: serde_json::Value = serde_json::from_slice(&session_bytes).map_err(|_| {
            DomainError::InvariantViolation("session payload is not canonical JSON".into())
        })?;

        let messages_by_id: BTreeMap<&str, &Message> = graph
            .messages
            .iter()
            .map(|message| (message.id.as_str(), message))
            .collect();
        let documents_by_id: BTreeMap<&str, &SourceDocument> = graph
            .source_documents
            .iter()
            .map(|document| (document.id.as_str(), document))
            .collect();
        let mut payloads = BTreeMap::<String, (serde_json::Value, usize)>::new();
        let mut occurrences = Vec::with_capacity(selected.len());
        for placement in selected {
            let message = messages_by_id
                .get(placement.message_id.as_str())
                .copied()
                .ok_or_else(|| {
                    DomainError::InvariantViolation(format!(
                        "placement {} references a missing message",
                        placement.id
                    ))
                })?;
            let document = documents_by_id
                .get(placement.source_document_id.as_str())
                .copied()
                .ok_or_else(|| {
                    DomainError::InvariantViolation(format!(
                        "placement {} references a missing source document",
                        placement.id
                    ))
                })?;

            let message_wire = message.id.as_str().to_string();
            let (payload, payload_len) = if let Some((payload, payload_len)) =
                payloads.get(&message_wire)
            {
                (payload.clone(), *payload_len)
            } else {
                let bytes = self.catalog.get(&message.id)?.ok_or_else(|| {
                    DomainError::InvariantViolation(
                        "context message is missing from the catalog".into(),
                    )
                })?;
                let payload: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
                    DomainError::InvariantViolation("message payload is not canonical JSON".into())
                })?;
                let payload_len = bytes.len();
                payloads.insert(message_wire.clone(), (payload.clone(), payload_len));
                (payload, payload_len)
            };

            let placement_wire = placement.id.as_str().to_string();
            let estimated_bytes = payload_len
                .saturating_add(message_wire.len().saturating_mul(2))
                .saturating_add(placement_wire.len())
                .saturating_add(96);
            occurrences.push(ContextOccurrence {
                message: ContextMessage {
                    id: message_wire.clone(),
                    placement_id: placement_wire,
                    message_id: message_wire,
                    payload,
                },
                evidence: evidence::assemble(message, placement, document, generation),
                role: message.role,
                source_document_id: document.id.as_str().to_string(),
                estimated_bytes,
            });
        }

        let pre_clamp_level = available_level(requested_level, &occurrences);
        let base_net_bytes = budget
            .max_response_bytes
            .saturating_sub(ENVELOPE_RESERVE_BYTES)
            // The session payload is embedded verbatim in the response; count
            // it against the hard byte gate up front, before clamping keeps.
            .saturating_sub(session_bytes.len());
        let mut assembly_level = requested_level;
        let mut net_bytes = base_net_bytes
            .saturating_sub(structural_metadata_reserve(&session_id, pre_clamp_level));
        let (mut kept_occurrences, mut truncation, _) = clamp_context_occurrences(
            occurrences.clone(),
            pre_clamp_level,
            budget.max_messages,
            net_bytes,
        );
        let mut fallback_reason = None;
        if pre_clamp_level != ContextLevel::Raw
            && available_level(pre_clamp_level, &kept_occurrences) == ContextLevel::Raw
        {
            fallback_reason = truncation.reason.clone();
            if requested_level == ContextLevel::Sessions {
                let talks_net_bytes = base_net_bytes.saturating_sub(structural_metadata_reserve(
                    &session_id,
                    ContextLevel::Talks,
                ));
                let (talks_occurrences, talks_truncation, _) = clamp_context_occurrences(
                    occurrences.clone(),
                    ContextLevel::Talks,
                    budget.max_messages,
                    talks_net_bytes,
                );
                if available_level(ContextLevel::Talks, &talks_occurrences) == ContextLevel::Talks {
                    assembly_level = ContextLevel::Talks;
                    net_bytes = talks_net_bytes;
                    kept_occurrences = talks_occurrences;
                    truncation = talks_truncation;
                }
            }
            if available_level(assembly_level, &kept_occurrences) == ContextLevel::Raw {
                assembly_level = ContextLevel::Raw;
                net_bytes = base_net_bytes;
                (kept_occurrences, truncation, _) = clamp_context_occurrences(
                    occurrences,
                    ContextLevel::Raw,
                    budget.max_messages,
                    net_bytes,
                );
            }
        }
        if let Some(reason) = fallback_reason
            && !truncation
                .reason
                .as_deref()
                .is_some_and(|current| current.split(',').any(|value| value == reason))
        {
            truncation.truncated = true;
            truncation.reason = Some(match truncation.reason.take() {
                None => reason,
                Some(current) => format!("{current},{reason}"),
            });
        }
        let mut evidence: Vec<EvidenceSpanDto> = kept_occurrences
            .iter()
            .map(|occurrence| occurrence.evidence.clone())
            .collect();
        if evidence.len() > budget.max_evidence_spans {
            evidence.truncate(budget.max_evidence_spans);
            truncation.truncated = true;
            // 两道闸都触发时两个旋钮名都要报——单升一个救不了另一个。
            truncation.reason = Some(match truncation.reason.take() {
                None => budget::TRUNCATION_MAX_EVIDENCE_SPANS.to_string(),
                Some(prior) => format!("{prior},{}", budget::TRUNCATION_MAX_EVIDENCE_SPANS),
            });
        }

        let messages: Vec<ContextMessage> = kept_occurrences
            .iter()
            .map(|occurrence| occurrence.message.clone())
            .collect();
        let (effective_level, talks, summary) = assemble_level(assembly_level, &kept_occurrences);
        let hint = build_hint(&session_id, requested_level, effective_level);
        if structural_fields_bytes(&talks, &summary, &hint) > net_bytes {
            truncation.truncated = true;
            truncation.reason = Some(match truncation.reason.take() {
                None => budget::TRUNCATION_MAX_RESPONSE_BYTES.to_string(),
                Some(prior)
                    if prior
                        .split(',')
                        .any(|reason| reason == budget::TRUNCATION_MAX_RESPONSE_BYTES) =>
                {
                    prior
                }
                Some(prior) => format!("{prior},{}", budget::TRUNCATION_MAX_RESPONSE_BYTES),
            });
        }

        Ok(AppResponse::Context {
            session_id: session_id.as_str().to_string(),
            session,
            branch_leaf,
            branch_leaf_placement_id,
            tool_activities: {
                let ids: Vec<StableId> = messages
                    .iter()
                    .filter_map(|m| StableId::from_wire(&m.message_id))
                    .collect();
                self.catalog
                    .tool_activities_for_messages(&ids)
                    .unwrap_or_default()
            },
            messages,
            evidence,
            requested_level,
            effective_level,
            talks,
            summary,
            hint,
            truncation,
            generation,
        })
    }

    fn handle_message(
        &self,
        message_id: StableId,
        session_id: Option<StableId>,
        around: usize,
        budget: ResponseBudget,
    ) -> Result<AppResponse, AppError> {
        budget.validate().map_err(AppError::from)?;
        let candidates = self.catalog.message_contexts(&message_id)?;
        let selected_session = match session_id {
            Some(requested) => {
                if !candidates
                    .iter()
                    .any(|candidate| candidate.session_id.as_str() == requested.as_str())
                {
                    return Err(DomainError::NotFound(
                        "message has no placement in the requested session".into(),
                    )
                    .into());
                }
                requested
            }
            None => {
                let session_ids = candidates
                    .iter()
                    .map(|candidate| candidate.session_id.as_str().to_string())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                match session_ids.as_slice() {
                    [] => {
                        return Err(DomainError::NotFound(
                            "message has no session placement".into(),
                        )
                        .into());
                    }
                    [only] => StableId::from_wire(only).ok_or_else(|| {
                        AppError::Domain(DomainError::InvariantViolation(
                            "context store returned a session id outside the wire format".into(),
                        ))
                    })?,
                    _ => {
                        let mut session_ids = session_ids;
                        let candidate_count = session_ids.len();
                        session_ids.truncate(MAX_MESSAGE_AMBIGUITY_CANDIDATES);
                        return Err(MessageAmbiguity {
                            candidate_session_ids: session_ids,
                            candidate_count,
                            hint: "retry get_message with one candidate session_id".into(),
                        }
                        .into());
                    }
                }
            }
        };

        let generation = self.catalog.active_generation()?;
        let graph = self.catalog.load_session_graph(&selected_session)?;
        if graph.session_id.as_str() != selected_session.as_str() {
            return Err(DomainError::InvariantViolation(
                "context store returned a different session than requested".into(),
            )
            .into());
        }
        let mainline = select_mainline(&graph)?
            .map(|selection| selection.placements)
            .unwrap_or_default();
        let matching_anchor_indexes = mainline
            .iter()
            .enumerate()
            .filter_map(|(index, placement)| {
                (placement.message_id.as_str() == message_id.as_str()).then_some(index)
            })
            .collect::<Vec<_>>();
        let anchor_index = match matching_anchor_indexes.as_slice() {
            [index] => *index,
            [] => {
                return Err(DomainError::NotFound(
                    "message is not on the selected session mainline".into(),
                )
                .into());
            }
            _ => {
                return Err(DomainError::InvalidRequest(
                    "message has multiple placements on selected session mainline; use get_session_context to inspect placements".into(),
                )
                .into());
            }
        };
        let start = anchor_index.saturating_sub(around);
        let end = anchor_index
            .saturating_add(around)
            .saturating_add(1)
            .min(mainline.len());
        let selected = &mainline[start..end];
        let anchor_placement_id = mainline[anchor_index].id.as_str().to_string();
        let messages_by_id: BTreeMap<&str, &Message> = graph
            .messages
            .iter()
            .map(|message| (message.id.as_str(), message))
            .collect();
        let net_bytes = budget
            .max_response_bytes
            .saturating_sub(ENVELOPE_RESERVE_BYTES);
        let mut occurrences = Vec::with_capacity(selected.len());
        for placement in selected {
            let message = messages_by_id
                .get(placement.message_id.as_str())
                .copied()
                .ok_or_else(|| {
                    DomainError::InvariantViolation(
                        "message placement references a missing message".into(),
                    )
                })?;
            let bytes = self.catalog.get(&message.id)?.ok_or_else(|| {
                DomainError::InvariantViolation("message is missing from the catalog".into())
            })?;
            let payload = serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|_| {
                DomainError::InvariantViolation("message payload is not canonical JSON".into())
            })?;
            let full_text = payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| message.text.clone());
            let message_wire = message.id.as_str().to_string();
            let placement_wire = placement.id.as_str().to_string();
            let is_anchor = placement.id.as_str() == anchor_placement_id;
            let mut context_message = ContextMessage {
                id: message_wire.clone(),
                placement_id: placement_wire,
                message_id: message_wire,
                payload,
            };
            let payload_truncated = if is_anchor {
                project_anchor_payload(
                    &mut context_message,
                    message.role,
                    &full_text,
                    net_bytes.saturating_sub(48),
                )?
            } else {
                false
            };
            occurrences.push(MessageOccurrence {
                estimated_bytes: message_occurrence_bytes(&context_message),
                is_anchor,
                payload_truncated,
                message: context_message,
            });
        }

        let (kept, truncation) = clamp_message_window(occurrences, &budget, net_bytes);
        let messages = kept
            .into_iter()
            .map(|occurrence| occurrence.message)
            .collect();
        Ok(AppResponse::Message {
            window: MessageWindow {
                message_id: message_id.as_str().to_string(),
                session_id: selected_session.as_str().to_string(),
                anchor_placement_id,
                messages,
                truncation,
                generation,
            },
        })
    }

    fn handle_message_contexts(&self, message_id: StableId) -> Result<AppResponse, AppError> {
        let raw_candidates = self.catalog.message_contexts(&message_id)?;
        let mut grouped = BTreeMap::<String, BTreeSet<String>>::new();
        for candidate in raw_candidates {
            if candidate.placement_ids.is_empty() {
                return Err(DomainError::InvariantViolation(
                    "message context candidate has no placements".into(),
                )
                .into());
            }
            let placement_ids = grouped
                .entry(candidate.session_id.as_str().to_string())
                .or_default();
            placement_ids.extend(
                candidate
                    .placement_ids
                    .into_iter()
                    .map(|placement_id| placement_id.as_str().to_string()),
            );
        }
        let candidates = grouped
            .into_iter()
            .map(|(session_id, placement_ids)| MessageContextCandidate {
                session_id,
                placement_ids: placement_ids.into_iter().collect(),
            })
            .collect();
        Ok(AppResponse::MessageContexts {
            message_id: message_id.as_str().to_string(),
            candidates,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_session_grep_domain::{
        EvidenceSpan, IdKind, MessageEdge, MessagePlacement, MessageRelation, Role,
        SessionContextGraph, SourceDocument, Stability,
    };
    use agent_session_grep_ports::SourcePlacement;
    use agent_session_grep_ports::{
        CatalogEntry, ContextStats, MessageContextCandidate as PortMessageContextCandidate,
        PortResult, SidechainFacet, SourceSnapshot,
    };
    use agent_session_grep_testkit::FakeProvider;

    /// 内存态假后端，仅用于用例逻辑测试。
    struct FakeCatalog;
    impl FakeCatalog {
        fn list_mock(&self, kind: Option<IdKind>, limit: usize) -> PortResult<Vec<CatalogEntry>> {
            let id = match kind {
                Some(IdKind::Session) => StableId::derive(
                    IdKind::Session,
                    Stability::Reconstructed,
                    &[b"listed-session"],
                ),
                _ => StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"listed"]),
            };
            Ok(vec![CatalogEntry {
                id,
                payload: b"payload".to_vec(),
            }]
            .into_iter()
            .take(limit)
            .collect())
        }
    }
    impl CatalogStore for FakeCatalog {
        fn get(&self, _id: &StableId) -> PortResult<Option<Vec<u8>>> {
            Ok(Some(b"payload".to_vec()))
        }
        fn get_many(&self, ids: &[StableId]) -> PortResult<Vec<(StableId, Option<Vec<u8>>)>> {
            Ok(ids
                .iter()
                .map(|id| (id.clone(), Some(b"payload".to_vec())))
                .collect())
        }
        fn put(&self, _id: &StableId, _payload: &[u8]) -> PortResult<()> {
            Ok(())
        }
        fn list(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
            self.list_mock(None, limit)
        }
        fn list_sessions(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
            self.list_mock(Some(IdKind::Session), limit)
        }
        fn sessions_by_recency(&self, limit: usize) -> PortResult<Vec<(StableId, Option<String>)>> {
            // 这个 double 只喂 list 的接线测试：一条会话，provider 没给时间戳。
            Ok(self
                .list_mock(Some(IdKind::Session), limit)?
                .into_iter()
                .map(|entry| (entry.id, None))
                .collect())
        }
        fn count(&self) -> PortResult<u64> {
            Ok(1)
        }
        fn active_generation(&self) -> PortResult<u64> {
            Ok(7)
        }
    }
    impl ContextGraphStore for FakeCatalog {
        fn load_session_graph(&self, _session_id: &StableId) -> PortResult<SessionContextGraph> {
            Err(PortError::NotFound("session context not found".into()))
        }

        fn message_contexts(
            &self,
            _message_id: &StableId,
        ) -> PortResult<Vec<PortMessageContextCandidate>> {
            Ok(Vec::new())
        }

        fn session_of(&self, ids: &[StableId]) -> PortResult<Vec<(StableId, Option<StableId>)>> {
            Ok(ids.iter().map(|id| (id.clone(), None)).collect())
        }

        fn source_placements_of(
            &self,
            ids: &[StableId],
        ) -> PortResult<Vec<(StableId, Option<SourcePlacement>)>> {
            Ok(ids.iter().map(|id| (id.clone(), None)).collect())
        }

        fn context_stats(&self) -> PortResult<ContextStats> {
            Ok(ContextStats {
                placements: 2,
                source_placement_claims: 3,
            })
        }
    }

    struct FakeIndex;
    impl SearchIndex for FakeIndex {
        fn index(&self, _id: &StableId, _text: &str) -> PortResult<()> {
            Ok(())
        }
        fn query_filtered(
            &self,
            _query: SearchQuery<'_>,
            _limit: usize,
        ) -> PortResult<Vec<SearchHit>> {
            Ok(vec![SearchHit {
                id: StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"h"]),
                score: 1.0,
                session_id: None,
                text: None,
                why_matched: Vec::new(),
                suggested_next_commands: Vec::new(),
                occurrences: 1,
                resume_available: false,
                provider_id: None,
                working_directory: None,
                project_name: None,
                match_ranges: Vec::new(),
            }])
        }
    }

    fn app() -> App<FakeCatalog, FakeIndex> {
        App::new(FakeCatalog, FakeIndex)
    }

    // 引用 SourceSnapshot 以确认 ports DTO 可被应用层消费（编译期保证）。
    #[allow(dead_code)]
    fn _snapshot_is_usable(s: SourceSnapshot) -> u64 {
        s.len
    }

    #[test]
    fn search_returns_hits() {
        let r = app().handle(AppRequest::Search {
            query: "hello".into(),
            filters: SearchFilters::default(),
            facets: SearchFacets::default(),
            limit: 10,
            cursor: None,
            budget: ResponseBudget::default(),
            include_system: false,
            group_by_session: false,
            mode: RetrievalMode::Lexical,
            query_embedding: None,
            context_lines: None,
        });
        assert!(matches!(r, Ok(AppResponse::Search { hits, .. }) if hits.len() == 1));
    }

    #[test]
    fn search_rejects_zero_limit() {
        let r = app().handle(AppRequest::Search {
            query: "x".into(),
            filters: SearchFilters::default(),
            facets: SearchFacets::default(),
            limit: 0,
            cursor: None,
            budget: ResponseBudget::default(),
            include_system: false,
            group_by_session: false,
            mode: RetrievalMode::Lexical,
            query_embedding: None,
            context_lines: None,
        });
        assert!(matches!(
            r.unwrap_err(),
            AppError::Domain(DomainError::InvalidRequest(_))
        ));
    }

    #[test]
    fn search_rejects_empty_query() {
        let r = app().handle(AppRequest::Search {
            query: "   ".into(),
            filters: SearchFilters::default(),
            facets: SearchFacets::default(),
            limit: 5,
            cursor: None,
            budget: ResponseBudget::default(),
            include_system: false,
            group_by_session: false,
            mode: RetrievalMode::Lexical,
            query_embedding: None,
            context_lines: None,
        });
        assert!(matches!(
            r.unwrap_err(),
            AppError::Domain(DomainError::InvalidRequest(_))
        ));
    }

    #[test]
    fn search_rejects_control_characters_before_index_query() {
        // R4.2（ADR-0003）：NUL/C0/C1 控制字符在 Application 边界拒绝为
        // invalid_request，绝不清除式净化（删除会拼接 token）；且必须发生在
        // 任何索引查询之前——命中索引即 panic。
        struct ExplodingIndex;
        impl SearchIndex for ExplodingIndex {
            fn index(&self, _id: &StableId, _text: &str) -> PortResult<()> {
                Ok(())
            }
            fn query_filtered(
                &self,
                _query: SearchQuery<'_>,
                _limit: usize,
            ) -> PortResult<Vec<SearchHit>> {
                panic!("control-character query must be rejected before any index query")
            }
        }
        let app = App::new(FakeCatalog, ExplodingIndex);
        for query in [
            "\u{0}", "a\u{1}b", "\u{7f}", "a\u{80}b", "\u{9f}", "a\tb", "a\nb",
        ] {
            let err = app
                .handle(search_req(query, 5, None))
                .expect_err("query {query:?} must be rejected");
            assert!(
                matches!(err, AppError::Domain(DomainError::InvalidRequest(_))),
                "{query:?}: {err}"
            );
        }
    }

    /// 与 [`PagedIndex`] 的派生规则一致（`hit{i:02}` 种子），使目录 payload
    /// 能对应到检索命中。
    fn hit_id(tag: &str) -> StableId {
        StableId::derive(IdKind::Message, Stability::Reconstructed, &[tag.as_bytes()])
    }

    #[test]
    fn search_hits_carry_text_summary_from_payloads() {
        // R1（ADR-0004）/ADR-0008：text 摘要（原 snippet）在 Application 检索
        // 装配时生成——批量取 payload、解析 `text` 字段、截取前缀；不做任何
        // 脱敏。MapCatalog 无 placement 数据 → session_id 为 None。
        let mut cat = MapCatalog::new(7);
        for (tag, text) in [("hit00", "hello world"), ("hit01", "second hit")] {
            let id = hit_id(tag);
            cat.insert(
                &id,
                serde_json::json!({ "role": "user", "text": text })
                    .to_string()
                    .into_bytes(),
            );
        }
        let app = App::with_clock(&cat, PagedIndex { n: 2 }, clock_t0);
        let resp = app.handle(search_req("q", 10, None)).unwrap();
        let AppResponse::Search { hits, .. } = resp else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, hit_id("hit00"));
        assert_eq!(hits[0].text.as_deref(), Some("hello world"));
        assert_eq!(hits[1].text.as_deref(), Some("second hit"));
        assert!(hits.iter().all(|hit| hit.session_id.is_none()));
    }

    #[test]
    fn search_text_truncates_to_max_snippet_chars() {
        // R1.2：单条 text 摘要按 `max_snippet_chars`（字符数）显式截取前缀。
        let mut cat = MapCatalog::new(7);
        let id = hit_id("hit00");
        cat.insert(
            &id,
            serde_json::json!({ "text": "abcdefghij" })
                .to_string()
                .into_bytes(),
        );
        let app = App::with_clock(&cat, PagedIndex { n: 1 }, clock_t0);
        let resp = app
            .handle(AppRequest::Search {
                query: "q".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 5,
                cursor: None,
                budget: ResponseBudget {
                    max_snippet_chars: 4,
                    ..Default::default()
                },
                include_system: false,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap();
        let AppResponse::Search { hits, .. } = resp else {
            panic!("expected Search response");
        };
        assert_eq!(hits[0].text.as_deref(), Some("abcd"));
    }

    #[test]
    fn search_text_none_when_payload_has_no_text() {
        // text 缺失 / 非字符串 / payload 非 JSON → text None（不臆造正文）。
        // 空字符串 text 视为有正文（与旧 CLI 行为一致）。
        let mut cat = MapCatalog::new(7);
        for (tag, payload) in [
            ("hit00", br#"{"role":"user"}"#.as_slice()),
            ("hit01", br#"{"text":42}"#.as_slice()),
            ("hit02", b"not json".as_slice()),
            ("hit03", br#"{"text":""}"#.as_slice()),
        ] {
            let id = hit_id(tag);
            cat.insert(&id, payload.to_vec());
        }
        let app = App::with_clock(&cat, PagedIndex { n: 4 }, clock_t0);
        let resp = app.handle(search_req("q", 10, None)).unwrap();
        let AppResponse::Search { hits, .. } = resp else {
            panic!("expected Search response");
        };
        assert_eq!(hits[3].text.as_deref(), Some(""));
        assert!(hits[..3].iter().all(|hit| hit.text.is_none()));
    }

    #[test]
    fn search_byte_gate_charges_text_bytes() {
        // R1.2/R4.2：text 摘要字节计入同一 `max_response_bytes` 闸。无 text 时
        // 4 条命中全部放得下（每条仅 ~70 B）；带 1000 字符 text 时每条
        // ~1080 B，净预算 3072 只容 2 条——证明摘要字节被计入闸门，
        // 且截断原因显式报 max_response_bytes。
        let mut cat = MapCatalog::new(7);
        for tag in ["hit00", "hit01", "hit02", "hit03"] {
            let id = hit_id(tag);
            cat.insert(
                &id,
                serde_json::json!({ "text": "x".repeat(1000) })
                    .to_string()
                    .into_bytes(),
            );
        }
        let app = App::with_clock(&cat, PagedIndex { n: 4 }, clock_t0);
        let resp = app
            .handle(AppRequest::Search {
                query: "q".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 10,
                cursor: None,
                budget: ResponseBudget {
                    max_response_bytes: budget::MIN_RESPONSE_BYTES,
                    ..Default::default()
                },
                include_system: false,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap();
        let (ids, _, _, truncation) = hits_of(resp);
        assert!(!ids.is_empty() && ids.len() < 4, "kept {}", ids.len());
        assert!(truncation.truncated);
        assert_eq!(
            truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_RESPONSE_BYTES)
        );
    }

    #[test]
    fn search_hits_carry_session_id_from_placements() {
        // ADR-0008：命中带归属会话 wire id（session_of 批量解析）+ text 摘要。
        // GraphCatalog 的 graph 里有 placement → session_id 有值；payload 的
        // `text` 字段 → text 有值。
        let fixture = ctx_fixture();
        let ids = vec![
            fixture.root.clone(),
            fixture.repeated.clone(),
            fixture.leaf.clone(),
        ];
        let app = App::with_clock(&fixture.store, FixedHits(ids.clone()), clock_t0);
        let resp = app.handle(search_req("q", 10, None)).unwrap();
        let AppResponse::Search { hits, .. } = resp else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 3);
        for (hit, expected) in hits.iter().zip(&ids) {
            assert_eq!(&hit.id, expected);
            assert_eq!(hit.session_id.as_deref(), Some(fixture.session.as_str()));
        }
        assert_eq!(hits[0].text.as_deref(), Some("root"));
        assert_eq!(hits[1].text.as_deref(), Some("repeated"));
        assert_eq!(hits[2].text.as_deref(), Some("leaf"));
    }

    #[test]
    fn get_returns_payload() {
        let id = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"g"]);
        let r = app().handle(AppRequest::Get { id });
        assert!(matches!(r, Ok(AppResponse::Get { payload: Some(_) })));
    }

    #[test]
    fn list_returns_stable_entries() {
        let r = app().handle(AppRequest::List {
            limit: 10,
            cursor: None,
            budget: ResponseBudget::default(),
            sessions_only: false,
            sort: ListSort::WireIdAsc,
        });
        assert!(matches!(r, Ok(AppResponse::List { entries, .. }) if entries.len() == 1));
    }

    #[test]
    fn list_sessions_only_filters_to_session_kind() {
        let r = app().handle(AppRequest::List {
            limit: 10,
            cursor: None,
            budget: ResponseBudget::default(),
            sessions_only: true,
            sort: ListSort::WireIdAsc,
        });
        assert!(
            matches!(r, Ok(AppResponse::List { entries, .. }) if entries.iter().all(|e| e.id.kind() == IdKind::Session))
        );
    }

    #[test]
    fn list_rejects_zero_limit() {
        let err = app()
            .handle(AppRequest::List {
                limit: 0,
                cursor: None,
                budget: ResponseBudget::default(),
                sessions_only: false,
                sort: ListSort::WireIdAsc,
            })
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::InvalidRequest(_))
        ));
    }

    #[test]
    fn status_returns_catalog_count() {
        let r = app().handle(AppRequest::Status);
        assert!(matches!(
            r,
            Ok(AppResponse::Status {
                catalog_count: 1,
                active_generation: 7,
                placements: 2,
                source_placement_claims: 3,
            })
        ));
    }

    // ---- 分页 + cursor + budget + context（design §6 集成）----

    fn clock_t0() -> i64 {
        1_000_000
    }

    fn clock_after_ttl() -> i64 {
        1_000_000 + cursor::DEFAULT_TTL_MS
    }

    /// 可分页假索引：n 个确定性命中，按 limit 截取（模拟钉住排序上的超取）。
    struct PagedIndex {
        n: usize,
    }
    impl SearchIndex for PagedIndex {
        fn index(&self, _id: &StableId, _text: &str) -> PortResult<()> {
            Ok(())
        }
        fn query_filtered(
            &self,
            _query: SearchQuery<'_>,
            limit: usize,
        ) -> PortResult<Vec<SearchHit>> {
            Ok((0..self.n.min(limit))
                .map(|i| SearchHit {
                    id: StableId::derive(
                        IdKind::Message,
                        Stability::Reconstructed,
                        &[format!("hit{i:02}").as_bytes()],
                    ),
                    score: -(i as f32),
                    session_id: None,
                    text: None,
                    why_matched: Vec::new(),
                    suggested_next_commands: Vec::new(),
                    occurrences: 1,
                    resume_available: false,
                    provider_id: None,
                    working_directory: None,
                    project_name: None,
                    match_ranges: Vec::new(),
                })
                .collect())
        }
    }

    struct FixedHits(Vec<StableId>);
    impl SearchIndex for FixedHits {
        fn index(&self, _id: &StableId, _text: &str) -> PortResult<()> {
            Ok(())
        }
        fn query_filtered(
            &self,
            _query: SearchQuery<'_>,
            limit: usize,
        ) -> PortResult<Vec<SearchHit>> {
            Ok(self
                .0
                .iter()
                .take(limit)
                .map(|id| SearchHit {
                    id: id.clone(),
                    score: 0.0,
                    session_id: None,
                    text: None,
                    why_matched: Vec::new(),
                    suggested_next_commands: Vec::new(),
                    occurrences: 1,
                    resume_available: false,
                    provider_id: None,
                    working_directory: None,
                    project_name: None,
                    match_ranges: Vec::new(),
                })
                .collect())
        }
    }

    /// 内存 map 目录：BTreeMap 键序即 wire id 升序（与 sqlite list 的钉住排序一致）；
    /// generation 用 Cell 可变，测 cursor 的 generation 绑定。
    struct MapCatalog {
        map: std::collections::BTreeMap<String, Vec<u8>>,
        generation: std::cell::Cell<u64>,
        session_of: std::collections::BTreeMap<String, String>,
        /// 会话 wire id → 最近活动时间戳；未登记的会话即"provider 没给时间戳"，
        /// 与 SQLite 的 `sessions_by_recency` 语义一致（排在末尾、值为 None）。
        latest_activity: std::collections::BTreeMap<String, String>,
    }
    impl MapCatalog {
        fn new(generation: u64) -> Self {
            Self {
                map: Default::default(),
                generation: std::cell::Cell::new(generation),
                session_of: Default::default(),
                latest_activity: Default::default(),
            }
        }
        fn insert(&mut self, id: &StableId, payload: impl Into<Vec<u8>>) {
            self.map.insert(id.as_str().to_string(), payload.into());
        }
        fn set_session_of(&mut self, message_id: &StableId, session_id: &StableId) {
            self.session_of.insert(
                message_id.as_str().to_string(),
                session_id.as_str().to_string(),
            );
        }
        fn set_latest_activity(&mut self, session_id: &StableId, timestamp: &str) {
            self.latest_activity
                .insert(session_id.as_str().to_string(), timestamp.to_string());
        }
    }
    impl CatalogStore for MapCatalog {
        fn get(&self, id: &StableId) -> PortResult<Option<Vec<u8>>> {
            Ok(self.map.get(id.as_str()).cloned())
        }
        fn get_many(&self, ids: &[StableId]) -> PortResult<Vec<(StableId, Option<Vec<u8>>)>> {
            Ok(ids
                .iter()
                .map(|id| {
                    let payload = self.map.get(id.as_str()).cloned();
                    (id.clone(), payload)
                })
                .collect())
        }
        fn put(&self, _id: &StableId, _payload: &[u8]) -> PortResult<()> {
            Ok(())
        }
        fn list(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
            Ok(self
                .map
                .iter()
                .take(limit)
                .map(|(k, v)| CatalogEntry {
                    id: StableId::from_wire(k).expect("map keys are wire ids"),
                    payload: v.clone(),
                })
                .collect())
        }
        fn list_sessions(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
            Ok(self
                .map
                .iter()
                .filter(|(k, _)| k.starts_with("ses_v1_"))
                .take(limit)
                .map(|(k, v)| CatalogEntry {
                    id: StableId::from_wire(k).expect("map keys are wire ids"),
                    payload: v.clone(),
                })
                .collect())
        }
        fn sessions_by_recency(&self, limit: usize) -> PortResult<Vec<(StableId, Option<String>)>> {
            let mut rows: Vec<(StableId, Option<String>)> = self
                .map
                .keys()
                .filter(|k| k.starts_with("ses_v1_"))
                .map(|k| {
                    (
                        StableId::from_wire(k).expect("map keys are wire ids"),
                        self.latest_activity.get(k).cloned(),
                    )
                })
                .collect();
            // 与 SQLite `ORDER BY latest IS NULL ASC, latest DESC, id ASC` 同序。
            rows.sort_by(|left, right| {
                left.1
                    .is_none()
                    .cmp(&right.1.is_none())
                    .then_with(|| right.1.cmp(&left.1))
                    .then_with(|| left.0.as_str().cmp(right.0.as_str()))
            });
            rows.truncate(limit);
            Ok(rows)
        }
        fn count(&self) -> PortResult<u64> {
            Ok(self.map.len() as u64)
        }
        fn active_generation(&self) -> PortResult<u64> {
            Ok(self.generation.get())
        }
    }
    impl ContextGraphStore for MapCatalog {
        fn load_session_graph(&self, _session_id: &StableId) -> PortResult<SessionContextGraph> {
            Err(PortError::NotFound("session context not found".into()))
        }

        fn message_contexts(
            &self,
            _message_id: &StableId,
        ) -> PortResult<Vec<PortMessageContextCandidate>> {
            Ok(Vec::new())
        }

        fn session_of(&self, ids: &[StableId]) -> PortResult<Vec<(StableId, Option<StableId>)>> {
            // 纯 map 目录没有 placement 数据 → 全部 None；测试可用 set_session_of
            // 显式注入归属（R3 归并按会话坍缩需要真实归属）。
            Ok(ids
                .iter()
                .map(|id| {
                    let session = self
                        .session_of
                        .get(id.as_str())
                        .and_then(|wire| StableId::from_wire(wire));
                    (id.clone(), session)
                })
                .collect())
        }

        fn source_placements_of(
            &self,
            ids: &[StableId],
        ) -> PortResult<Vec<(StableId, Option<SourcePlacement>)>> {
            Ok(ids.iter().map(|id| (id.clone(), None)).collect())
        }

        fn context_stats(&self) -> PortResult<ContextStats> {
            Ok(ContextStats::default())
        }
    }

    fn search_req(query: &str, limit: usize, cursor: Option<String>) -> AppRequest {
        AppRequest::Search {
            query: query.into(),
            filters: SearchFilters::default(),
            facets: SearchFacets::default(),
            limit,
            cursor,
            budget: ResponseBudget::default(),
            include_system: false,
            group_by_session: false,
            mode: RetrievalMode::Lexical,
            query_embedding: None,
            context_lines: None,
        }
    }

    /// 与 [`search_req`] 同构，但携带 facet 过滤。
    fn search_req_facets(
        query: &str,
        limit: usize,
        cursor: Option<String>,
        facets: SearchFacets,
    ) -> AppRequest {
        AppRequest::Search {
            query: query.into(),
            filters: SearchFilters::default(),
            facets,
            limit,
            cursor,
            budget: ResponseBudget::default(),
            include_system: false,
            group_by_session: false,
            mode: RetrievalMode::Lexical,
            query_embedding: None,
            context_lines: None,
        }
    }

    #[test]
    fn search_attaches_cjk_and_ascii_why_matched_and_suggestions() {
        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "text": "包含数据库迁移方案 guidance" })
                .to_string()
                .into_bytes(),
        );
        cat.insert(
            &hit_id("hit01"),
            serde_json::json!({ "text": "different guidance" })
                .to_string()
                .into_bytes(),
        );
        let app = App::with_clock(cat, PagedIndex { n: 2 }, clock_t0);
        let AppResponse::Search { hits, .. } =
            app.handle(search_req("数据库 guidance", 10, None)).unwrap()
        else {
            panic!("expected Search response");
        };
        assert_eq!(hits[0].why_matched, vec!["数据", "据库", "guidance"]);
        assert_eq!(hits[1].why_matched, vec!["guidance"]);
        assert!(
            hits.iter()
                .all(|hit| hit.suggested_next_commands.is_empty())
        );
    }

    #[test]
    fn semantic_mode_without_index_falls_back_explicitly() {
        // #3 Q54：semantic/hybrid 在语义索引未就绪（占位 NoSemanticIndex）时
        // 必须显式降级为 lexical_fallback + warning，禁止静默切换。
        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "text": "needle in haystack" })
                .to_string()
                .into_bytes(),
        );
        let app = App::with_clock(cat, PagedIndex { n: 2 }, clock_t0);
        for mode in [RetrievalMode::Semantic, RetrievalMode::Hybrid] {
            let response = app
                .handle(AppRequest::Search {
                    query: "needle".into(),
                    filters: SearchFilters::default(),
                    facets: SearchFacets::default(),
                    limit: 10,
                    cursor: None,
                    budget: ResponseBudget::default(),
                    include_system: false,
                    group_by_session: false,
                    mode,
                    query_embedding: Some(vec![0.1f32; 384]),
                    context_lines: None,
                })
                .unwrap();
            let AppResponse::Search {
                hits,
                retrieval_mode,
                fallback_warning,
                ..
            } = response
            else {
                panic!("expected Search response");
            };
            // 词法命中仍可用（结果非空），但模式如实标注降级。
            assert!(!hits.is_empty());
            assert_eq!(retrieval_mode, RetrievalMode::LexicalFallback);
            let warning = fallback_warning.expect("fallback must be surfaced");
            assert!(warning.contains("fell back to lexical"));
        }
    }

    #[test]
    fn lexical_mode_is_never_marked_fallback() {
        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        let app = App::with_clock(cat, PagedIndex { n: 2 }, clock_t0);
        let AppResponse::Search {
            retrieval_mode,
            fallback_warning,
            ..
        } = app
            .handle(AppRequest::Search {
                query: "needle".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 10,
                cursor: None,
                budget: ResponseBudget::default(),
                include_system: false,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap()
        else {
            panic!("expected Search response");
        };
        assert_eq!(retrieval_mode, RetrievalMode::Lexical);
        assert!(fallback_warning.is_none());
    }

    #[test]
    fn search_excludes_system_and_developer_roles_by_default() {
        // R2 系统噪声默认排除：role=system/developer 的命中不进结果；无 role
        // 字段或非 JSON payload（legacy）不判为噪声。过滤发生在 offset 切片前，
        // 保证 cursor 位置指向"非系统"序列。
        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "role": "user", "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        cat.insert(
            &hit_id("hit01"),
            serde_json::json!({ "role": "system", "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        cat.insert(
            &hit_id("hit02"),
            serde_json::json!({ "role": "developer", "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        cat.insert(&hit_id("hit03"), b"not json".to_vec());
        let index = FixedHits(vec![
            hit_id("hit00"),
            hit_id("hit01"),
            hit_id("hit02"),
            hit_id("hit03"),
        ]);
        let app = App::with_clock(cat, index, clock_t0);
        let AppResponse::Search { hits, .. } = app.handle(search_req("needle", 10, None)).unwrap()
        else {
            panic!("expected Search response");
        };
        let kept: Vec<StableId> = hits.iter().map(|hit| hit.id.clone()).collect();
        assert_eq!(kept, vec![hit_id("hit00"), hit_id("hit03")]);
    }

    #[test]
    fn search_include_system_restores_system_developer_hits() {
        // R2 opt-in：include_system=true 时 system/developer 命中恢复进结果。
        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "role": "system", "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        let index = FixedHits(vec![hit_id("hit00")]);
        let app = App::with_clock(cat, index, clock_t0);
        let AppResponse::Search { hits, .. } = app
            .handle(AppRequest::Search {
                query: "needle".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 10,
                cursor: None,
                budget: ResponseBudget::default(),
                include_system: true,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap()
        else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, hit_id("hit00"));
    }

    #[test]
    fn search_role_filter_supersedes_the_system_noise_default() {
        // M3-8: `--role system` must return system messages without also being
        // told `--include-system`. The store predicate has already narrowed the
        // window to the named roles, so re-applying the default noise policy here
        // would subtract exactly what the caller asked for and always answer
        // "no hits" — a silently empty answer to a well-formed request.
        use agent_session_grep_ports::SearchRole;

        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "role": "system", "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        let index = FixedHits(vec![hit_id("hit00")]);
        let app = App::with_clock(cat, index, clock_t0);
        let request = filtered_search_req(
            "needle",
            10,
            None,
            SearchFilters {
                roles: vec![SearchRole::System],
                ..SearchFilters::default()
            },
        );
        let AppResponse::Search { hits, .. } = app.handle(request).unwrap() else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 1, "include_system must not be re-applied");
        assert_eq!(hits[0].id, hit_id("hit00"));
    }

    #[test]
    fn search_without_a_role_filter_still_drops_system_noise() {
        // The escape hatch above is scoped to an explicit role allowlist: with no
        // roles named, the default noise policy is untouched.
        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "role": "system", "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        let index = FixedHits(vec![hit_id("hit00")]);
        let app = App::with_clock(cat, index, clock_t0);
        let AppResponse::Search { hits, .. } = app.handle(search_req("needle", 10, None)).unwrap()
        else {
            panic!("expected Search response");
        };
        assert!(hits.is_empty());
    }

    #[test]
    fn search_rejects_unusable_exclusion_terms() {
        // Same boundary rule the query string gets: control characters are
        // rejected rather than stripped (stripping splices tokens together), and
        // a blank term is a request the caller cannot have meant.
        for term in ["", "   ", "no\u{0}ise", "line\nbreak"] {
            let err = app()
                .handle(filtered_search_req(
                    "needle",
                    5,
                    None,
                    SearchFilters {
                        exclude_terms: vec![term.to_string()],
                        ..SearchFilters::default()
                    },
                ))
                .expect_err("unusable exclusion term must be rejected");
            assert!(
                matches!(err, AppError::Domain(DomainError::InvalidRequest(_))),
                "{term:?} -> {err:?}"
            );
        }
    }

    #[test]
    fn search_group_by_session_collapses_with_occurrences() {
        // R3 归并：每会话保留最高分命中（钉住顺序中的首个），occurrences 为该
        // 会话在扫描窗内的命中数；无归属（None）命中自成单例组。
        let mut cat = MapCatalog::new(7);
        let session_a = StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"sA"]);
        let session_b = StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"sB"]);
        for (tag, session) in [
            ("hit00", Some(&session_a)),
            ("hit01", Some(&session_a)),
            ("hit02", Some(&session_b)),
            ("hit03", Some(&session_b)),
            ("hit04", None),
        ] {
            let id = hit_id(tag);
            cat.insert(
                &id,
                serde_json::json!({ "role": "user", "text": "needle" })
                    .to_string()
                    .into_bytes(),
            );
            if let Some(session) = session {
                cat.set_session_of(&id, session);
            }
        }
        let index = FixedHits(vec![
            hit_id("hit00"),
            hit_id("hit01"),
            hit_id("hit02"),
            hit_id("hit03"),
            hit_id("hit04"),
        ]);
        let app = App::with_clock(cat, index, clock_t0);
        let AppResponse::Search { hits, .. } = app
            .handle(AppRequest::Search {
                query: "needle".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 10,
                cursor: None,
                budget: ResponseBudget::default(),
                include_system: false,
                group_by_session: true,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap()
        else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 3, "one group per session + singleton");
        assert_eq!(hits[0].id, hit_id("hit00"));
        assert_eq!(hits[0].occurrences, 2);
        assert_eq!(hits[0].session_id.as_deref(), Some(session_a.as_str()));
        assert_eq!(hits[1].id, hit_id("hit02"));
        assert_eq!(hits[1].occurrences, 2);
        assert_eq!(hits[1].session_id.as_deref(), Some(session_b.as_str()));
        assert_eq!(hits[2].id, hit_id("hit04"));
        assert_eq!(hits[2].occurrences, 1);
        assert!(hits[2].session_id.is_none());
    }

    #[test]
    fn search_group_by_session_default_path_keeps_occurrences_one() {
        // R3 默认路径（group_by_session=false）保持不变：不归并、逐命中返回，
        // occurrences 恒为 1（序列化时省略该键，与既有输出字节兼容）。
        let mut cat = MapCatalog::new(7);
        let session_a = StableId::derive(IdKind::Session, Stability::Reconstructed, &[b"sA"]);
        for tag in ["hit00", "hit01"] {
            let id = hit_id(tag);
            cat.insert(
                &id,
                serde_json::json!({ "role": "user", "text": "needle" })
                    .to_string()
                    .into_bytes(),
            );
            cat.set_session_of(&id, &session_a);
        }
        let index = FixedHits(vec![hit_id("hit00"), hit_id("hit01")]);
        let app = App::with_clock(cat, index, clock_t0);
        let AppResponse::Search { hits, .. } = app.handle(search_req("needle", 10, None)).unwrap()
        else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, hit_id("hit00"));
        assert_eq!(hits[0].occurrences, 1);
        assert_eq!(hits[1].occurrences, 1);
    }

    #[test]
    fn search_why_matched_detects_term_beyond_displayed_prefix() {
        let mut cat = MapCatalog::new(7);
        let mut text = "x".repeat(1500);
        text.push_str(" needle");
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "text": text }).to_string().into_bytes(),
        );
        let app = App::with_clock(cat, PagedIndex { n: 1 }, clock_t0);
        let AppResponse::Search { hits, .. } = app
            .handle(AppRequest::Search {
                query: "needle".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 10,
                cursor: None,
                budget: ResponseBudget {
                    max_snippet_chars: 8,
                    ..Default::default()
                },
                include_system: false,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap()
        else {
            panic!("expected Search response");
        };
        // M3-9 起摘要窗口以首个命中词为中心。此测试的窗口只有 8 字符,装不下
        // 完整的 `needle`,但**命中位置不再不可见**:旧行为盲取前缀,得到纯填充
        // `"xxxxxxxx"`(命中词毫无踪影);现在窗口移到命中处,显示出词的开头。
        // 要保住的性质没变:`why_matched` 必须报出超出显示宽度的命中词。
        let snippet = hits[0].text.as_deref().expect("snippet present");
        assert!(
            snippet.contains("nee"),
            "窗口必须移到命中处并显示命中词开头(而非纯前缀): {snippet:?}"
        );
        assert!(
            snippet.starts_with('…'),
            "命中落在前缀之外,左侧被裁时必须有省略标记: {snippet:?}"
        );
        assert_ne!(snippet, "xxxxxxxx", "不得再盲取前缀");
        assert_eq!(hits[0].why_matched, vec!["needle"]);
    }

    #[test]
    fn snippet_window_centres_on_the_match_and_clamps_at_both_ends() {
        // M3-9 的核心：命中词落在窗口外时,用户看不出为什么命中。
        let terms = vec!["needle".to_string()];

        // 中段命中 → 两侧都裁,两侧都有省略号,且命中词可见。
        let mid = format!("{}needle{}", "a".repeat(300), "b".repeat(300));
        let s = snippet_around_match(&mid, 40, &terms);
        assert!(s.contains("needle"), "{s}");
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");

        // 开头命中 → 左侧无需裁,不加左省略号,窗口仍是满宽。
        let head = format!("needle{}", "b".repeat(300));
        let s = snippet_around_match(&head, 40, &terms);
        assert!(s.starts_with("needle"), "{s}");
        assert!(!s.starts_with('…') && s.ends_with('…'), "{s}");

        // 结尾命中 → 右侧无需裁。
        let tail = format!("{}needle", "a".repeat(300));
        let s = snippet_around_match(&tail, 40, &terms);
        assert!(s.contains("needle"), "{s}");
        assert!(s.starts_with('…') && !s.ends_with('…'), "{s}");

        // 文本短于窗口 → 原样返回,不加任何标记(与旧前缀行为逐字相同)。
        assert_eq!(
            snippet_around_match("short needle", 40, &terms),
            "short needle"
        );

        // 大小写不敏感(与产生该命中的 FTS 行为一致)。
        let upper = format!("{}NEEDLE{}", "a".repeat(300), "b".repeat(300));
        assert!(snippet_around_match(&upper, 40, &terms).contains("NEEDLE"));
    }

    #[test]
    fn snippet_never_exceeds_the_char_budget_including_markers() {
        // `max_snippet_chars` 是**响应大小契约**:省略号必须从窗口里扣,
        // 不能加在预算之外。第一版把两个标记加在满宽窗口之外,于是 2000 的
        // 预算返回 2001 字符,e2e 的预算断言直接失败 —— 这是我引入的真 bug,
        // 不是过时的测试。
        let terms = vec!["needle".to_string()];
        let cases = [
            format!("{}needle{}", "a".repeat(300), "b".repeat(300)), // 中段
            format!("needle{}", "b".repeat(300)),                    // 开头
            format!("{}needle", "a".repeat(300)),                    // 结尾
            "a".repeat(300),                                         // 定位不到词
            format!("{}找到{}", "文".repeat(200), "字".repeat(200)), // 多字节
        ];
        for text in &cases {
            for budget in [1usize, 2, 3, 8, 40, 299, 300, 301, 2000] {
                let s = snippet_around_match(text, budget, &terms);
                assert!(
                    s.chars().count() <= budget,
                    "budget {budget} exceeded: {} chars",
                    s.chars().count()
                );
            }
        }
    }

    #[test]
    fn snippet_falls_back_to_prefix_when_no_term_is_located() {
        // 命中来自 `text` 之外的 payload 字段,或 CJK bigram 分词不对应字面子串时
        // 定位不到词 —— 回落到前缀,即旧行为,绝不比原来差,也绝不 panic。
        let text = "a".repeat(300);
        assert_eq!(
            snippet_around_match(&text, 10, &["needle".to_string()]),
            "a".repeat(10)
        );
        assert_eq!(snippet_around_match(&text, 10, &[]), "a".repeat(10));
        // 空 term 不得被当成"在位置 0 命中"。
        assert_eq!(
            snippet_around_match(&text, 10, &[String::new()]),
            "a".repeat(10)
        );
    }

    #[test]
    fn snippet_window_is_char_safe_on_multibyte_text() {
        // 按字符而非字节切,多字节文本不得被切坏(to_lowercase 还会改变字节长度)。
        let terms = vec!["找到".to_string()];
        let text = format!("{}找到{}", "文".repeat(200), "字".repeat(200));
        let s = snippet_around_match(&text, 20, &terms);
        assert!(s.contains("找到"), "{s}");
        assert!(s.chars().count() <= 20, "{s}");
        // 往返 UTF-8 校验：切点没有落在字符中间。
        assert_eq!(String::from_utf8(s.clone().into_bytes()).unwrap(), s);
    }

    #[test]
    fn search_json_escaped_guidance_counts_toward_byte_budget() {
        let mut cat = MapCatalog::new(7);
        for tag in ["hit00", "hit01", "hit02", "hit03"] {
            let text = format!("{} quoted \\\"needle\\\"", "x".repeat(1000));
            cat.insert(
                &hit_id(tag),
                serde_json::json!({ "text": text }).to_string().into_bytes(),
            );
        }
        let app = App::with_clock(cat, PagedIndex { n: 4 }, clock_t0);
        let response = app
            .handle(AppRequest::Search {
                query: "quoted needle".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 10,
                cursor: None,
                budget: ResponseBudget {
                    max_response_bytes: budget::MIN_RESPONSE_BYTES,
                    ..Default::default()
                },
                include_system: false,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap();
        let (kept, next, _, truncation) = hits_of(response);
        assert!(!kept.is_empty() && kept.len() < 4);
        assert_eq!(
            truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_RESPONSE_BYTES)
        );
        assert!(next.is_some());
    }

    #[test]
    fn search_guidance_deterministic_across_identical_queries() {
        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "text": "数据库 guidance" })
                .to_string()
                .into_bytes(),
        );
        let app = App::with_clock(&cat, PagedIndex { n: 1 }, clock_t0);
        let first = app.handle(search_req("数据库 guidance", 10, None)).unwrap();
        let second = app.handle(search_req("数据库 guidance", 10, None)).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn search_guidance_uses_real_session_id_in_suggestions() {
        let mut fixture = ctx_fixture();
        fixture.store.catalog.insert(
            &fixture.leaf,
            serde_json::json!({ "text": "leaf guidance" })
                .to_string()
                .into_bytes(),
        );
        let app = App::with_clock(
            &fixture.store,
            FixedHits(vec![fixture.leaf.clone()]),
            clock_t0,
        );
        let AppResponse::Search { hits, .. } =
            app.handle(search_req("guidance", 10, None)).unwrap()
        else {
            panic!("expected Search response");
        };
        let hit = &hits[0];
        assert_eq!(hit.session_id.as_deref(), Some(fixture.session.as_str()));
        assert_eq!(hit.why_matched, vec!["guidance"]);
        assert!(hit.suggested_next_commands[0].contains(hit.id.as_str()));
        assert!(
            hit.suggested_next_commands
                .iter()
                .all(|command| command.contains(fixture.session.as_str()))
        );
    }

    #[test]
    fn search_guidance_omits_get_message_when_session_id_missing() {
        let mut cat = MapCatalog::new(7);
        cat.insert(
            &hit_id("hit00"),
            serde_json::json!({ "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        let app = App::with_clock(cat, PagedIndex { n: 1 }, clock_t0);
        let AppResponse::Search { hits, .. } = app.handle(search_req("needle", 10, None)).unwrap()
        else {
            panic!("expected Search response");
        };
        assert!(hits[0].session_id.is_none());
        assert!(hits[0].suggested_next_commands.is_empty());
    }

    fn filtered_search_req(
        query: &str,
        limit: usize,
        cursor: Option<String>,
        filters: SearchFilters,
    ) -> AppRequest {
        AppRequest::Search {
            query: query.into(),
            filters,
            facets: SearchFacets::default(),
            limit,
            cursor,
            budget: ResponseBudget::default(),
            include_system: false,
            group_by_session: false,
            mode: RetrievalMode::Lexical,
            query_embedding: None,
            context_lines: None,
        }
    }

    fn seconds_instant(unix_seconds: i64) -> SearchInstant {
        SearchInstant {
            unix_seconds,
            nanosecond: 0,
        }
    }

    #[test]
    fn search_rejects_since_not_before_until() {
        for (since, until) in [
            (seconds_instant(1_000), seconds_instant(1_000)),
            (seconds_instant(2_000), seconds_instant(1_000)),
        ] {
            let err = app()
                .handle(filtered_search_req(
                    "q",
                    5,
                    None,
                    SearchFilters {
                        providers: Vec::new(),
                        since: Some(since),
                        until: Some(until),
                        ..SearchFilters::default()
                    },
                ))
                .expect_err("since >= until must be rejected");
            assert!(matches!(
                err,
                AppError::Domain(DomainError::InvalidRequest(_))
            ));
        }
    }

    #[test]
    fn search_cursor_is_bound_to_normalized_filters() {
        use agent_session_grep_ports::SearchProvider;

        let app = App::with_clock(FakeCatalog, PagedIndex { n: 5 }, clock_t0);
        let issued_filters = SearchFilters {
            providers: vec![SearchProvider::codex(), SearchProvider::claude_code()],
            since: Some(seconds_instant(1_000)),
            until: None,
            ..SearchFilters::default()
        };
        let (_, next, _, _) = hits_of(
            app.handle(filtered_search_req("q", 2, None, issued_filters))
                .unwrap(),
        );
        let normalized_equivalent = SearchFilters {
            providers: vec![
                SearchProvider::claude_code(),
                SearchProvider::codex(),
                SearchProvider::claude_code(),
            ],
            since: Some(seconds_instant(1_000)),
            until: None,
            ..SearchFilters::default()
        };
        assert!(
            app.handle(filtered_search_req(
                "q",
                2,
                next.clone(),
                normalized_equivalent,
            ))
            .is_ok()
        );

        let mutated = SearchFilters {
            providers: Vec::new(),
            since: Some(seconds_instant(2_000)),
            until: None,
            ..SearchFilters::default()
        };
        let err = app
            .handle(filtered_search_req("q", 2, next.clone(), mutated))
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Cursor(cursor::CursorError::Invalid(_))
        ));

        let err = app.handle(search_req("q", 2, next)).unwrap_err();
        assert!(matches!(
            err,
            AppError::Cursor(cursor::CursorError::Invalid(_))
        ));

        let (_, plain_next, _, _) = hits_of(app.handle(search_req("q", 2, None)).unwrap());
        let err = app
            .handle(filtered_search_req(
                "q",
                2,
                plain_next,
                SearchFilters {
                    providers: vec![SearchProvider::claude_code()],
                    since: None,
                    until: None,
                    ..SearchFilters::default()
                },
            ))
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Cursor(cursor::CursorError::Invalid(_))
        ));
    }

    #[test]
    fn search_cursor_is_bound_to_roles_and_exclusion_terms() {
        // A cursor is bound to its query shape. Adding a filter dimension means a
        // token minted under a different role set or a different exclusion set has
        // to be rejected — re-filtering it would hand back a page whose offset was
        // computed against a different ordering.
        use agent_session_grep_ports::SearchRole;

        let app = App::with_clock(FakeCatalog, PagedIndex { n: 5 }, clock_t0);
        let issued = SearchFilters {
            roles: vec![SearchRole::Tool, SearchRole::Assistant],
            exclude_terms: vec!["beta".to_string(), "alpha".to_string()],
            ..SearchFilters::default()
        };
        let (_, next, _, _) = hits_of(
            app.handle(filtered_search_req("q", 2, None, issued))
                .unwrap(),
        );

        // Same sets, different order and with a duplicate: normalization makes
        // these the same query, so the token still reads.
        let normalized_equivalent = SearchFilters {
            roles: vec![
                SearchRole::Assistant,
                SearchRole::Tool,
                SearchRole::Assistant,
            ],
            exclude_terms: vec!["alpha".to_string(), "beta".to_string(), "alpha".to_string()],
            ..SearchFilters::default()
        };
        assert!(
            app.handle(filtered_search_req(
                "q",
                2,
                next.clone(),
                normalized_equivalent
            ))
            .is_ok()
        );

        // Every mutation of either dimension invalidates the token, including
        // dropping it entirely (the unfiltered digest is a different shape).
        use agent_session_grep_ports::SearchRole as Role;
        let mutations = [
            SearchFilters {
                roles: vec![Role::Tool],
                exclude_terms: vec!["alpha".to_string(), "beta".to_string()],
                ..SearchFilters::default()
            },
            SearchFilters {
                roles: vec![Role::Tool, Role::Assistant, Role::User],
                exclude_terms: vec!["alpha".to_string(), "beta".to_string()],
                ..SearchFilters::default()
            },
            SearchFilters {
                roles: vec![Role::Tool, Role::Assistant],
                exclude_terms: vec!["alpha".to_string()],
                ..SearchFilters::default()
            },
            SearchFilters {
                roles: vec![Role::Tool, Role::Assistant],
                exclude_terms: vec!["alpha".to_string(), "gamma".to_string()],
                ..SearchFilters::default()
            },
            SearchFilters {
                roles: vec![Role::Tool, Role::Assistant],
                ..SearchFilters::default()
            },
        ];
        for mutated in mutations {
            let err = app
                .handle(filtered_search_req("q", 2, next.clone(), mutated.clone()))
                .unwrap_err();
            assert!(
                matches!(err, AppError::Cursor(cursor::CursorError::Invalid(_))),
                "{mutated:?} accepted a cursor from a different filter set"
            );
        }
        let err = app.handle(search_req("q", 2, next)).unwrap_err();
        assert!(matches!(
            err,
            AppError::Cursor(cursor::CursorError::Invalid(_))
        ));

        // And the reverse direction: an unfiltered token cannot be replayed with a
        // role or exclusion filter bolted on.
        let (_, plain_next, _, _) = hits_of(app.handle(search_req("q", 2, None)).unwrap());
        let err = app
            .handle(filtered_search_req(
                "q",
                2,
                plain_next,
                SearchFilters {
                    roles: vec![Role::User],
                    ..SearchFilters::default()
                },
            ))
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Cursor(cursor::CursorError::Invalid(_))
        ));
    }

    #[test]
    fn parse_search_instant_normalizes_offsets_and_compact_time() {
        let z = parse_search_instant("2026-07-28T12:00:00Z").unwrap();
        for parsed in [
            parse_search_instant("2026-07-28T14:00:00+02:00").unwrap(),
            parse_search_instant("2026-07-28T140000+0200").unwrap(),
            parse_search_instant("2026-07-28 07:00:00-05:00").unwrap(),
        ] {
            assert_eq!(parsed, z);
        }
        assert_eq!(z.unix_seconds, 1_785_240_000);
        assert_eq!(z.nanosecond, 0);
    }

    #[test]
    fn parse_search_instant_validates_ranges_and_precision() {
        let instant = parse_search_instant("2026-07-28T00:00:00.123456789Z").unwrap();
        assert_eq!(instant.nanosecond, 123_456_789);
        for value in [
            "2026-07-28T12:00:00",
            "2026-02-29T00:00:00Z",
            "2026-07-28T24:00:00Z",
            "2026-07-28T00:00:00.1234567891Z",
            "2026-07-28T12:00Z",
            "1h",
            "",
        ] {
            assert!(parse_search_instant(value).is_none(), "{value:?}");
        }
        assert!(parse_search_instant("2024-02-29T00:00:00Z").is_some());
    }

    #[test]
    fn parse_relative_search_instant_uses_injected_clock_and_rejects_negative_amounts() {
        let now_ms = 1_000_000_000;
        assert_eq!(
            parse_relative_search_instant("1h", now_ms).unwrap(),
            SearchInstant::from_unix_millis(now_ms - 3_600_000)
        );
        assert_eq!(
            parse_relative_search_instant(" 2d ", now_ms).unwrap(),
            SearchInstant::from_unix_millis(now_ms - 2 * 86_400_000)
        );
        for value in ["", "h", "1m", "-1h", "0h", "1.5h"] {
            assert!(parse_relative_search_instant(value, now_ms).is_none());
        }
    }

    fn hits_of(resp: AppResponse) -> (Vec<String>, Option<String>, u64, Truncation) {
        match resp {
            AppResponse::Search {
                hits,
                next_cursor,
                generation,
                truncation,
                retrieval_mode: _,
                fallback_warning: _,
                time_filter_excluded: _,
                context_lines: _,
            } => (
                hits.iter().map(|h| h.id.as_str().to_string()).collect(),
                next_cursor,
                generation,
                truncation,
            ),
            other => panic!("expected Search response, got {other:?}"),
        }
    }

    #[test]
    fn search_pages_partition_the_pinned_ordering() {
        let app = App::with_clock(FakeCatalog, PagedIndex { n: 5 }, clock_t0);
        let (unpaged, _, _, _) = hits_of(app.handle(search_req("q", 5, None)).unwrap());
        assert_eq!(unpaged.len(), 5);

        let mut paged: Vec<String> = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..3 {
            let (ids, next, generation, truncation) =
                hits_of(app.handle(search_req("q", 2, token.take())).unwrap());
            assert_eq!(generation, 7);
            assert!(!truncation.truncated);
            paged.extend(ids);
            token = next;
            if token.is_none() {
                break;
            }
        }
        // 三页恰好不重不漏地划分同一钉住排序。
        assert_eq!(paged, unpaged);
        assert!(token.is_none());
    }

    #[test]
    fn search_cursor_is_bound_to_query() {
        let app = App::with_clock(FakeCatalog, PagedIndex { n: 5 }, clock_t0);
        let (_, next, _, _) = hits_of(app.handle(search_req("alpha", 2, None)).unwrap());
        let err = app
            .handle(search_req("beta", 2, Some(next.unwrap())))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Cursor(cursor::CursorError::Invalid(_))),
            "{err}"
        );
    }

    #[test]
    fn search_cursor_rejects_generation_bump() {
        let cat = MapCatalog::new(7);
        let app = App::with_clock(&cat, PagedIndex { n: 5 }, clock_t0);
        let (_, next, _, _) = hits_of(app.handle(search_req("q", 2, None)).unwrap());
        cat.generation.set(8);
        let err = app.handle(search_req("q", 2, next)).unwrap_err();
        assert!(
            matches!(
                err,
                AppError::Cursor(cursor::CursorError::GenerationMismatch {
                    cursor: 7,
                    active: 8
                })
            ),
            "{err}"
        );
    }

    #[test]
    fn search_cursor_is_bound_to_facets() {
        // 同一查询串、不同 facet 的旧令牌必须失效（设计：facet 绑定进 digest）——
        // 否则换 facet 翻页会跨过滤条件续读。
        let app = App::with_clock(FakeCatalog, PagedIndex { n: 5 }, clock_t0);
        let (_, next, _, _) = hits_of(app.handle(search_req("q", 2, None)).unwrap());
        let err = app
            .handle(search_req_facets(
                "q",
                2,
                next,
                SearchFacets {
                    sidechain: SidechainFacet::MainOnly,
                    ..Default::default()
                },
            ))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Cursor(cursor::CursorError::Invalid(_))),
            "{err}"
        );
    }

    #[test]
    fn search_cursor_expires_after_ttl() {
        let issued = App::with_clock(FakeCatalog, PagedIndex { n: 5 }, clock_t0);
        let (_, next, _, _) = hits_of(issued.handle(search_req("q", 2, None)).unwrap());
        let later = App::with_clock(FakeCatalog, PagedIndex { n: 5 }, clock_after_ttl);
        let err = later.handle(search_req("q", 2, next)).unwrap_err();
        assert!(
            matches!(err, AppError::Cursor(cursor::CursorError::Expired(_))),
            "{err}"
        );
    }

    #[test]
    fn search_byte_gate_truncates_and_cursor_resumes_at_kept_offset() {
        let app = App::with_clock(FakeCatalog, PagedIndex { n: 100 }, clock_t0);
        let resp = app
            .handle(AppRequest::Search {
                query: "q".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 60,
                cursor: None,
                budget: ResponseBudget {
                    max_response_bytes: budget::MIN_RESPONSE_BYTES,
                    ..Default::default()
                },
                include_system: false,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap();
        let (ids, next, _, truncation) = hits_of(resp);
        assert!(!ids.is_empty() && ids.len() < 60, "kept {}", ids.len());
        assert!(truncation.truncated);
        assert_eq!(
            truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_RESPONSE_BYTES)
        );
        // 续读令牌钉在字节闸切断处，而不是页边界。
        let claims = cursor::verify(
            &next.expect("byte-cut page must issue a cursor"),
            &cursor::CursorExpectations {
                now_ms: clock_t0(),
                active_generation: 7,
                query_digest: cursor::digest_query("q"),
                sort_digest: SORT_SCORE_DESC.into(),
                result_set: None,
            },
        )
        .unwrap();
        assert_eq!(claims.offset, ids.len() as u64);
    }

    #[test]
    fn search_rejects_budget_below_floor() {
        let err = app()
            .handle(AppRequest::Search {
                query: "q".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 5,
                cursor: None,
                budget: ResponseBudget {
                    max_items: 0,
                    ..Default::default()
                },
                include_system: false,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap_err();
        assert!(
            matches!(err, AppError::Budget(budget::BudgetError::TooSmall(_))),
            "{err}"
        );
    }

    #[test]
    fn list_pages_partition_in_wire_id_order() {
        let mut cat = MapCatalog::new(7);
        for tag in ["la", "lb", "lc"] {
            let id = StableId::derive(IdKind::Message, Stability::Reconstructed, &[tag.as_bytes()]);
            cat.insert(&id, b"x".to_vec());
        }
        let app = App::with_clock(&cat, FakeIndex, clock_t0);
        let page1 = app
            .handle(AppRequest::List {
                limit: 2,
                cursor: None,
                budget: ResponseBudget::default(),
                sessions_only: false,
                sort: ListSort::WireIdAsc,
            })
            .unwrap();
        let AppResponse::List {
            entries,
            next_cursor,
            ..
        } = page1
        else {
            panic!("expected List response");
        };
        assert_eq!(entries.len(), 2);
        let page2 = app
            .handle(AppRequest::List {
                limit: 2,
                cursor: next_cursor,
                budget: ResponseBudget::default(),
                sessions_only: false,
                sort: ListSort::WireIdAsc,
            })
            .unwrap();
        let AppResponse::List {
            entries: entries2,
            next_cursor: next2,
            ..
        } = page2
        else {
            panic!("expected List response");
        };
        assert_eq!(entries2.len(), 1);
        assert!(next2.is_none());
        let mut all: Vec<String> = entries
            .iter()
            .chain(entries2.iter())
            .map(|e| e.id.as_str().to_string())
            .collect();
        let sorted = {
            let mut s = all.clone();
            s.sort();
            s
        };
        // 两页拼接即完整 wire id 升序（钉住排序稳定，无重无漏）。
        assert_eq!(all.len(), 3);
        assert_eq!(all, sorted);
        all.dedup();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn list_byte_gate_counts_serialized_payload_inflation() {
        // payload 含控制字节时，最终 JSON 里每个字节膨胀为 ``（6 字符）；
        // 估算按序列化后长度计（约 1.8 KB/条），净预算 3072 只容一条——若按
        // 原始字节数估算（约 360 B/条）会错误地放行全部四条。
        let mut cat = MapCatalog::new(7);
        let payload = vec![1u8; 300];
        for tag in ["pa", "pb", "pc", "pd"] {
            let id = StableId::derive(IdKind::Message, Stability::Reconstructed, &[tag.as_bytes()]);
            cat.insert(&id, payload.clone());
        }
        let app = App::with_clock(&cat, FakeIndex, clock_t0);
        let resp = app
            .handle(AppRequest::List {
                limit: 10,
                cursor: None,
                budget: ResponseBudget {
                    max_response_bytes: budget::MIN_RESPONSE_BYTES,
                    ..Default::default()
                },
                sessions_only: false,
                sort: ListSort::WireIdAsc,
            })
            .unwrap();
        let AppResponse::List {
            entries,
            truncation,
            ..
        } = resp
        else {
            panic!("expected List response");
        };
        assert_eq!(entries.len(), 1);
        assert!(truncation.truncated);
        assert_eq!(
            truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_RESPONSE_BYTES)
        );
    }

    // ---- recency 浏览（M3-4）----

    /// 三个会话：两个有时间戳、一个没有。构造顺序与期望顺序都不等于 wire id 序，
    /// 因此断言真的在验证排序维度，而不是碰巧命中默认序。
    fn recency_catalog() -> MapCatalog {
        let mut cat = MapCatalog::new(7);
        let older = StableId::native(IdKind::Session, "recency-older");
        let newer = StableId::native(IdKind::Session, "recency-newer");
        let undated = StableId::native(IdKind::Session, "recency-undated");
        for id in [&older, &newer, &undated] {
            cat.insert(id, b"{}".to_vec());
        }
        cat.set_latest_activity(&older, "2026-08-17T09:00:00Z");
        cat.set_latest_activity(&newer, "2026-08-18T21:30:00Z");
        cat
    }

    fn recency_request(cursor: Option<String>, limit: usize) -> AppRequest {
        AppRequest::List {
            limit,
            cursor,
            budget: ResponseBudget::default(),
            sessions_only: true,
            sort: ListSort::RecencyDesc,
        }
    }

    #[test]
    fn list_recency_sorts_newest_first_and_keeps_timestampless_sessions_last() {
        let cat = recency_catalog();
        let app = App::with_clock(&cat, FakeIndex, clock_t0);
        let AppResponse::List { entries, .. } = app.handle(recency_request(None, 10)).unwrap()
        else {
            panic!("expected List response");
        };
        let order: Vec<(String, Option<String>)> = entries
            .iter()
            .map(|entry| (entry.id.as_str().to_string(), entry.latest_activity.clone()))
            .collect();
        let expected: Vec<(String, Option<String>)> = vec![
            (
                StableId::native(IdKind::Session, "recency-newer")
                    .as_str()
                    .to_string(),
                Some("2026-08-18T21:30:00Z".to_string()),
            ),
            (
                StableId::native(IdKind::Session, "recency-older")
                    .as_str()
                    .to_string(),
                Some("2026-08-17T09:00:00Z".to_string()),
            ),
            // 缺时间戳的会话既不被丢弃、也不冒充最新：排在末尾且值为 null。
            (
                StableId::native(IdKind::Session, "recency-undated")
                    .as_str()
                    .to_string(),
                None,
            ),
        ];
        assert_eq!(order, expected);
    }

    #[test]
    fn list_recency_pages_partition_the_recency_order() {
        let cat = recency_catalog();
        let app = App::with_clock(&cat, FakeIndex, clock_t0);
        let AppResponse::List {
            entries,
            next_cursor,
            ..
        } = app.handle(recency_request(None, 2)).unwrap()
        else {
            panic!("expected List response");
        };
        assert_eq!(entries.len(), 2);
        let token = next_cursor.expect("recency list must page");
        let AppResponse::List {
            entries: page2,
            next_cursor: next2,
            ..
        } = app.handle(recency_request(Some(token), 2)).unwrap()
        else {
            panic!("expected List response");
        };
        assert_eq!(page2.len(), 1);
        assert!(next2.is_none());
        // 两页拼接 = 完整 recency 序，无重无漏（第二页正是缺时间戳的那条）。
        let joined: Vec<Option<&str>> = entries
            .iter()
            .chain(page2.iter())
            .map(|entry| entry.latest_activity.as_deref())
            .collect();
        assert_eq!(
            joined,
            vec![
                Some("2026-08-18T21:30:00Z"),
                Some("2026-08-17T09:00:00Z"),
                None
            ]
        );
    }

    #[test]
    fn list_cursor_rejects_sort_dimension_mismatch() {
        // 排序维度绑进 sort_digest：recency 令牌不得在 wire-id 序列上续读，
        // 反之亦然——否则第二页会按另一个顺序切片，静默返回错的记录。
        let cat = recency_catalog();
        let app = App::with_clock(&cat, FakeIndex, clock_t0);
        let wire_id_request = |cursor: Option<String>| AppRequest::List {
            limit: 2,
            cursor,
            budget: ResponseBudget::default(),
            sessions_only: true,
            sort: ListSort::WireIdAsc,
        };

        let AppResponse::List { next_cursor, .. } = app.handle(recency_request(None, 2)).unwrap()
        else {
            panic!("expected List response");
        };
        let recency_token = next_cursor.expect("recency list must page");
        let err = app
            .handle(wire_id_request(Some(recency_token)))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Cursor(cursor::CursorError::Invalid(_))),
            "{err}"
        );
        assert!(err.to_string().contains("different sort order"), "{err}");

        let AppResponse::List { next_cursor, .. } = app.handle(wire_id_request(None)).unwrap()
        else {
            panic!("expected List response");
        };
        let wire_token = next_cursor.expect("wire-id list must page");
        let err = app
            .handle(recency_request(Some(wire_token), 2))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Cursor(cursor::CursorError::Invalid(_))),
            "{err}"
        );
        assert!(err.to_string().contains("different sort order"), "{err}");
    }

    #[test]
    fn list_recency_requires_sessions_only_and_names_the_command() {
        let cat = recency_catalog();
        let app = App::with_clock(&cat, FakeIndex, clock_t0);
        let err = app
            .handle(AppRequest::List {
                limit: 10,
                cursor: None,
                budget: ResponseBudget::default(),
                sessions_only: false,
                sort: ListSort::RecencyDesc,
            })
            .unwrap_err();
        assert!(
            matches!(err, AppError::Domain(DomainError::InvalidRequest(_))),
            "{err}"
        );
        // 错误必须给出可照抄的下一步命令，而不是只说"不支持"。
        assert!(
            err.to_string().contains("list --sessions --sort recency"),
            "{err}"
        );
    }

    #[test]
    fn list_wire_id_sort_reports_no_activity_projection() {
        // wire-id 维度不做时间投影：`latest_activity` 保持 None，而不是填一个
        // 看起来像事实的值。
        let cat = recency_catalog();
        let app = App::with_clock(&cat, FakeIndex, clock_t0);
        let AppResponse::List { entries, .. } = app
            .handle(AppRequest::List {
                limit: 10,
                cursor: None,
                budget: ResponseBudget::default(),
                sessions_only: true,
                sort: ListSort::WireIdAsc,
            })
            .unwrap()
        else {
            panic!("expected List response");
        };
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|entry| entry.latest_activity.is_none()));
    }

    #[test]
    fn list_sort_digests_are_distinct_and_stable() {
        assert_eq!(ListSort::WireIdAsc.digest(), SORT_WIRE_ID_ASC);
        assert_eq!(ListSort::RecencyDesc.digest(), SORT_RECENCY_DESC);
        assert_ne!(ListSort::WireIdAsc.digest(), ListSort::RecencyDesc.digest());
        assert_eq!(ListSort::default(), ListSort::WireIdAsc);
    }

    // ---- context 装配 ----

    fn ctx_msg_payload(role: &str, text: &str, timestamp: &str) -> Vec<u8> {
        serde_json::json!({
            "role": role,
            "text": text,
            "timestamp": timestamp,
            // Deliberately misleading compatibility aliases. Context must use
            // the typed graph, never these values.
            "parent": null,
            "is_sidechain": false,
            "span": {"start": 90, "end": 99},
        })
        .to_string()
        .into_bytes()
    }

    fn context_message(id: StableId, role: Role, text: &str, timestamp: &str) -> Message {
        Message {
            id,
            role,
            text: text.into(),
            timestamp: Some(timestamp.into()),
        }
    }

    fn context_document(id: StableId, fingerprint: &str) -> SourceDocument {
        SourceDocument {
            id,
            provider_id: "test-provider".into(),
            variant_id: "test-provider/v1".into(),
            fingerprint: fingerprint.into(),
            len: 200,
        }
    }

    struct GraphCatalog {
        catalog: MapCatalog,
        graph: SessionContextGraph,
        context_message_id: StableId,
        candidates: Vec<PortMessageContextCandidate>,
        stats: ContextStats,
    }

    impl CatalogStore for GraphCatalog {
        fn get(&self, id: &StableId) -> PortResult<Option<Vec<u8>>> {
            self.catalog.get(id)
        }

        fn get_many(&self, ids: &[StableId]) -> PortResult<Vec<(StableId, Option<Vec<u8>>)>> {
            self.catalog.get_many(ids)
        }

        fn put(&self, id: &StableId, payload: &[u8]) -> PortResult<()> {
            self.catalog.put(id, payload)
        }

        fn list(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
            self.catalog.list(limit)
        }

        fn list_sessions(&self, limit: usize) -> PortResult<Vec<CatalogEntry>> {
            self.catalog.list_sessions(limit)
        }

        fn sessions_by_recency(&self, limit: usize) -> PortResult<Vec<(StableId, Option<String>)>> {
            self.catalog.sessions_by_recency(limit)
        }

        fn count(&self) -> PortResult<u64> {
            self.catalog.count()
        }

        fn active_generation(&self) -> PortResult<u64> {
            self.catalog.active_generation()
        }
    }

    impl ContextGraphStore for GraphCatalog {
        fn load_session_graph(&self, session_id: &StableId) -> PortResult<SessionContextGraph> {
            if session_id.as_str() == self.graph.session_id.as_str() {
                Ok(self.graph.clone())
            } else {
                Err(PortError::NotFound("session context not found".into()))
            }
        }

        fn message_contexts(
            &self,
            message_id: &StableId,
        ) -> PortResult<Vec<PortMessageContextCandidate>> {
            if message_id.as_str() == self.context_message_id.as_str() {
                Ok(self.candidates.clone())
            } else {
                // Port contract: a message absent from the catalog is a
                // lookup miss, never an empty success.
                Err(PortError::NotFound("message not found".into()))
            }
        }

        fn session_of(&self, ids: &[StableId]) -> PortResult<Vec<(StableId, Option<StableId>)>> {
            // 与 SqliteStore 语义一致：取该消息所有 placement 中 wire id 字典序
            // 最小的会话；无 placement → None。
            let owners = self.graph.placements.iter().fold(
                BTreeMap::<String, String>::new(),
                |mut owners, placement| {
                    owners
                        .entry(placement.message_id.as_str().to_string())
                        .and_modify(|owner| {
                            if placement.session_id.as_str() < owner.as_str() {
                                *owner = placement.session_id.as_str().to_string();
                            }
                        })
                        .or_insert_with(|| placement.session_id.as_str().to_string());
                    owners
                },
            );
            Ok(ids
                .iter()
                .map(|id| {
                    let session = owners
                        .get(id.as_str())
                        .and_then(|wire| StableId::from_wire(wire));
                    (id.clone(), session)
                })
                .collect())
        }

        fn source_placements_of(
            &self,
            ids: &[StableId],
        ) -> PortResult<Vec<(StableId, Option<SourcePlacement>)>> {
            // 与 SqliteStore 语义一致：取该消息所有 placement 中 source_document_id
            // 字典序最小的 placement 作为权威来源（确定、稳定）。
            let mut best: BTreeMap<String, SourcePlacement> = BTreeMap::new();
            for placement in &self.graph.placements {
                let entry = best
                    .entry(placement.message_id.as_str().to_string())
                    .or_insert_with(|| SourcePlacement {
                        source_document_id: placement.source_document_id.clone(),
                        byte_start: placement.span.as_ref().map(|s| s.start),
                        byte_end: placement.span.as_ref().map(|s| s.end),
                    });
                if placement.source_document_id.as_str() < entry.source_document_id.as_str() {
                    entry.source_document_id = placement.source_document_id.clone();
                    entry.byte_start = placement.span.as_ref().map(|s| s.start);
                    entry.byte_end = placement.span.as_ref().map(|s| s.end);
                }
            }
            Ok(ids
                .iter()
                .map(|id| (id.clone(), best.get(id.as_str()).cloned()))
                .collect())
        }

        fn context_stats(&self) -> PortResult<ContextStats> {
            Ok(self.stats)
        }
    }

    struct ContextFixture {
        store: GraphCatalog,
        session: StableId,
        document_a: StableId,
        document_b: StableId,
        root: StableId,
        repeated: StableId,
        sidechain: StableId,
        leaf: StableId,
        root_placement: MessagePlacement,
        repeated_a: MessagePlacement,
        repeated_b: MessagePlacement,
        leaf_placement: MessagePlacement,
    }

    /// Graph shape:
    ///
    /// - one stable Message has two placements in one Session;
    /// - the repeated placement in document B is the parent of the real leaf;
    /// - a sidechain placement exists but is excluded from mainline;
    /// - payload aliases disagree with graph facts and must be ignored.
    fn ctx_fixture() -> ContextFixture {
        let session = StableId::native(IdKind::Session, "sess-ctx");
        let document_a = StableId::native(IdKind::Document, "ctx-doc-a");
        let document_b = StableId::native(IdKind::Document, "ctx-doc-b");
        let root = StableId::native(IdKind::Message, "ctx-root");
        let repeated = StableId::native(IdKind::Message, "ctx-repeated");
        let sidechain = StableId::native(IdKind::Message, "ctx-side");
        let leaf = StableId::native(IdKind::Message, "ctx-leaf");

        let root_message =
            context_message(root.clone(), Role::User, "root", "2026-07-26T00:00:00Z");
        let repeated_message = context_message(
            repeated.clone(),
            Role::Assistant,
            "repeated",
            "2026-07-26T00:01:00Z",
        );
        let sidechain_message = context_message(
            sidechain.clone(),
            Role::Assistant,
            "side",
            "2026-07-26T00:01:30Z",
        );
        let leaf_message =
            context_message(leaf.clone(), Role::User, "leaf", "2026-07-26T00:02:00Z");
        let source_a = context_document(document_a.clone(), "b3-doc-a");
        let source_b = context_document(document_b.clone(), "b3-doc-b");

        let root_placement = MessagePlacement::new(
            session.clone(),
            document_a.clone(),
            root.clone(),
            0,
            false,
            Some(EvidenceSpan { start: 0, end: 10 }),
        );
        let repeated_a = MessagePlacement::new(
            session.clone(),
            document_a.clone(),
            repeated.clone(),
            1,
            false,
            Some(EvidenceSpan { start: 11, end: 25 }),
        );
        let sidechain_placement = MessagePlacement::new(
            session.clone(),
            document_a.clone(),
            sidechain.clone(),
            2,
            true,
            Some(EvidenceSpan { start: 26, end: 40 }),
        );
        let repeated_b = MessagePlacement::new(
            session.clone(),
            document_b.clone(),
            repeated.clone(),
            0,
            false,
            Some(EvidenceSpan { start: 5, end: 19 }),
        );
        let leaf_placement = MessagePlacement::new(
            session.clone(),
            document_b.clone(),
            leaf.clone(),
            1,
            false,
            Some(EvidenceSpan { start: 20, end: 40 }),
        );
        let edges = vec![
            MessageEdge {
                child_placement_id: repeated_a.id.clone(),
                parent_message_id: root.clone(),
                parent_native_id: Some("ctx-root".into()),
                relation: MessageRelation::Reply,
            },
            MessageEdge {
                child_placement_id: repeated_b.id.clone(),
                parent_message_id: root.clone(),
                parent_native_id: Some("ctx-root".into()),
                relation: MessageRelation::Reply,
            },
            MessageEdge {
                child_placement_id: sidechain_placement.id.clone(),
                parent_message_id: repeated.clone(),
                parent_native_id: Some("ctx-repeated".into()),
                relation: MessageRelation::Reply,
            },
            MessageEdge {
                child_placement_id: leaf_placement.id.clone(),
                parent_message_id: repeated.clone(),
                parent_native_id: Some("ctx-repeated".into()),
                relation: MessageRelation::Reply,
            },
        ];
        let graph = SessionContextGraph {
            session_id: session.clone(),
            messages: vec![
                root_message,
                repeated_message,
                sidechain_message,
                leaf_message,
            ],
            source_documents: vec![source_a, source_b],
            placements: vec![
                root_placement.clone(),
                repeated_a.clone(),
                sidechain_placement.clone(),
                repeated_b.clone(),
                leaf_placement.clone(),
            ],
            edges,
        };
        graph.validate().unwrap();

        let mut catalog = MapCatalog::new(7);
        catalog.insert(
            &root,
            ctx_msg_payload("user", "root", "2026-07-26T00:00:00Z"),
        );
        catalog.insert(
            &repeated,
            ctx_msg_payload("assistant", "repeated", "2026-07-26T00:01:00Z"),
        );
        catalog.insert(
            &sidechain,
            ctx_msg_payload("assistant", "side", "2026-07-26T00:01:30Z"),
        );
        catalog.insert(
            &leaf,
            ctx_msg_payload("user", "leaf", "2026-07-26T00:02:00Z"),
        );
        catalog.insert(
            &session,
            serde_json::json!({
                "document": "doc_v1_wrong-compatibility-alias",
                "messages": [],
            })
            .to_string()
            .into_bytes(),
        );

        let store = GraphCatalog {
            catalog,
            graph,
            context_message_id: repeated.clone(),
            candidates: vec![
                PortMessageContextCandidate {
                    session_id: session.clone(),
                    placement_ids: vec![repeated_b.id.clone()],
                },
                PortMessageContextCandidate {
                    session_id: session.clone(),
                    placement_ids: vec![repeated_a.id.clone(), repeated_b.id.clone()],
                },
            ],
            stats: ContextStats {
                placements: 5,
                source_placement_claims: 6,
            },
        };
        ContextFixture {
            store,
            session,
            document_a,
            document_b,
            root,
            repeated,
            sidechain,
            leaf,
            root_placement,
            repeated_a,
            repeated_b,
            leaf_placement,
        }
    }

    fn ctx_req(ses: &StableId, policy: ContextPolicy, budget: ResponseBudget) -> AppRequest {
        AppRequest::Context {
            session_id: ses.clone(),
            policy,
            level: ContextLevel::Raw,
            budget,
        }
    }

    fn ctx_req_level(
        ses: &StableId,
        policy: ContextPolicy,
        level: ContextLevel,
        budget: ResponseBudget,
    ) -> AppRequest {
        AppRequest::Context {
            session_id: ses.clone(),
            policy,
            level,
            budget,
        }
    }

    #[test]
    fn context_mainline_uses_typed_edges_and_exact_placement_evidence() {
        let fixture = ctx_fixture();
        let resp = app_ctx(&fixture.store)
            .handle(ctx_req(
                &fixture.session,
                ContextPolicy::Mainline,
                ResponseBudget::default(),
            ))
            .unwrap();
        let AppResponse::Context {
            session_id,
            branch_leaf,
            branch_leaf_placement_id,
            messages,
            evidence,
            truncation,
            generation,
            ..
        } = resp
        else {
            panic!("expected Context response");
        };
        assert_eq!(session_id, fixture.session.as_str());
        assert_eq!(generation, 7);
        assert!(!truncation.truncated);
        assert_eq!(branch_leaf.as_deref(), Some(fixture.leaf.as_str()));
        assert_eq!(
            branch_leaf_placement_id.as_deref(),
            Some(fixture.leaf_placement.id.as_str())
        );
        let message_ids: Vec<&str> = messages
            .iter()
            .map(|message| message.message_id.as_str())
            .collect();
        assert_eq!(
            message_ids,
            vec![
                fixture.root.as_str(),
                fixture.repeated.as_str(),
                fixture.leaf.as_str()
            ]
        );
        assert_eq!(
            messages[1].placement_id,
            fixture.repeated_b.id.as_str(),
            "same-document parent resolution must choose document B"
        );
        assert!(
            messages
                .iter()
                .all(|message| message.id == message.message_id)
        );

        assert_eq!(evidence.len(), 3);
        assert_eq!(evidence[0].precision, evidence::Precision::Byte);
        assert_eq!(evidence[0].byte_start, Some(0));
        assert_eq!(evidence[1].byte_start, Some(5));
        assert_eq!(evidence[2].byte_end, Some(40));
        assert_eq!(evidence[2].record_ordinal, Some(1));
        assert_eq!(evidence[0].source_fingerprint.as_deref(), Some("b3-doc-a"));
        assert_eq!(evidence[1].source_fingerprint.as_deref(), Some("b3-doc-b"));
        assert_eq!(
            evidence[0].source_document_id.as_deref(),
            Some(fixture.document_a.as_str())
        );
        assert_eq!(
            evidence[1].source_document_id.as_deref(),
            Some(fixture.document_b.as_str())
        );
        assert_eq!(
            evidence
                .iter()
                .map(|span| span.occurrence_id.as_str())
                .collect::<Vec<_>>(),
            messages
                .iter()
                .map(|message| message.placement_id.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn context_full_keeps_repeated_stable_messages_as_distinct_occurrences() {
        let fixture = ctx_fixture();
        let resp = app_ctx(&fixture.store)
            .handle(ctx_req(
                &fixture.session,
                ContextPolicy::Full,
                ResponseBudget::default(),
            ))
            .unwrap();
        let AppResponse::Context {
            messages,
            branch_leaf,
            branch_leaf_placement_id,
            evidence,
            ..
        } = resp
        else {
            panic!("expected Context response");
        };
        let wires: Vec<&str> = messages
            .iter()
            .map(|message| message.message_id.as_str())
            .collect();
        assert_eq!(
            wires,
            vec![
                fixture.root.as_str(),
                fixture.repeated.as_str(),
                fixture.repeated.as_str(),
                fixture.sidechain.as_str(),
                fixture.leaf.as_str(),
            ]
        );
        assert_ne!(messages[1].placement_id, messages[2].placement_id);
        assert_eq!(messages[1].placement_id, fixture.repeated_a.id.as_str());
        assert_eq!(messages[2].placement_id, fixture.repeated_b.id.as_str());
        assert_eq!(evidence.len(), 5);
        assert_eq!(evidence[1].message_id, evidence[2].message_id);
        assert_ne!(evidence[1].occurrence_id, evidence[2].occurrence_id);
        assert_eq!(branch_leaf.as_deref(), Some(fixture.leaf.as_str()));
        assert_eq!(
            branch_leaf_placement_id.as_deref(),
            Some(fixture.leaf_placement.id.as_str())
        );
    }

    #[test]
    fn context_budget_counts_placement_occurrences() {
        let fixture = ctx_fixture();
        let resp = app_ctx(&fixture.store)
            .handle(ctx_req(
                &fixture.session,
                ContextPolicy::Full,
                ResponseBudget {
                    max_messages: 3,
                    ..Default::default()
                },
            ))
            .unwrap();
        let AppResponse::Context {
            messages,
            evidence,
            truncation,
            ..
        } = resp
        else {
            panic!("expected Context response");
        };
        assert_eq!(messages.len(), 3);
        assert_eq!(evidence.len(), 3);
        assert_eq!(messages[1].message_id, messages[2].message_id);
        assert_ne!(messages[1].placement_id, messages[2].placement_id);
        assert!(truncation.truncated);
        assert_eq!(
            truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_MESSAGES)
        );
    }

    #[test]
    fn context_evidence_span_budget_reports_reason() {
        let fixture = ctx_fixture();
        let resp = app_ctx(&fixture.store)
            .handle(ctx_req(
                &fixture.session,
                ContextPolicy::Mainline,
                ResponseBudget {
                    max_evidence_spans: 1,
                    ..Default::default()
                },
            ))
            .unwrap();
        let AppResponse::Context {
            messages,
            evidence,
            truncation,
            ..
        } = resp
        else {
            panic!("expected Context response");
        };
        assert_eq!(messages.len(), 3);
        assert_eq!(evidence.len(), 1);
        assert!(truncation.truncated);
        assert_eq!(
            truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_EVIDENCE_SPANS)
        );
    }

    #[test]
    fn context_reports_both_knobs_when_messages_and_evidence_truncate() {
        // 两道闸同时触发时两个旋钮名都要报，不能只报 max_messages。
        let fixture = ctx_fixture();
        let resp = app_ctx(&fixture.store)
            .handle(ctx_req(
                &fixture.session,
                ContextPolicy::Full,
                ResponseBudget {
                    max_messages: 3,
                    max_evidence_spans: 1,
                    ..Default::default()
                },
            ))
            .unwrap();
        let AppResponse::Context {
            messages,
            evidence,
            truncation,
            ..
        } = resp
        else {
            panic!("expected Context response");
        };
        assert_eq!(messages.len(), 3);
        assert_eq!(evidence.len(), 1);
        assert!(truncation.truncated);
        assert_eq!(
            truncation.reason.as_deref(),
            Some("max_messages,max_evidence_spans")
        );
    }

    #[test]
    fn context_missing_session_is_not_found() {
        let fixture = ctx_fixture();
        let missing = StableId::native(IdKind::Session, "no-such-session");
        let err = app_ctx(&fixture.store)
            .handle(ctx_req(
                &missing,
                ContextPolicy::Mainline,
                ResponseBudget::default(),
            ))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Port(PortError::NotFound(_))),
            "{err}"
        );
    }

    #[test]
    fn context_malformed_session_payload_is_invariant_violation() {
        let mut fixture = ctx_fixture();
        fixture
            .store
            .catalog
            .insert(&fixture.session, b"user\tnot canonical json".to_vec());
        let err = app_ctx(&fixture.store)
            .handle(ctx_req(
                &fixture.session,
                ContextPolicy::Mainline,
                ResponseBudget::default(),
            ))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Domain(DomainError::InvariantViolation(_))),
            "{err}"
        );
    }

    #[test]
    fn message_context_candidates_are_grouped_by_distinct_session() {
        let mut fixture = ctx_fixture();
        let second_session = StableId::native(IdKind::Session, "sess-other");
        fixture.store.candidates.push(PortMessageContextCandidate {
            session_id: second_session.clone(),
            placement_ids: vec![fixture.root_placement.id.clone()],
        });

        let response = app_ctx(&fixture.store)
            .handle(AppRequest::MessageContexts {
                message_id: fixture.repeated.clone(),
            })
            .unwrap();
        let AppResponse::MessageContexts {
            message_id,
            candidates,
        } = response
        else {
            panic!("expected MessageContexts response");
        };
        assert_eq!(message_id, fixture.repeated.as_str());
        assert_eq!(candidates.len(), 2);
        let primary = candidates
            .iter()
            .find(|candidate| candidate.session_id == fixture.session.as_str())
            .unwrap();
        let expected: BTreeSet<String> = [
            fixture.repeated_a.id.as_str().to_string(),
            fixture.repeated_b.id.as_str().to_string(),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            primary
                .placement_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.session_id == second_session.as_str())
        );
    }

    #[test]
    fn message_contexts_missing_message_is_not_found() {
        // Port contract: a message absent from the catalog is a lookup miss
        // (NotFound), never an empty success.
        let fixture = ctx_fixture();
        let index_only = StableId::native(IdKind::Message, "index-only");
        let err = app_ctx(&fixture.store)
            .handle(AppRequest::MessageContexts {
                message_id: index_only.clone(),
            })
            .unwrap_err();
        assert!(
            matches!(err, AppError::Port(PortError::NotFound(_))),
            "missing message must map to NotFound, got {err:?}"
        );
    }

    fn app_ctx(cat: &GraphCatalog) -> App<&GraphCatalog, FakeIndex> {
        App::with_clock(cat, FakeIndex, clock_t0)
    }

    #[test]
    fn context_talks_group_user_message_with_following_messages() {
        let fixture = ctx_fixture();
        let AppResponse::Context {
            requested_level,
            effective_level,
            talks,
            summary,
            hint,
            messages,
            ..
        } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Talks,
                ResponseBudget::default(),
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        assert_eq!(requested_level, ContextLevel::Talks);
        assert_eq!(effective_level, ContextLevel::Talks);
        assert!(summary.is_none());
        assert_eq!(talks.len(), 2);
        assert_eq!(talks[0].user_message.message_id, fixture.root.as_str());
        assert_eq!(
            talks[0]
                .following_messages
                .iter()
                .map(|message| message.message_id.as_str())
                .collect::<Vec<_>>(),
            vec![fixture.repeated.as_str()]
        );
        assert_eq!(talks[1].user_message.message_id, fixture.leaf.as_str());
        assert!(talks[1].following_messages.is_empty());
        assert_eq!(messages.len(), 3);
        let hint = hint.expect("talks must hint toward raw");
        assert_eq!(hint.command, "get_session_context");
        assert_eq!(hint.session_id, fixture.session.as_str());
        assert_eq!(hint.level, ContextLevel::Raw);
    }

    #[test]
    fn context_talks_only_include_assistant_and_tool_followers() {
        for (role, payload_role, included) in [
            (Role::Assistant, "assistant", true),
            (Role::Tool, "tool", true),
            (Role::System, "system", false),
            (Role::Developer, "developer", false),
        ] {
            let mut fixture = ctx_fixture();
            fixture.store.catalog.insert(
                &fixture.repeated,
                ctx_msg_payload(payload_role, payload_role, "2026-07-26T00:01:00Z"),
            );
            fixture
                .store
                .graph
                .messages
                .iter_mut()
                .find(|message| message.id == fixture.repeated)
                .expect("repeated message")
                .role = role;
            let AppResponse::Context {
                talks, messages, ..
            } = app_ctx(&fixture.store)
                .handle(ctx_req_level(
                    &fixture.session,
                    ContextPolicy::Mainline,
                    ContextLevel::Talks,
                    ResponseBudget::default(),
                ))
                .unwrap()
            else {
                panic!("expected Context response");
            };
            assert_eq!(talks.len(), 2);
            assert_eq!(talks[0].following_messages.len(), included as usize);
            assert_eq!(messages.len(), 3);
        }
    }

    #[test]
    fn context_sessions_returns_structural_overview_and_talk_hint() {
        let fixture = ctx_fixture();
        let AppResponse::Context {
            requested_level,
            effective_level,
            talks,
            summary,
            hint,
            ..
        } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Sessions,
                ResponseBudget::default(),
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        assert_eq!(requested_level, ContextLevel::Sessions);
        assert_eq!(effective_level, ContextLevel::Sessions);
        let summary = summary.expect("sessions must produce a summary");
        assert_eq!(
            summary.first_user_message.unwrap().message_id,
            fixture.root.as_str()
        );
        assert_eq!(summary.message_count, 3);
        assert_eq!(summary.turn_count, 2);
        assert_eq!(
            summary.file_references,
            vec![
                fixture.document_a.as_str().to_string(),
                fixture.document_b.as_str().to_string(),
            ]
        );
        assert!(talks.is_empty());
        let hint = hint.expect("sessions must hint toward talks");
        assert_eq!(hint.command, "get_session_context");
        assert_eq!(hint.session_id, fixture.session.as_str());
        assert_eq!(hint.level, ContextLevel::Talks);
    }

    #[test]
    fn context_talks_and_sessions_fall_back_toward_raw_without_user() {
        let mut fixture = ctx_fixture();
        fixture
            .store
            .graph
            .messages
            .iter_mut()
            .for_each(|message| message.role = Role::Assistant);
        for requested in [ContextLevel::Talks, ContextLevel::Sessions] {
            let AppResponse::Context {
                requested_level,
                effective_level,
                talks,
                summary,
                hint,
                messages,
                ..
            } = app_ctx(&fixture.store)
                .handle(ctx_req_level(
                    &fixture.session,
                    ContextPolicy::Mainline,
                    requested,
                    ResponseBudget::default(),
                ))
                .unwrap()
            else {
                panic!("expected Context response");
            };
            assert_eq!(requested_level, requested);
            assert_eq!(effective_level, ContextLevel::Raw);
            assert!(talks.is_empty());
            assert!(summary.is_none());
            assert!(hint.is_none());
            assert_eq!(messages.len(), 3);
        }
    }

    #[test]
    fn context_sessions_budget_falls_back_through_talks_before_raw() {
        let mut fixture = ctx_fixture();
        fixture
            .store
            .graph
            .messages
            .iter_mut()
            .find(|message| message.id == fixture.root)
            .expect("root message")
            .role = Role::System;
        let mut response = None;
        for text_len in 1000..=2000 {
            fixture.store.catalog.insert(
                &fixture.root,
                ctx_msg_payload("system", &"x".repeat(text_len), "2026-07-26T00:00:00Z"),
            );
            let candidate = app_ctx(&fixture.store)
                .handle(ctx_req_level(
                    &fixture.session,
                    ContextPolicy::Mainline,
                    ContextLevel::Sessions,
                    ResponseBudget {
                        max_response_bytes: budget::MIN_RESPONSE_BYTES,
                        ..Default::default()
                    },
                ))
                .unwrap();
            if matches!(
                &candidate,
                AppResponse::Context {
                    effective_level: ContextLevel::Talks,
                    ..
                }
            ) {
                response = Some(candidate);
                break;
            }
        }
        let AppResponse::Context {
            requested_level,
            effective_level,
            talks,
            summary,
            hint,
            truncation,
            ..
        } = response.expect("a boundary must exist where sessions falls back to talks")
        else {
            panic!("expected Context response");
        };
        assert_eq!(requested_level, ContextLevel::Sessions);
        assert_eq!(effective_level, ContextLevel::Talks);
        assert_eq!(talks.len(), 1);
        assert!(summary.is_none());
        assert_eq!(
            hint.expect("talks fallback must hint raw").level,
            ContextLevel::Raw
        );
        assert!(truncation.truncated);
        assert!(
            truncation
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains(budget::TRUNCATION_MAX_RESPONSE_BYTES))
        );
    }

    #[test]
    fn context_clamp_reports_public_max_messages_reason() {
        let fixture = ctx_fixture();
        let AppResponse::Context { truncation, .. } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Talks,
                ResponseBudget {
                    max_messages: 1,
                    ..Default::default()
                },
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        assert!(truncation.truncated);
        assert_eq!(
            truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_MESSAGES)
        );
        assert!(
            !truncation
                .reason
                .unwrap()
                .contains(budget::TRUNCATION_MAX_ITEMS)
        );
    }

    #[test]
    fn context_fallback_does_not_charge_unproduced_derived_bytes() {
        let mut fixture = ctx_fixture();
        fixture
            .store
            .graph
            .messages
            .iter_mut()
            .for_each(|message| message.role = Role::Assistant);
        let floor_budget = || ResponseBudget {
            max_response_bytes: budget::MIN_RESPONSE_BYTES,
            ..Default::default()
        };
        let AppResponse::Context {
            messages: raw_messages,
            truncation: raw_truncation,
            ..
        } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Raw,
                floor_budget(),
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        let AppResponse::Context {
            effective_level,
            messages: fallback_messages,
            truncation: fallback_truncation,
            ..
        } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Talks,
                floor_budget(),
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        assert_eq!(effective_level, ContextLevel::Raw);
        assert_eq!(fallback_messages, raw_messages);
        assert_eq!(fallback_truncation, raw_truncation);
    }

    #[test]
    fn context_raw_never_falls_back_and_carries_no_hint() {
        let fixture = ctx_fixture();
        let AppResponse::Context {
            requested_level,
            effective_level,
            talks,
            summary,
            hint,
            ..
        } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Raw,
                ResponseBudget::default(),
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        assert_eq!(requested_level, ContextLevel::Raw);
        assert_eq!(effective_level, ContextLevel::Raw);
        assert!(talks.is_empty());
        assert!(summary.is_none());
        assert!(hint.is_none());
    }

    #[test]
    fn context_derived_level_bytes_count_toward_budget() {
        let mut fixture = ctx_fixture();
        let big_text = "x".repeat(900);
        for (id, role, timestamp) in [
            (&fixture.root, "user", "2026-07-26T00:00:00Z"),
            (&fixture.repeated, "assistant", "2026-07-26T00:01:00Z"),
            (&fixture.leaf, "user", "2026-07-26T00:02:00Z"),
        ] {
            fixture
                .store
                .catalog
                .insert(id, ctx_msg_payload(role, &big_text, timestamp));
        }
        let floor_budget = || ResponseBudget {
            max_response_bytes: budget::MIN_RESPONSE_BYTES,
            ..Default::default()
        };
        let AppResponse::Context {
            messages: raw_messages,
            ..
        } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Raw,
                floor_budget(),
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        let AppResponse::Context {
            messages: derived_messages,
            effective_level,
            truncation,
            ..
        } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Talks,
                floor_budget(),
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        assert_eq!(effective_level, ContextLevel::Talks);
        assert!(truncation.truncated);
        assert!(
            truncation
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains(budget::TRUNCATION_MAX_RESPONSE_BYTES))
        );
        assert!(derived_messages.len() < raw_messages.len());
    }

    #[test]
    fn context_derived_views_honor_message_clamp() {
        let fixture = ctx_fixture();
        let AppResponse::Context {
            effective_level,
            talks,
            summary,
            truncation,
            ..
        } = app_ctx(&fixture.store)
            .handle(ctx_req_level(
                &fixture.session,
                ContextPolicy::Mainline,
                ContextLevel::Sessions,
                ResponseBudget {
                    max_messages: 2,
                    ..Default::default()
                },
            ))
            .unwrap()
        else {
            panic!("expected Context response");
        };
        assert!(truncation.truncated);
        assert_eq!(
            truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_MESSAGES)
        );
        assert_eq!(effective_level, ContextLevel::Sessions);
        assert!(talks.is_empty());
        let summary = summary.expect("clamped sessions still summarizes");
        assert_eq!(summary.message_count, 2);
        assert_eq!(summary.turn_count, 1);
    }

    fn message_req_with_id(
        message_id: StableId,
        session_id: Option<StableId>,
        around: usize,
        budget: ResponseBudget,
    ) -> AppRequest {
        AppRequest::Message {
            message_id,
            session_id,
            around,
            budget,
        }
    }

    fn message_req(
        fixture: &ContextFixture,
        session_id: Option<StableId>,
        around: usize,
        budget: ResponseBudget,
    ) -> AppRequest {
        message_req_with_id(fixture.repeated.clone(), session_id, around, budget)
    }

    #[test]
    fn message_around_zero_returns_anchor_only() {
        let fixture = ctx_fixture();
        let AppResponse::Message { window } = app_ctx(&fixture.store)
            .handle(message_req(
                &fixture,
                Some(fixture.session.clone()),
                0,
                ResponseBudget::default(),
            ))
            .unwrap()
        else {
            panic!("expected Message response");
        };
        assert_eq!(window.message_id, fixture.repeated.as_str());
        assert_eq!(window.session_id, fixture.session.as_str());
        assert_eq!(window.anchor_placement_id, fixture.repeated_b.id.as_str());
        assert_eq!(window.messages.len(), 1);
        assert_eq!(
            window.messages[0].placement_id,
            fixture.repeated_b.id.as_str()
        );
        assert!(!window.truncation.truncated);
    }

    #[test]
    fn message_window_clamps_at_mainline_ends_and_stays_chronological() {
        let fixture = ctx_fixture();
        let AppResponse::Message { window } = app_ctx(&fixture.store)
            .handle(message_req(
                &fixture,
                Some(fixture.session.clone()),
                9,
                ResponseBudget::default(),
            ))
            .unwrap()
        else {
            panic!("expected Message response");
        };
        assert_eq!(
            window
                .messages
                .iter()
                .map(|message| message.message_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                fixture.root.as_str(),
                fixture.repeated.as_str(),
                fixture.leaf.as_str(),
            ]
        );
        assert!(
            !window
                .messages
                .iter()
                .any(|message| message.message_id == fixture.sidechain.as_str())
        );
    }

    #[test]
    fn message_window_includes_one_neighbor_per_side() {
        let fixture = ctx_fixture();
        let AppResponse::Message { window } = app_ctx(&fixture.store)
            .handle(message_req(
                &fixture,
                Some(fixture.session.clone()),
                1,
                ResponseBudget::default(),
            ))
            .unwrap()
        else {
            panic!("expected Message response");
        };
        assert_eq!(window.messages.len(), 3);
        assert_eq!(window.messages[0].message_id, fixture.root.as_str());
        assert_eq!(window.messages[2].message_id, fixture.leaf.as_str());
    }

    #[test]
    fn message_multiple_mainline_placements_is_invalid_request() {
        let mut fixture = ctx_fixture();
        fixture.store.graph.placements[2].is_sidechain = false;
        fixture.store.graph.edges[1].parent_message_id = fixture.sidechain.clone();
        fixture.store.graph.validate().unwrap();

        let err = app_ctx(&fixture.store)
            .handle(message_req(
                &fixture,
                Some(fixture.session.clone()),
                0,
                ResponseBudget::default(),
            ))
            .unwrap_err();
        let AppError::Domain(error @ DomainError::InvalidRequest(_)) = err else {
            panic!("expected InvalidRequest, got {err:?}");
        };
        assert_eq!(error.code(), "invalid_request");
        let DomainError::InvalidRequest(message) = error else {
            unreachable!();
        };
        assert!(message.contains("multiple placements"), "{message}");
        assert!(message.contains("get_session_context"), "{message}");
    }

    #[test]
    fn message_without_session_auto_resolves_single_candidate() {
        let fixture = ctx_fixture();
        let AppResponse::Message { window } = app_ctx(&fixture.store)
            .handle(message_req(&fixture, None, 0, ResponseBudget::default()))
            .unwrap()
        else {
            panic!("expected Message response");
        };
        assert_eq!(window.session_id, fixture.session.as_str());
        assert_eq!(window.messages.len(), 1);
    }

    #[test]
    fn message_with_unknown_session_is_not_found() {
        let fixture = ctx_fixture();
        let wrong = StableId::native(IdKind::Session, "sess-not-a-candidate");
        let err = app_ctx(&fixture.store)
            .handle(message_req(
                &fixture,
                Some(wrong),
                0,
                ResponseBudget::default(),
            ))
            .unwrap_err();
        assert!(matches!(err, AppError::Domain(DomainError::NotFound(_))));
    }

    #[test]
    fn message_ambiguity_is_bounded_sorted_and_actionable() {
        let mut fixture = ctx_fixture();
        fixture.store.candidates = (0..10)
            .map(|index| PortMessageContextCandidate {
                session_id: StableId::native(IdKind::Session, &format!("sess-x{index}")),
                placement_ids: vec![fixture.repeated_a.id.clone()],
            })
            .collect();
        let AppError::MessageAmbiguous(ambiguity) = app_ctx(&fixture.store)
            .handle(message_req(&fixture, None, 0, ResponseBudget::default()))
            .unwrap_err()
        else {
            panic!("expected MessageAmbiguous error");
        };
        assert_eq!(ambiguity.candidate_count, 10);
        assert_eq!(
            ambiguity.candidate_session_ids.len(),
            MAX_MESSAGE_AMBIGUITY_CANDIDATES
        );
        let mut sorted = ambiguity.candidate_session_ids.clone();
        sorted.sort();
        assert_eq!(ambiguity.candidate_session_ids, sorted);
        assert!(!ambiguity.hint.is_empty());
    }

    #[test]
    fn message_missing_is_not_found() {
        let fixture = ctx_fixture();
        let err = app_ctx(&fixture.store)
            .handle(message_req_with_id(
                StableId::native(IdKind::Message, "index-only"),
                None,
                0,
                ResponseBudget::default(),
            ))
            .unwrap_err();
        assert!(matches!(err, AppError::Port(PortError::NotFound(_))));
    }

    #[test]
    fn message_off_mainline_is_not_found() {
        let mut fixture = ctx_fixture();
        fixture.store.context_message_id = fixture.sidechain.clone();
        fixture.store.candidates = vec![PortMessageContextCandidate {
            session_id: fixture.session.clone(),
            placement_ids: vec![fixture.store.graph.placements[2].id.clone()],
        }];
        let err = app_ctx(&fixture.store)
            .handle(message_req_with_id(
                fixture.sidechain.clone(),
                Some(fixture.session.clone()),
                0,
                ResponseBudget::default(),
            ))
            .unwrap_err();
        assert!(matches!(err, AppError::Domain(DomainError::NotFound(_))));
    }

    #[test]
    fn message_budget_rejects_below_floor() {
        let fixture = ctx_fixture();
        let err = app_ctx(&fixture.store)
            .handle(message_req(
                &fixture,
                Some(fixture.session.clone()),
                0,
                ResponseBudget {
                    max_response_bytes: budget::MIN_RESPONSE_BYTES - 1,
                    ..Default::default()
                },
            ))
            .unwrap_err();
        assert!(matches!(err, AppError::Budget(_)));
    }

    #[test]
    fn message_item_budget_keeps_anchor_and_reports_max_items() {
        let fixture = ctx_fixture();
        let AppResponse::Message { window } = app_ctx(&fixture.store)
            .handle(message_req(
                &fixture,
                Some(fixture.session.clone()),
                9,
                ResponseBudget {
                    max_items: 1,
                    ..Default::default()
                },
            ))
            .unwrap()
        else {
            panic!("expected Message response");
        };
        assert_eq!(window.messages.len(), 1);
        assert_eq!(
            window.messages[0].placement_id,
            fixture.repeated_b.id.as_str()
        );
        assert_eq!(
            window.truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_ITEMS)
        );
    }

    #[test]
    fn message_byte_budget_keeps_anchor_and_reports_max_response_bytes() {
        let mut fixture = ctx_fixture();
        fixture.store.context_message_id = fixture.root.clone();
        fixture.store.candidates = vec![PortMessageContextCandidate {
            session_id: fixture.session.clone(),
            placement_ids: vec![fixture.root_placement.id.clone()],
        }];
        let oversized_text = "\"\n".repeat(3072);
        fixture.store.catalog.insert(
            &fixture.root,
            serde_json::json!({
                "role": "user",
                "text": oversized_text.clone(),
                "metadata": "m".repeat(4096),
            })
            .to_string()
            .into_bytes(),
        );
        let requested_budget = budget::MIN_RESPONSE_BYTES;
        let AppResponse::Message { window } = app_ctx(&fixture.store)
            .handle(message_req_with_id(
                fixture.root.clone(),
                Some(fixture.session.clone()),
                9,
                ResponseBudget {
                    max_response_bytes: requested_budget,
                    ..Default::default()
                },
            ))
            .unwrap()
        else {
            panic!("expected Message response");
        };
        assert_eq!(window.message_id, fixture.root.as_str());
        assert_eq!(window.session_id, fixture.session.as_str());
        assert_eq!(
            window.anchor_placement_id,
            fixture.root_placement.id.as_str()
        );
        assert_eq!(window.messages.len(), 1);
        let anchor = &window.messages[0];
        assert_eq!(anchor.id, fixture.root.as_str());
        assert_eq!(anchor.message_id, fixture.root.as_str());
        assert_eq!(anchor.placement_id, fixture.root_placement.id.as_str());
        assert!(anchor.payload.get("metadata").is_none());
        assert_eq!(anchor.payload["role"], "user");
        assert!(
            anchor.payload["text"].as_str().unwrap().len() < oversized_text.len(),
            "oversized anchor text must be projected"
        );
        assert_eq!(
            window.truncation.reason.as_deref(),
            Some(budget::TRUNCATION_MAX_RESPONSE_BYTES)
        );
        let render_equivalent_bytes = ENVELOPE_RESERVE_BYTES
            + window
                .messages
                .iter()
                .map(message_occurrence_bytes)
                .sum::<usize>();
        assert!(
            render_equivalent_bytes <= requested_budget,
            "projected response estimate {render_equivalent_bytes} exceeds {requested_budget}"
        );
    }

    // ---- stage（RFC-0002 §5）----

    #[test]
    fn stage_returns_all_messages_on_success() {
        let provider = FakeProvider::good("demo", &[("user", "hi"), ("assistant", "yo")]);
        let staged = stage(&provider, b"anything").unwrap();
        assert_eq!(staged.messages.len(), 2);
        assert_eq!(staged.messages[0].seq, 0);
        assert_eq!(staged.messages[0].role, "user");
        assert_eq!(staged.messages[0].text, "hi");
        assert_eq!(staged.messages[1].seq, 1);
        assert_eq!(staged.messages[1].role, "assistant");
        assert_eq!(staged.messages[1].text, "yo");
        assert_eq!(staged.report.committed, 2);
        assert_eq!(staged.report.skipped, 0);
        assert!(staged.report.diagnostics.is_empty());
    }

    #[test]
    fn stage_preserves_spans_and_complete_parse_report() {
        // 内联 provider：emit 带 span 的消息并报告会话 native id，
        // 验证 staging 对两者的透传（不落在 FakeProvider 上，保持 testkit 最小）。
        struct SpanProvider;
        impl ProviderAdapter for SpanProvider {
            fn provider_id(&self) -> &str {
                "span-demo"
            }
            fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
                agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
            }
            fn probe(
                &self,
                _bytes: &[u8],
            ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
                Ok(agent_session_grep_ports::ProbeResult {
                    variant_id: "span-demo/fake-v1".into(),
                    confidence: Confidence::Confirmed,
                    matched_evidence: vec!["inline".into()],
                    unmatched_evidence: Vec::new(),
                })
            }
            fn parse(
                &self,
                _bytes: &[u8],
                sink: &mut dyn CanonicalEventSink,
            ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
                sink.emit_message(MessageEvent {
                    seq: 0,
                    native_id: "m-1",
                    parent_native_id: None,
                    role: "user",
                    text: "hello",
                    timestamp: None,
                    is_sidechain: false,
                    span: Some((0, 42)),
                })
                .map_err(|e| ProviderError::Io(e.to_string()))?;
                sink.emit_message(MessageEvent {
                    seq: 1,
                    native_id: "m-2",
                    parent_native_id: None,
                    role: "assistant",
                    text: "world",
                    timestamp: None,
                    is_sidechain: false,
                    span: None,
                })
                .map_err(|e| ProviderError::Io(e.to_string()))?;
                Ok(agent_session_grep_ports::ParseReport {
                    committed: 2,
                    skipped: 3,
                    diagnostics: vec!["record 3 skipped".into(), "unknown field seen".into()],
                    session_native_id: Some("native-sess-1".into()),
                    session_observation: Default::default(),
                })
            }
        }
        let staged = stage(&SpanProvider, b"x").unwrap();
        // span 逐条透传；None 保持显式缺失。
        assert_eq!(staged.messages[0].span, Some((0, 42)));
        assert_eq!(staged.messages[1].span, None);
        assert_eq!(staged.report.committed, 2);
        assert_eq!(staged.report.skipped, 3);
        assert_eq!(
            staged.report.diagnostics,
            vec!["record 3 skipped", "unknown field seen"]
        );
        assert_eq!(
            staged.report.session_native_id.as_deref(),
            Some("native-sess-1")
        );
        // Compatibility alias remains synchronized with the authoritative report.
        assert_eq!(staged.session_native_id.as_deref(), Some("native-sess-1"));
    }

    #[test]
    fn stage_rejects_ambiguous_variant() {
        let provider = FakeProvider::ambiguous("demo");
        let err = stage(&provider, b"x").unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::InvalidRequest(_))
        ));
    }

    #[test]
    fn stage_ambiguous_error_does_not_leak_evidence() {
        // ambiguous 错误消息只报 variant 标识，不携带 unmatched_evidence——
        // 证据可能含路径/原文片段。
        struct LeakyAmbiguous;
        impl ProviderAdapter for LeakyAmbiguous {
            fn provider_id(&self) -> &str {
                "leaky"
            }
            fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
                agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
            }
            fn probe(
                &self,
                _bytes: &[u8],
            ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
                Ok(agent_session_grep_ports::ProbeResult {
                    variant_id: "leaky/unknown".into(),
                    confidence: Confidence::Ambiguous,
                    matched_evidence: Vec::new(),
                    unmatched_evidence: vec!["C:\\Users\\secret\\transcript.jsonl".into()],
                })
            }
            fn parse(
                &self,
                _bytes: &[u8],
                _sink: &mut dyn CanonicalEventSink,
            ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
                Ok(agent_session_grep_ports::ParseReport::default())
            }
        }
        let err = stage(&LeakyAmbiguous, b"x").unwrap_err();
        let message = err.to_string();
        assert!(
            !message.contains("secret"),
            "错误消息不得泄漏证据内容: {message}"
        );
        assert!(
            message.contains("leaky/unknown"),
            "仍应报告 variant 标识: {message}"
        );
    }

    #[test]
    fn select_and_stage_probes_each_adapter_exactly_once() {
        // 选中 adapter 后复用其 probe 结果——同一字节不得二次 probe。
        struct ProbeCounting<'a> {
            inner: &'a FakeProvider,
            probes: &'a std::sync::atomic::AtomicUsize,
        }
        impl ProviderAdapter for ProbeCounting<'_> {
            fn provider_id(&self) -> &str {
                self.inner.provider_id()
            }
            fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
                self.inner.manifest()
            }
            fn probe(
                &self,
                bytes: &[u8],
            ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
                self.probes
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.inner.probe(bytes)
            }
            fn parse(
                &self,
                bytes: &[u8],
                sink: &mut dyn CanonicalEventSink,
            ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
                self.inner.parse(bytes, sink)
            }
        }

        let inner = FakeProvider::good("demo", &[("user", "hi"), ("assistant", "yo")]);
        let probes = std::sync::atomic::AtomicUsize::new(0);
        let wrapper = ProbeCounting {
            inner: &inner,
            probes: &probes,
        };
        let refs: Vec<&dyn ProviderAdapter> = vec![&wrapper];

        let staged = select_and_stage(&refs, b"x").unwrap();
        assert_eq!(
            probes.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "选中后不得对同一字节二次 probe"
        );
        assert_eq!(staged.messages.len(), 2);
    }

    #[test]
    fn select_and_stage_surfaces_probe_error_line_detail_when_all_rejected() {
        // PRD R2.2：全部 adapter 拒绝时，最后一个 probe 错误自带的行号定位与
        // 修复方向必须透出——绝不裸报 "no provider recognized this source"。
        // （probe 错误只由源字节内容派生，provider 看不到路径，消息即诊断。）
        struct LineDetailRejecter;
        impl ProviderAdapter for LineDetailRejecter {
            fn provider_id(&self) -> &str {
                "line-detail"
            }
            fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
                agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
            }
            fn probe(
                &self,
                _bytes: &[u8],
            ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
                Err(ProviderError::AmbiguousVariant(
                    "not line-delimited JSON: 0/3 sampled lines parsed。第 2 行不是有效 JSON。请修复或删除这些行后重试"
                        .into(),
                ))
            }
            fn parse(
                &self,
                _bytes: &[u8],
                _sink: &mut dyn CanonicalEventSink,
            ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
                Ok(agent_session_grep_ports::ParseReport::default())
            }
        }
        struct SilentRejecter;
        impl ProviderAdapter for SilentRejecter {
            fn provider_id(&self) -> &str {
                "silent"
            }
            fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
                agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
            }
            fn probe(
                &self,
                _bytes: &[u8],
            ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
                Ok(agent_session_grep_ports::ProbeResult {
                    variant_id: "silent/unknown".into(),
                    confidence: Confidence::Ambiguous,
                    matched_evidence: Vec::new(),
                    unmatched_evidence: Vec::new(),
                })
            }
            fn parse(
                &self,
                _bytes: &[u8],
                _sink: &mut dyn CanonicalEventSink,
            ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
                Ok(agent_session_grep_ports::ParseReport::default())
            }
        }
        let refs: Vec<&dyn ProviderAdapter> = vec![&SilentRejecter, &LineDetailRejecter];
        let err = select_and_stage(&refs, b"x").unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("no provider recognized this source"),
            "分类前缀必须保留: {message}"
        );
        assert!(message.contains("第 2 行"), "必须携带行号定位: {message}");
        assert!(
            message.contains("请修复或删除这些行后重试"),
            "必须携带修复方向: {message}"
        );
    }

    #[test]
    fn select_and_stage_all_ambiguous_keeps_bare_message() {
        // 全部 adapter 只是 ambiguous（非报错）时，保持原有裸消息，不出现
        // "last probe failure" 后缀（没有可复用的错误细节）。
        struct AmbiguousOnly;
        impl ProviderAdapter for AmbiguousOnly {
            fn provider_id(&self) -> &str {
                "ambiguous-only"
            }
            fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
                agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
            }
            fn probe(
                &self,
                _bytes: &[u8],
            ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
                Ok(agent_session_grep_ports::ProbeResult {
                    variant_id: "ambiguous-only/unknown".into(),
                    confidence: Confidence::Ambiguous,
                    matched_evidence: Vec::new(),
                    unmatched_evidence: Vec::new(),
                })
            }
            fn parse(
                &self,
                _bytes: &[u8],
                _sink: &mut dyn CanonicalEventSink,
            ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
                Ok(agent_session_grep_ports::ParseReport::default())
            }
        }
        let refs: Vec<&dyn ProviderAdapter> = vec![&AmbiguousOnly];
        let err = select_and_stage(&refs, b"x").unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::InvalidRequest(_))
        ));
        let message = err.to_string();
        // 没有任何 probe 报错 → 不得凭空拼接 probe 细节（旧断言用整串相等表达
        // 这一点；现在分类前缀之后恒有可行动的下一步，故改为断言"分类前缀 +
        // 无 probe 细节"这两件事本身，约束未放松）。
        assert!(
            message.starts_with("invalid request: no provider recognized this source."),
            "分类前缀后不得出现 probe 细节: {message}"
        );
        assert!(
            !message.contains("ambiguous or unknown variant"),
            "不得外泄 probe 内部分级词汇: {message}"
        );
        // D11 可行动性：消息必须给出下一步，而不是只说"没人认领"。
        assert!(
            message.contains("agent-session-grep providers"),
            "必须给出可行动的下一步: {message}"
        );
    }

    /// 内容同形的两个 provider（如 pi 与 openclaw）：probe 必然同分。
    struct SameShape {
        id: &'static str,
    }
    impl ProviderAdapter for SameShape {
        fn provider_id(&self) -> &str {
            self.id
        }
        fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
            agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
        }
        fn probe(
            &self,
            _bytes: &[u8],
        ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
            Ok(agent_session_grep_ports::ProbeResult {
                variant_id: format!("{}/session-jsonl-v1", self.id),
                confidence: Confidence::High,
                matched_evidence: Vec::new(),
                unmatched_evidence: Vec::new(),
            })
        }
        fn parse(
            &self,
            _bytes: &[u8],
            sink: &mut dyn CanonicalEventSink,
        ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
            sink.emit_message(agent_session_grep_ports::MessageEvent {
                seq: 0,
                native_id: self.id,
                parent_native_id: None,
                role: "user",
                text: "hi",
                timestamp: None,
                is_sidechain: false,
                span: None,
            })
            .map_err(|e| ProviderError::Io(e.to_string()))?;
            Ok(agent_session_grep_ports::ParseReport {
                committed: 1,
                ..Default::default()
            })
        }
        // record-stream 家族要求显式实现；本 fake 不看字节，直接委派。
        fn parse_source(
            &self,
            _source: &dyn agent_session_grep_ports::ReadOnlySource,
            sink: &mut dyn CanonicalEventSink,
        ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
            self.parse(&[], sink)
        }
    }

    #[test]
    fn discovery_root_breaks_a_tie_between_indistinguishable_providers() {
        // M2P-16：pi 与 openclaw 的磁盘格式确实同形（openclaw adapter 自己的文档
        // 就这么写），任何内容 probe 都区分不了。此时 discover 记下的 root 归属是
        // 唯一可用证据——它独立于文件内容，不是"猜一个"。
        let pi = SameShape { id: "pi" };
        let openclaw = SameShape { id: "openclaw" };
        let refs: Vec<&dyn ProviderAdapter> = vec![&pi, &openclaw];
        let source = agent_session_grep_ports::SliceSource::new(b"{}");

        let (_, variant) =
            select_and_stage_source(&refs, &source, Some("openclaw")).expect("root 归属必须能消歧");
        assert_eq!(variant, "openclaw/session-jsonl-v1");

        // 反向同样成立：胜者由 root 决定，不受注册顺序影响。
        let (_, variant) =
            select_and_stage_source(&refs, &source, Some("pi")).expect("root 归属必须能消歧");
        assert_eq!(variant, "pi/session-jsonl-v1");
    }

    #[test]
    fn tie_without_a_root_hint_is_still_refused() {
        // 手动 `sync <file>` 没有 root 上下文。消歧证据缺席时必须回到拒绝，
        // 绝不因为"多数情况下是 X"而挑一个（RFC-0002 宁拒不猜）。
        let pi = SameShape { id: "pi" };
        let openclaw = SameShape { id: "openclaw" };
        let refs: Vec<&dyn ProviderAdapter> = vec![&pi, &openclaw];
        let source = agent_session_grep_ports::SliceSource::new(b"{}");

        let err = select_and_stage_source(&refs, &source, None).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("pi/session-jsonl-v1")
                && message.contains("openclaw/session-jsonl-v1"),
            "必须列出全部并列 variant: {message}"
        );
    }

    #[test]
    fn a_provider_hint_outside_the_tie_refuses_and_says_so() {
        // root 指向的 provider 不在同分候选里（用户把 transcript 放错目录，或
        // `--provider` 点错名）：仍然拒绝，且必须说明 hint 为何没起作用——
        // 否则用户会以为消歧失效了。
        let pi = SameShape { id: "pi" };
        let openclaw = SameShape { id: "openclaw" };
        let refs: Vec<&dyn ProviderAdapter> = vec![&pi, &openclaw];
        let source = agent_session_grep_ports::SliceSource::new(b"{}");

        let err = select_and_stage_source(&refs, &source, Some("claude-code")).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("claude-code") && message.contains("not among the tied candidates"),
            "必须说明 provider 提示为何未生效: {message}"
        );
    }

    #[test]
    fn text_source_rejection_never_blames_a_sqlite_adapter() {
        // M2P-5：adapter 按注册顺序 probe，注册表以 SQLite 家族（opencode/cursor）
        // 收尾，于是任何无人认领的文本文件都被解释成 "not a SQLite database
        // (missing magic header)"——点错格式，且是 probe 内部细节。现在只有与本源
        // 同家族的诊断可以外泄。
        struct SqliteFamilyRejecter;
        impl ProviderAdapter for SqliteFamilyRejecter {
            fn provider_id(&self) -> &str {
                // 家族由 provider id 中心化派生；cursor 属 SQLite 家族。
                "cursor"
            }
            fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
                agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
            }
            fn probe(
                &self,
                _bytes: &[u8],
            ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
                Err(ProviderError::AmbiguousVariant(
                    "not a SQLite database (missing magic header)".into(),
                ))
            }
            fn parse(
                &self,
                _bytes: &[u8],
                _sink: &mut dyn CanonicalEventSink,
            ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
                Ok(agent_session_grep_ports::ParseReport::default())
            }
        }
        struct TextFamilyRejecter;
        impl ProviderAdapter for TextFamilyRejecter {
            fn provider_id(&self) -> &str {
                "claude-code"
            }
            fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
                agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
            }
            fn probe(
                &self,
                _bytes: &[u8],
            ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
                Err(ProviderError::AmbiguousVariant(
                    "第 1 行不是有效 JSON。请修复或删除这些行后重试".into(),
                ))
            }
            fn parse(
                &self,
                _bytes: &[u8],
                _sink: &mut dyn CanonicalEventSink,
            ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
                Ok(agent_session_grep_ports::ParseReport::default())
            }
        }
        // 文本源 + SQLite adapter 排在最后（复现注册表顺序）。
        let refs: Vec<&dyn ProviderAdapter> = vec![&TextFamilyRejecter, &SqliteFamilyRejecter];
        let message = select_and_stage(&refs, b"this is not json at all")
            .unwrap_err()
            .to_string();
        assert!(
            !message.contains("SQLite"),
            "文本源的拒绝理由不得点名 SQLite: {message}"
        );
        assert!(
            message.contains("第 1 行"),
            "同家族 adapter 的行号诊断必须保留: {message}"
        );

        // 反向：真的是 SQLite 头的源，则该由 SQLite 家族解释，文本家族的行号
        // 诊断与它无关。
        let mut sqlite_head = agent_session_grep_ports::SQLITE_MAGIC_HEADER.to_vec();
        sqlite_head.extend_from_slice(b"trailing bytes");
        let message = select_and_stage(&refs, &sqlite_head)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("not a SQLite database"),
            "SQLite 源应由 SQLite 家族解释: {message}"
        );
        assert!(
            !message.contains("第 1 行"),
            "异家族的行号诊断不得外泄: {message}"
        );
    }

    // ---- source-scoped staging（RFC-0002 §7 bounded ingest）----

    /// 用 BoundedWholeSource provider id（cline）的 fake：默认 probe_source /
    /// parse_source 走「上限内整读 + 委托字节 parse」路径，无需覆盖新方法。
    struct SourceFake;
    impl ProviderAdapter for SourceFake {
        fn provider_id(&self) -> &str {
            "cline"
        }
        fn manifest(&self) -> agent_session_grep_ports::AdapterManifest {
            agent_session_grep_ports::manifest_for(self.provider_id(), None, &[])
        }
        fn probe(
            &self,
            _bytes: &[u8],
        ) -> Result<agent_session_grep_ports::ProbeResult, ProviderError> {
            Ok(agent_session_grep_ports::ProbeResult {
                variant_id: "cline/fake-v1".into(),
                confidence: Confidence::Confirmed,
                matched_evidence: vec!["fake probe".into()],
                unmatched_evidence: Vec::new(),
            })
        }
        fn parse(
            &self,
            _bytes: &[u8],
            sink: &mut dyn CanonicalEventSink,
        ) -> Result<agent_session_grep_ports::ParseReport, ProviderError> {
            sink.emit_message(MessageEvent {
                seq: 0,
                native_id: "m-1",
                parent_native_id: None,
                role: "user",
                text: "hi",
                timestamp: None,
                is_sidechain: false,
                span: Some((0, 2)),
            })
            .map_err(|e| ProviderError::Io(e.to_string()))?;
            Ok(agent_session_grep_ports::ParseReport {
                committed: 1,
                ..Default::default()
            })
        }
    }

    #[test]
    fn select_and_stage_source_streams_selected_variant() {
        // 生产路径：probe/parse 都从只读 source 重新打开，返回 (staged, variant)。
        let source = agent_session_grep_ports::SliceSource::new(b"{}");
        let refs: Vec<&dyn ProviderAdapter> = vec![&SourceFake];
        let (staged, variant) = select_and_stage_source(&refs, &source, None).unwrap();
        assert_eq!(variant, "cline/fake-v1");
        assert_eq!(staged.messages.len(), 1);
        assert_eq!(staged.messages[0].text, "hi");
        assert_eq!(staged.report.committed, 1);
    }

    #[test]
    fn select_and_stage_source_rejects_source_beyond_declared_cap() {
        // 大文件上限回归：超过 manifest max_source_size 的源必须诚实拒绝
        // （SourceTooLarge 诊断），而不是按文件大小分配内存或静默截断。
        let big = vec![b'x'; (agent_session_grep_ports::JSON_FAMILY_MAX_SOURCE_BYTES as usize) + 1];
        let source = agent_session_grep_ports::SliceSource::new(&big);
        let refs: Vec<&dyn ProviderAdapter> = vec![&SourceFake];
        let err = select_and_stage_source(&refs, &source, None).unwrap_err();
        // probe 失败按既有选择语义跳过该 adapter，但最后 probe 错误细节（含
        // 受测上限）必须透出，绝不 OOM、绝不静默截断。
        assert!(matches!(
            err,
            AppError::Domain(DomainError::InvalidRequest(_))
        ));
        assert!(
            err.to_string().contains("exceeds supported limit"),
            "必须携带受测上限诊断: {err}"
        );
    }

    #[test]
    fn stage_discards_partial_on_mid_parse_fatal() {
        // 关键：provider 先 emit 2 条再 StructuralFatal。
        // stage 必须返回 Err，且不产出任何部分结果（原子丢弃）。
        let provider =
            FakeProvider::failing_after("demo", &[("user", "a"), ("assistant", "b")], "boom");
        let err = stage(&provider, b"x").unwrap_err();
        assert!(matches!(
            &err,
            AppError::Provider(ProviderError::StructuralFatal(_))
        ));
        assert!(err.to_string().contains("structural fatal"));
    }

    // ---- ADR-0009 resume metadata / cursor result_set ----

    struct FakeResumeClaims;
    impl ResumeClaimsStore for FakeResumeClaims {
        fn resume_of(&self, session_ids: &[StableId]) -> PortResult<Vec<SessionResumeMetadata>> {
            Ok(session_ids
                .iter()
                .map(|id| SessionResumeMetadata {
                    session_id: id.clone(),
                    provider_id: Some("claude-code".into()),
                    resume_available: true,
                    provider_session_id: Some("prov-sess-1".into()),
                    original_working_directory: Some("C:/work".into()),
                    unavailable_reason: None,
                })
                .collect())
        }
    }

    #[test]
    fn get_session_resume_returns_fixed_nullable_metadata() {
        let session = StableId::native(IdKind::Session, "sess-resume");
        let app = App::with_resume_and_clock(FakeCatalog, FakeIndex, FakeResumeClaims, clock_t0);
        let AppResponse::SessionResume(metadata) = app
            .handle(AppRequest::GetSessionResume {
                session_id: session.clone(),
            })
            .unwrap()
        else {
            panic!("expected SessionResume response");
        };
        assert_eq!(metadata.session_id, session);
        assert_eq!(metadata.provider_id.as_deref(), Some("claude-code"));
        assert!(metadata.resume_available);
        assert_eq!(metadata.provider_session_id.as_deref(), Some("prov-sess-1"));
        assert_eq!(
            metadata.original_working_directory.as_deref(),
            Some("C:/work")
        );
        assert!(metadata.unavailable_reason.is_none());
    }

    #[test]
    fn get_session_resume_rejects_non_session_kind() {
        let message = StableId::native(IdKind::Message, "msg-not-session");
        let err = app()
            .handle(AppRequest::GetSessionResume {
                session_id: message,
            })
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::InvalidRequest(_))
        ));
    }

    #[test]
    fn get_session_resume_without_claims_reports_unavailable() {
        // NoResumeClaims 兜底：resume_available=false + 明确 unavailable_reason。
        let session = StableId::native(IdKind::Session, "sess-no-claims");
        let AppResponse::SessionResume(metadata) = app()
            .handle(AppRequest::GetSessionResume {
                session_id: session.clone(),
            })
            .unwrap()
        else {
            panic!("expected SessionResume response");
        };
        assert_eq!(metadata.session_id, session);
        assert!(!metadata.resume_available);
        assert!(metadata.unavailable_reason.is_some());
    }

    #[test]
    fn search_hits_carry_resume_availability_from_claims() {
        // 一次批量 resume_of 装配页内命中：有声明 → true；无声明/无归属 → false。
        let mut cat = MapCatalog::new(7);
        let session_a = StableId::native(IdKind::Session, "sess-a");
        let session_b = StableId::native(IdKind::Session, "sess-b");
        for (tag, session) in [
            ("hit00", Some(&session_a)),
            ("hit01", Some(&session_b)),
            ("hit02", None),
        ] {
            let id = hit_id(tag);
            cat.insert(
                &id,
                serde_json::json!({ "role": "user", "text": "needle" })
                    .to_string()
                    .into_bytes(),
            );
            if let Some(session) = session {
                cat.set_session_of(&id, session);
            }
        }
        struct OnlySessionA;
        impl ResumeClaimsStore for OnlySessionA {
            fn resume_of(
                &self,
                session_ids: &[StableId],
            ) -> PortResult<Vec<SessionResumeMetadata>> {
                Ok(session_ids
                    .iter()
                    .map(|id| SessionResumeMetadata {
                        session_id: id.clone(),
                        provider_id: None,
                        resume_available: id.as_str().ends_with("sess-a"),
                        provider_session_id: None,
                        original_working_directory: None,
                        unavailable_reason: (!id.as_str().ends_with("sess-a"))
                            .then(|| "no claims".into()),
                    })
                    .collect())
            }
        }
        let index = FixedHits(vec![hit_id("hit00"), hit_id("hit01"), hit_id("hit02")]);
        let app = App::with_resume_and_clock(cat, index, OnlySessionA, clock_t0);
        let AppResponse::Search { hits, .. } = app.handle(search_req("needle", 10, None)).unwrap()
        else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 3);
        assert!(hits[0].resume_available, "claimed session must be true");
        assert!(!hits[1].resume_available, "unclaimed session must be false");
        assert!(!hits[2].resume_available, "no session_id must be false");
    }

    #[test]
    fn search_group_by_session_carries_resume_availability() {
        // 归并路径同样在 clamp 后装配：组代表命中携带其会话的声明。
        let mut cat = MapCatalog::new(7);
        let session_a = StableId::native(IdKind::Session, "sess-a");
        for tag in ["hit00", "hit01"] {
            let id = hit_id(tag);
            cat.insert(
                &id,
                serde_json::json!({ "role": "user", "text": "needle" })
                    .to_string()
                    .into_bytes(),
            );
            cat.set_session_of(&id, &session_a);
        }
        let index = FixedHits(vec![hit_id("hit00"), hit_id("hit01")]);
        let app = App::with_resume_and_clock(cat, index, FakeResumeClaims, clock_t0);
        let AppResponse::Search { hits, .. } = app
            .handle(AppRequest::Search {
                query: "needle".into(),
                filters: SearchFilters::default(),
                facets: SearchFacets::default(),
                limit: 10,
                cursor: None,
                budget: ResponseBudget::default(),
                include_system: false,
                group_by_session: true,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
                context_lines: None,
            })
            .unwrap()
        else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].occurrences, 2);
        assert!(hits[0].resume_available);
    }

    #[test]
    fn search_without_resume_claims_reports_false() {
        // NoResumeClaims 默认：resume_available 恒 false。
        let mut cat = MapCatalog::new(7);
        let session_a = StableId::native(IdKind::Session, "sess-a");
        let id = hit_id("hit00");
        cat.insert(
            &id,
            serde_json::json!({ "role": "user", "text": "needle" })
                .to_string()
                .into_bytes(),
        );
        cat.set_session_of(&id, &session_a);
        let app = App::with_clock(cat, FixedHits(vec![id]), clock_t0);
        let AppResponse::Search { hits, .. } = app.handle(search_req("needle", 10, None)).unwrap()
        else {
            panic!("expected Search response");
        };
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].resume_available);
    }

    #[test]
    fn list_cursor_rejects_result_set_mismatch() {
        // result_set 判别器：sessions_only 与全实体列表的续读令牌不得互换。
        let mut cat = MapCatalog::new(7);
        for tag in ["la", "lb", "lc"] {
            let id = StableId::derive(IdKind::Message, Stability::Reconstructed, &[tag.as_bytes()]);
            cat.insert(&id, b"x".to_vec());
        }
        for tag in ["ls-a", "ls-b", "ls-c"] {
            let id = StableId::native(IdKind::Session, tag);
            cat.insert(&id, b"x".to_vec());
        }
        let app = App::with_clock(&cat, FakeIndex, clock_t0);
        let list_req = |cursor: Option<String>, sessions_only: bool| AppRequest::List {
            limit: 2,
            cursor,
            budget: ResponseBudget::default(),
            sessions_only,
            sort: ListSort::WireIdAsc,
        };
        let AppResponse::List { next_cursor, .. } = app.handle(list_req(None, false)).unwrap()
        else {
            panic!("expected List response");
        };
        let all_token = next_cursor.expect("list must page");

        // 全实体令牌 + sessions_only=true → 拒绝。
        let err = app
            .handle(list_req(Some(all_token.clone()), true))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Cursor(cursor::CursorError::Invalid(_))),
            "{err}"
        );

        // 反向：sessions_only 令牌 + 全实体 → 拒绝。
        let AppResponse::List { next_cursor, .. } = app.handle(list_req(None, true)).unwrap()
        else {
            panic!("expected List response");
        };
        let sessions_token = next_cursor.expect("sessions list must page");
        let err = app
            .handle(list_req(Some(sessions_token), false))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Cursor(cursor::CursorError::Invalid(_))),
            "{err}"
        );

        // 同判别器续读正常。
        assert!(app.handle(list_req(Some(all_token), false)).is_ok());
    }
}
