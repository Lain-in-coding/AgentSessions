//! Application：跨前端（CLI / Robot-JSON / MCP / TUI）共享的用例与请求/结果 ADT。
//!
//! 本层不知道任何具体前端或后端——只依赖 domain 的类型与 ports 的抽象。
//! 所有前端把各自的输入归一为 [`AppRequest`]，把 [`AppResponse`] 渲染成各自的输出格式。
//! 分层依赖不变量：domain ← ports ← application ← adapters。

use agent_session_grep_domain::{
    ContextPolicy, DomainError, Message, MessagePlacement, SourceDocument, StableId, select_full,
    select_mainline,
};
use agent_session_grep_ports::{
    CanonicalEventSink, CatalogEntry, CatalogStore, Confidence, ContextGraphStore, MessageEvent,
    ParseReport, PortError, PortResult, ProbeResult, ProviderAdapter, ProviderError, SearchHit,
    SearchIndex,
};
use std::collections::{BTreeMap, BTreeSet};

pub mod budget;
pub mod cursor;
pub mod evidence;

pub use budget::{ResponseBudget, Truncation};
pub use evidence::EvidenceSpanDto;

/// 排序方案标识：catalog 列表的钉住排序（wire id 升序，见 sqlite `ORDER BY id ASC`）。
pub const SORT_WIRE_ID_ASC: &str = "wire_id_asc";
/// 排序方案标识：检索结果的钉住排序（bm25 降序 + id tiebreak，全序确定）。
pub const SORT_SCORE_DESC: &str = "score_desc";

/// clamp 前从 `max_response_bytes` 扣除的 envelope 预留（budget.rs 声明预留是调用方义务）。
/// 预算下限 4096 保证扣除后仍为正。
const ENVELOPE_RESERVE_BYTES: usize = 1024;

/// Upper bound on a single fetch window. Cursor offsets are tamper-evident
/// but not unforgeable; capping the window keeps a forged huge offset from
/// overflowing into a negative SQL LIMIT (SQLite treats -1 as "no limit").
const MAX_FETCH_WINDOW: u64 = 1 << 20;

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
}

/// 应用层请求 ADT：所有前端的统一入口。
///
/// 每个变体是一个用例。前端负责解析各自语法后构造本枚举，
/// 从而保证 CLI / Robot / MCP / TUI 行为一致（见 CONTRACT-cli-robot-mcp-draft）。
#[derive(Debug, Clone, PartialEq)]
pub enum AppRequest {
    /// 全文检索：按查询串返回命中列表（分页 + 预算）。
    Search {
        query: String,
        /// 最多返回条数；0 视为非法请求。与 `budget.max_items` 取较小者为页大小。
        limit: usize,
        /// 上一页发出的续读令牌；`None` 表示第一页。
        cursor: Option<String>,
        /// 响应预算（CONTRACT §3）；低于下限即校验错误。
        budget: ResponseBudget,
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
    },
    /// 会话上下文装配：按策略选取分支，返回消息链与证据区间（CONTRACT §1-2）。
    Context {
        session_id: StableId,
        policy: ContextPolicy,
        budget: ResponseBudget,
    },
    /// Resolve every distinct Session that contains placements for one Message.
    MessageContexts { message_id: StableId },
    /// 返回当前 Catalog 统计状态。
    Status,
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

/// One distinct Session candidate for a stable Message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MessageContextCandidate {
    pub session_id: String,
    pub placement_ids: Vec<String>,
}

