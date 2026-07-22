//! Application：跨前端（CLI / Robot-JSON / MCP / TUI）共享的用例与请求/结果 ADT。
//!
//! 本层不知道任何具体前端或后端——只依赖 domain 的类型与 ports 的抽象。
//! 所有前端把各自的输入归一为 [`AppRequest`]，把 [`AppResponse`] 渲染成各自的输出格式。
//! 分层依赖：domain ← ports ← application ← adapters（见计划 §5 crate 结构）。

use agentsessions_domain::{DomainError, StableId};
use agentsessions_ports::{
    CanonicalEventSink, CatalogEntry, CatalogStore, Confidence, MessageEvent, PortError,
    PortResult, ProviderAdapter, ProviderError, SearchHit, SearchIndex,
};

/// Application 边界错误：保留 Domain、Port 与 Provider 的原始分类，供各前端统一映射协议。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Port(#[from] PortError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

/// 应用层请求 ADT：所有前端的统一入口。
///
/// 每个变体是一个用例。前端负责解析各自语法后构造本枚举，
/// 从而保证 CLI / Robot / MCP / TUI 行为一致（见 CONTRACT-cli-robot-mcp-draft）。
#[derive(Debug, Clone, PartialEq)]
pub enum AppRequest {
    /// 全文检索：按查询串返回命中列表。
    Search {
        query: String,
        /// 最多返回条数；0 视为非法请求。
        limit: usize,
    },
    /// 按稳定 ID 取回单个实体的原始负载。
    Get { id: StableId },
    /// 按稳定 wire id 展开展示一个实体；当前 Beta 返回规范化 payload。
    Show { id: StableId },
    /// 按稳定 wire id 顺序列出实体；0 视为非法。
    List { limit: usize },
    /// 返回当前 Catalog 统计状态。
    Status,
}

/// 应用层结果 ADT：前端据此渲染，不再回到 domain/ports 类型。
#[derive(Debug, Clone, PartialEq)]
pub enum AppResponse {
    /// 检索结果，按相关性降序。
    Search { hits: Vec<SearchHit> },
    /// 单个实体的原始负载；`None` 表示未找到。
    Get { payload: Option<Vec<u8>> },
    /// 单个实体的规范化展示；`None` 表示未找到。
    Show { payload: Option<Vec<u8>> },
    /// 稳定排序后的 Catalog 条目。
    List { entries: Vec<CatalogEntry> },
    /// 当前 Catalog 实体总数与活动 generation。
    Status {
        catalog_count: u64,
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
}

/// Ingest 编排（RFC-0002 §5 source-level staging）：
///
/// 1. `probe` 判定 variant；ambiguous / 探测失败 → 立即拒绝，不 parse；
/// 2. `parse` 把 Canonical 消息推入**内存缓冲**（不写库）；
/// 3. 仅当 parse 完整成功才返回全部缓冲消息；任何失败返回 Err 且**不产出部分结果**。
///
/// 提交（写 catalog + FTS）由调用方在拿到完整 `Vec<StagedMessage>` 后，
/// 用单事务原子执行（见 `SqliteStore::commit_batch`）。
pub fn stage(adapter: &dyn ProviderAdapter, bytes: &[u8]) -> Result<Vec<StagedMessage>, AppError> {
    let probe = adapter.probe(bytes)?;
    // ambiguous 置信度：默认拒绝解析，不做"尽量解析"（RFC-0002 §3）。
    if matches!(probe.confidence, Confidence::Ambiguous) {
        return Err(DomainError::InvalidRequest(format!(
            "ambiguous variant {}: {:?}",
            probe.variant_id, probe.unmatched_evidence
        ))
        .into());
    }
    let mut sink = StagingSink::default();
    adapter.parse(bytes, &mut sink)?;
    Ok(sink.buffered)
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
) -> Result<Vec<StagedMessage>, AppError> {
    // 置信度排序键：越大越可信；ambiguous 不参与。
    fn rank(c: Confidence) -> Option<u8> {
        match c {
            Confidence::Confirmed => Some(3),
            Confidence::High => Some(2),
            Confidence::Low => Some(1),
            Confidence::Ambiguous => None,
        }
    }

    let mut best: Option<(u8, usize, String)> = None; // (rank, adapter index, variant)
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
            Some((best_rank, _, best_variant)) => {
                if r > *best_rank {
                    best = Some((r, idx, probe.variant_id));
                    tie = false;
                } else if r == *best_rank && probe.variant_id != *best_variant {
                    // 同等置信度、不同 variant——无法区分，标记歧义。
                    tie = true;
                }
            }
            None => best = Some((r, idx, probe.variant_id)),
        }
    }

    let (_, idx, variant) = best
        .ok_or_else(|| DomainError::InvalidRequest("no provider recognized this source".into()))?;
    if tie {
        return Err(DomainError::InvalidRequest(format!(
            "ambiguous provider selection: multiple variants matched with equal confidence \
             (one candidate was {variant})"
        ))
        .into());
    }
    stage(adapters[idx], bytes)
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
        });
        Ok(())
    }
}