/// 应用层结果 ADT：前端据此渲染，不再回到 domain/ports 类型。
#[derive(Debug, Clone, PartialEq)]
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
    },
    /// 单个实体的原始负载；`None` 表示未找到。
    Get { payload: Option<Vec<u8>> },
    /// 单个实体的规范化展示；`None` 表示未找到。
    Show { payload: Option<Vec<u8>> },
    /// 稳定排序后的 Catalog 条目（wire id 升序）。
    List {
        entries: Vec<CatalogEntry>,
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
        truncation: Truncation,
        generation: u64,
    },
    /// Distinct-session candidates for a stable Message.
    MessageContexts {
        message_id: String,
        candidates: Vec<MessageContextCandidate>,
    },
    /// 当前 Catalog 实体总数、关系统计与活动 generation。
    Status {
        catalog_count: u64,
        active_generation: u64,
        placements: u64,
        source_placement_claims: u64,
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

/// 一次 staging 的完整产物：缓冲消息 + provider 的完整解析报告。
///
/// 会话 native id 的唯一权威来源是 [`ParseReport::session_native_id`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedBatch {
    pub messages: Vec<StagedMessage>,
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
    // 镜像到兼容别名（见 StagedBatch::session_native_id 文档）：唯一权威来源
    // 是 report.session_native_id。
    let session_native_id = report.session_native_id.clone();
    Ok(StagedBatch {
        messages: sink.buffered,
        report,
        session_native_id,
    })
}

/// 从多个 provider adapter 中选出匹配的那个，再 stage（RFC-0002 §3 provider 选择）。
///
/// 对每个 adapter 调 `probe`：跳过报错或 ambiguous 的（不是候选）；在剩余候选里
/// 取置信度最高的（Confirmed > High > Low）。若并列最高有多个不同 variant，视为
/// 无法区分，拒绝（不做"猜一个"）。选中后用该 adapter stage。
///
/// 无任何候选 → `InvalidRequest`（没有 provider 认领此源）。
/// 组合根（CLI）持有具体 adapter 清单，本函数只负责与格式无关的选择编排。
pub fn select_and_stage(
    adapters: &[&dyn ProviderAdapter],
    bytes: &[u8],
) -> Result<StagedBatch, AppError> {
    // 置信度排序键：越大越可信；ambiguous 不参与。
    fn rank(c: Confidence) -> Option<u8> {
        match c {
            Confidence::Confirmed => Some(3),
            Confidence::High => Some(2),
            Confidence::Low => Some(1),
            Confidence::Ambiguous => None,
        }
    }

    let mut best: Option<(u8, usize, ProbeResult)> = None; // (rank, adapter index, probe)
    let mut tie = false;
    for (idx, adapter) in adapters.iter().enumerate() {
        // probe 报错的 adapter 不是候选——它明确表示"这不是我的格式"。
        let Ok(probe) = adapter.probe(bytes) else {
            continue;
        };
        let Some(r) = rank(probe.confidence) else {
            continue;
        };
        match &best {
            Some((best_rank, _, best_probe)) => {
                if r > *best_rank {
                    best = Some((r, idx, probe));
                    tie = false;
                } else if r == *best_rank && probe.variant_id != best_probe.variant_id {
                    // 同等置信度、不同 variant——无法区分，标记歧义。
                    tie = true;
                }
            }
            None => best = Some((r, idx, probe)),
        }
    }

    let (_, idx, probe) = best
        .ok_or_else(|| DomainError::InvalidRequest("no provider recognized this source".into()))?;
    if tie {
        return Err(DomainError::InvalidRequest(format!(
            "ambiguous provider selection: multiple variants matched with equal confidence \
             (one candidate was {})",
            probe.variant_id
        ))
        .into());
    }
    // 复用选中时的 probe 结果，不再对同一字节第二次 probe。
    stage_probed(adapters[idx], bytes, probe)
}

/// 内存 staging sink：只缓冲，绝不触库。
#[derive(Default)]
struct StagingSink {
    buffered: Vec<StagedMessage>,
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
}

/// 系统时钟（Unix 毫秒）。[`App::new`] 的默认时钟；测试经 [`App::with_clock`] 注入固定值。
fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct ContextOccurrence {
    message: ContextMessage,
    evidence: EvidenceSpanDto,
    estimated_bytes: usize,
}

/// 用例执行器：绑定所需端口，串起领域校验与端口调用。
///
/// 泛型而非 trait object——前端在构造期决定后端实现，零动态分发开销。
/// 时钟以 fn 指针注入：cursor 的发行/校验都不读环境时间（可测确定性）。
pub struct App<C: CatalogStore + ContextGraphStore, S: SearchIndex> {
    catalog: C,
    index: S,
    clock_ms: fn() -> i64,
}

impl<C: CatalogStore + ContextGraphStore, S: SearchIndex> App<C, S> {
    pub fn new(catalog: C, index: S) -> Self {
        Self {
            catalog,
            index,
            clock_ms: system_now_ms,
        }
    }

    /// 注入固定时钟的构造器（测试用）；生产路径一律 [`App::new`]。
    pub fn with_clock(catalog: C, index: S, clock_ms: fn() -> i64) -> Self {
        Self {
            catalog,
            index,
            clock_ms,
        }
    }

    /// 解析续读偏移：无令牌即第一页（offset 0）；有令牌则完整校验
    /// （结构/摘要/过期/generation/查询与排序绑定），失败显式报错，绝不静默回第一页。
    fn resolve_offset(
        &self,
        token: Option<&str>,
        active_generation: u64,
        query_digest: &str,
        sort_digest: &str,
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
                offset: offset_next,
            })
            .into_string(),
        )
    }

    /// 执行一个应用请求。校验错误保持 Domain 分类，端口错误保持 Port 分类，
    /// cursor/budget 错误保持各自分类（protocol 层有专属 canonical code）。
    pub fn handle(&self, req: AppRequest) -> Result<AppResponse, AppError> {
        match req {
            AppRequest::Search {
                query,
                limit,
                cursor: token,
                budget,
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
                budget.validate().map_err(AppError::from)?;
                let generation = self.catalog.active_generation()?;
                let query_digest = cursor::digest_query(&query);
                let offset = self.resolve_offset(
                    token.as_deref(),
                    generation,
                    &query_digest,
                    SORT_SCORE_DESC,
                )?;

                // 分页模型：钉住排序（bm25 + id tiebreak 全序）内的 offset 续读。
                // 端口无 offset 参数——超取 offset+page+1（+1 作 has_more 哨兵）后切片。
                let page = limit.min(budget.max_items);
                let fetch = offset
                    .saturating_add(page as u64)
                    .saturating_add(1)
                    .min(MAX_FETCH_WINDOW);
                let fetched = self.index.query(&query, fetch as usize)?;
                let fetched_len = fetched.len() as u64;
                let slice: Vec<SearchHit> = fetched
                    .into_iter()
                    .skip(usize::try_from(offset).unwrap_or(usize::MAX))
                    .take(page)
                    .collect();

                // R1（ADR-0004）：snippet 装配——对页内命中一次性批量取 payload
                // （分块 IN，无 N+1），解析 `text` 字段，按 `max_snippet_chars`
                // 截取前缀。payload 无 text（或非 JSON）→ None，不臆造正文；
                // 不做任何脱敏（所有者决定，本地优先工具接受屏显）。
                let ids: Vec<StableId> = slice.iter().map(|hit| hit.id.clone()).collect();
                let payloads = self.catalog.get_many(&ids)?;
                let max_snippet_chars = budget.max_snippet_chars;
                let mut hits = slice;
                for (hit, (_id, payload)) in hits.iter_mut().zip(payloads) {
                    hit.snippet = payload.and_then(|bytes| {
                        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                            return None;
                        };
                        value
                            .get("text")
                            .and_then(serde_json::Value::as_str)
                            .map(|text| text.chars().take(max_snippet_chars).collect())
                    });
                }
                let net_bytes = budget
                    .max_response_bytes
                    .saturating_sub(ENVELOPE_RESERVE_BYTES);
                let (hits, truncation, _) = budget::clamp_items(hits, page, net_bytes, |hit| {
                    // 最终渲染 `{id, score, text}`：snippet 字节计入同一字节闸
                    // （`,"text":` 为字段开销）；截断原因保持显式。
                    json_string_len(hit.id.as_str())
                        + hit.snippet.as_ref().map_or(0, |s| json_string_len(s) + 8)
                        + 32
                });
                let consumed = offset + hits.len() as u64;
                // A truncated page with zero kept hits cannot advance the
                // cursor offset; terminate paging instead of looping forever.
                let has_more = fetched_len > consumed && !hits.is_empty();
                let next_cursor = self.issue_cursor(
                    has_more,
                    generation,
                    &query_digest,
                    SORT_SCORE_DESC,
                    consumed,
                );
                Ok(AppResponse::Search {
                    hits,
                    next_cursor,
                    generation,
                    truncation,
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
            } => {
                if limit == 0 {
                    return Err(DomainError::InvalidRequest("limit must be > 0".into()).into());
                }
                budget.validate().map_err(AppError::from)?;
                let generation = self.catalog.active_generation()?;
                // list 无查询串；令牌以空串摘要 + wire_id_asc 排序标识绑定用例。
                let query_digest = cursor::digest_query("");
                let offset = self.resolve_offset(
                    token.as_deref(),
                    generation,
                    &query_digest,
                    SORT_WIRE_ID_ASC,
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
                let fetched = self.catalog.list(fetch as usize)?;
                let fetched_len = fetched.len() as u64;
                let slice: Vec<CatalogEntry> = fetched
                    .into_iter()
                    .skip(usize::try_from(offset).unwrap_or(usize::MAX))
                    .take(page)
                    .collect();
                let net_bytes = budget
                    .max_response_bytes
                    .saturating_sub(ENVELOPE_RESERVE_BYTES);
                let (entries, truncation, _) =
                    budget::clamp_items(slice, page, net_bytes, |entry| {
                        // 最终 JSON 形态 `{"id":"<id>","payload":"<lossy utf-8>"}`：
                        // id 按转义计长，payload 按序列化后长度计（不是原始字节数）。
                        json_string_len(entry.id.as_str())
                            + lossy_payload_json_len(&entry.payload)
                            + 18
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
                    SORT_WIRE_ID_ASC,
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
                budget,
            } => self.handle_context(session_id, policy, budget),
            AppRequest::MessageContexts { message_id } => self.handle_message_contexts(message_id),
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
                estimated_bytes,
            });
        }

        let net_bytes = budget
            .max_response_bytes
            .saturating_sub(ENVELOPE_RESERVE_BYTES)
            // The session payload is embedded verbatim in the response; count
            // it against the hard byte gate up front, before clamping keeps.
            .saturating_sub(session_bytes.len());
        let (kept_occurrences, mut truncation, _) =
            budget::clamp_items(occurrences, budget.max_messages, net_bytes, |occurrence| {
                occurrence.estimated_bytes
            });
        if truncation.reason.as_deref() == Some(budget::TRUNCATION_MAX_ITEMS) {
            // 消息条数闸对应的预算旋钮是 max_messages；如实报告该旋钮名。
            truncation.reason = Some(budget::TRUNCATION_MAX_MESSAGES.to_string());
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
            .into_iter()
            .map(|occurrence| occurrence.message)
            .collect();

        Ok(AppResponse::Context {
            session_id: session_id.as_str().to_string(),
            session,
            branch_leaf,
            branch_leaf_placement_id,
            messages,
            evidence,
            truncation,
            generation,
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
    use agent_session_grep_ports::{
        ContextStats, MessageContextCandidate as PortMessageContextCandidate, PortResult,
        SourceSnapshot,
    };
    use agent_session_grep_testkit::FakeProvider;

    /// 内存态假后端，仅用于用例逻辑测试。
    struct FakeCatalog;
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
            let id = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"listed"]);
            Ok(vec![CatalogEntry {
                id,
                payload: b"payload".to_vec(),
            }]
            .into_iter()
            .take(limit)
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
        fn query(&self, _query: &str, _limit: usize) -> PortResult<Vec<SearchHit>> {
            Ok(vec![SearchHit {
                id: StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"h"]),
                score: 1.0,
                snippet: None,
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
            limit: 10,
            cursor: None,
            budget: ResponseBudget::default(),
        });
        assert!(matches!(r, Ok(AppResponse::Search { hits, .. }) if hits.len() == 1));
    }

    #[test]
    fn search_rejects_zero_limit() {
        let r = app().handle(AppRequest::Search {
            query: "x".into(),
            limit: 0,
            cursor: None,
            budget: ResponseBudget::default(),
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
            limit: 5,
            cursor: None,
            budget: ResponseBudget::default(),
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
            fn query(&self, _query: &str, _limit: usize) -> PortResult<Vec<SearchHit>> {
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
    fn search_snippets_extract_text_from_payloads() {
        // R1（ADR-0004）：snippet 在 Application 检索装配时生成——批量取 payload、
        // 解析 `text` 字段、截取前缀；不做任何脱敏。
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
        assert_eq!(hits[0].snippet.as_deref(), Some("hello world"));
        assert_eq!(hits[1].snippet.as_deref(), Some("second hit"));
    }

    #[test]
    fn search_snippet_truncates_to_max_snippet_chars() {
        // R1.2：单条 snippet 按 `max_snippet_chars`（字符数）显式截取前缀。
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
                limit: 5,
                cursor: None,
                budget: ResponseBudget {
                    max_snippet_chars: 4,
                    ..Default::default()
                },
            })
            .unwrap();
        let AppResponse::Search { hits, .. } = resp else {
            panic!("expected Search response");
        };
        assert_eq!(hits[0].snippet.as_deref(), Some("abcd"));
    }

    #[test]
    fn search_snippet_none_when_payload_has_no_text() {
        // text 缺失 / 非字符串 / payload 非 JSON → snippet None（不臆造正文）。
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
        assert_eq!(hits[3].snippet.as_deref(), Some(""));
        assert!(hits[..3].iter().all(|hit| hit.snippet.is_none()));
    }

    #[test]
    fn search_byte_gate_charges_snippet_bytes() {
        // R1.2：snippet 字节计入同一 `max_response_bytes` 闸。无 snippet 时
        // 4 条命中全部放得下（每条仅 ~70 B）；带 1000 字符 snippet 时每条
        // ~1080 B，净预算 3072 只容 2 条——证明 snippet 字节被计入闸门，
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
                limit: 10,
                cursor: None,
                budget: ResponseBudget {
                    max_response_bytes: budget::MIN_RESPONSE_BYTES,
                    ..Default::default()
                },
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
        });
        assert!(matches!(r, Ok(AppResponse::List { entries, .. }) if entries.len() == 1));
    }

    #[test]
    fn list_rejects_zero_limit() {
        let err = app()
            .handle(AppRequest::List {
                limit: 0,
                cursor: None,
                budget: ResponseBudget::default(),
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
        fn query(&self, _query: &str, limit: usize) -> PortResult<Vec<SearchHit>> {
            Ok((0..self.n.min(limit))
                .map(|i| SearchHit {
                    id: StableId::derive(
                        IdKind::Message,
                        Stability::Reconstructed,
                        &[format!("hit{i:02}").as_bytes()],
                    ),
                    score: -(i as f32),
                    snippet: None,
                })
                .collect())
        }
    }

    /// 内存 map 目录：BTreeMap 键序即 wire id 升序（与 sqlite list 的钉住排序一致）；
    /// generation 用 Cell 可变，测 cursor 的 generation 绑定。
    struct MapCatalog {
        map: std::collections::BTreeMap<String, Vec<u8>>,
        generation: std::cell::Cell<u64>,
    }
    impl MapCatalog {
        fn new(generation: u64) -> Self {
            Self {
                map: Default::default(),
                generation: std::cell::Cell::new(generation),
            }
        }
        fn insert(&mut self, id: &StableId, payload: impl Into<Vec<u8>>) {
            self.map.insert(id.as_str().to_string(), payload.into());
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

        fn context_stats(&self) -> PortResult<ContextStats> {
            Ok(ContextStats::default())
        }
    }

    fn search_req(query: &str, limit: usize, cursor: Option<String>) -> AppRequest {
        AppRequest::Search {
            query: query.into(),
            limit,
            cursor,
            budget: ResponseBudget::default(),
        }
    }

    fn hits_of(resp: AppResponse) -> (Vec<String>, Option<String>, u64, Truncation) {
        match resp {
            AppResponse::Search {
                hits,
                next_cursor,
                generation,
                truncation,
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
                limit: 60,
                cursor: None,
                budget: ResponseBudget {
                    max_response_bytes: budget::MIN_RESPONSE_BYTES,
                    ..Default::default()
                },
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
                limit: 5,
                cursor: None,
                budget: ResponseBudget {
                    max_items: 0,
                    ..Default::default()
                },
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
}