/// 用例执行器：绑定所需端口，串起领域校验与端口调用。
///
/// 泛型而非 trait object——前端在构造期决定后端实现，零动态分发开销。
pub struct App<C: CatalogStore, S: SearchIndex> {
    catalog: C,
    index: S,
}

impl<C: CatalogStore, S: SearchIndex> App<C, S> {
    pub fn new(catalog: C, index: S) -> Self {
        Self { catalog, index }
    }

    /// 执行一个应用请求。校验错误保持 Domain 分类，端口错误保持 Port 分类。
    pub fn handle(&self, req: AppRequest) -> Result<AppResponse, AppError> {
        match req {
            AppRequest::Search { query, limit } => {
                if limit == 0 {
                    return Err(DomainError::InvalidRequest("limit must be > 0".into()).into());
                }
                if query.trim().is_empty() {
                    return Err(
                        DomainError::InvalidRequest("query must not be empty".into()).into(),
                    );
                }
                let hits = self.index.query(&query, limit)?;
                Ok(AppResponse::Search { hits })
            }
            AppRequest::Get { id } => {
                let payload = self.catalog.get(&id)?;
                Ok(AppResponse::Get { payload })
            }
            AppRequest::Show { id } => {
                let payload = self.catalog.get(&id)?;
                Ok(AppResponse::Show { payload })
            }
            AppRequest::List { limit } => {
                if limit == 0 {
                    return Err(DomainError::InvalidRequest("limit must be > 0".into()).into());
                }
                let entries = self.catalog.list(limit)?;
                Ok(AppResponse::List { entries })
            }
            AppRequest::Status => {
                let catalog_count = self.catalog.count()?;
                let active_generation = self.catalog.active_generation()?;
                Ok(AppResponse::Status {
                    catalog_count,
                    active_generation,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentsessions_domain::{IdKind, Stability};
    use agentsessions_ports::{PortResult, SourceSnapshot};
    use agentsessions_testkit::FakeProvider;

    /// 内存态假后端，仅用于用例逻辑测试。
    struct FakeCatalog;
    impl CatalogStore for FakeCatalog {
        fn get(&self, _id: &StableId) -> PortResult<Option<Vec<u8>>> {
            Ok(Some(b"payload".to_vec()))
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

    struct FakeIndex;
    impl SearchIndex for FakeIndex {
        fn index(&self, _id: &StableId, _text: &str) -> PortResult<()> {
            Ok(())
        }
        fn query(&self, _query: &str, _limit: usize) -> PortResult<Vec<SearchHit>> {
            Ok(vec![SearchHit {
                id: StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"h"]),
                score: 1.0,
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
        });
        assert!(matches!(r, Ok(AppResponse::Search { hits }) if hits.len() == 1));
    }

    #[test]
    fn search_rejects_zero_limit() {
        let r = app().handle(AppRequest::Search {
            query: "x".into(),
            limit: 0,
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
        });
        assert!(matches!(
            r.unwrap_err(),
            AppError::Domain(DomainError::InvalidRequest(_))
        ));
    }

    #[test]
    fn get_returns_payload() {
        let id = StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"g"]);
        let r = app().handle(AppRequest::Get { id });
        assert!(matches!(r, Ok(AppResponse::Get { payload: Some(_) })));
    }

    #[test]
    fn list_returns_stable_entries() {
        let r = app().handle(AppRequest::List { limit: 10 });
        assert!(matches!(r, Ok(AppResponse::List { entries }) if entries.len() == 1));
    }

    #[test]
    fn list_rejects_zero_limit() {
        let err = app().handle(AppRequest::List { limit: 0 }).unwrap_err();
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
                active_generation: 7
            })
        ));
    }

    // ---- stage（RFC-0002 §5）----

    #[test]
    fn stage_returns_all_messages_on_success() {
        let provider = FakeProvider::good("demo", &[("user", "hi"), ("assistant", "yo")]);
        let staged = stage(&provider, b"anything").unwrap();
        assert_eq!(staged.len(), 2);
        assert_eq!(staged[0].seq, 0);
        assert_eq!(staged[0].role, "user");
        assert_eq!(staged[0].text, "hi");
        assert_eq!(staged[1].seq, 1);
        assert_eq!(staged[1].role, "assistant");
        assert_eq!(staged[1].text, "yo");
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
