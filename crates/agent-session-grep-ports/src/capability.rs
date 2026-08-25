//! Provider Capability Matrix（能力矩阵单源权威）。
//!
//! 所有入口（CLI/MCP/Robot/Web）渲染 provider 能力必须读本矩阵，禁止硬编码。
//! 当前已实现的 provider maturity=experimental，在 `08-15-sixteen-provider-evidence-wave`
//! 任务中逐步实现并晋级。
//!
//! 参考：ctx 的 `provider-support-matrix.json` 格式（idea-only）。

/// Provider 成熟度分级（诚实公开评级，证据晋级，禁止跨级宣传）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMaturity {
    /// 可发现、可 probe、可解析小 fixture，限制明确。
    Experimental,
    /// 主路径 fixture、golden、contract、只读、增量、source span 均通过。
    Beta,
    /// 完整能力矩阵达成，历史 variant/混合版本/崩溃恢复/正式 target/性能与回滚证据齐备。
    Ga,
    /// 跨平台 Gate D + golden + 全能力链 + owner 晋级决策。
    Certified,
    /// 明确不支持，附原因。
    Unsupported,
}

impl ProviderMaturity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
            Self::Beta => "beta",
            Self::Ga => "ga",
            Self::Certified => "certified",
            Self::Unsupported => "unsupported",
        }
    }

    /// 目标分级（路线图），不是当前实现状态。
    /// Deferred provider（无 transcript 证据）不声明目标，返回 None。
    pub fn target_for(provider_id: &str) -> Option<Self> {
        match provider_id {
            "claude-code" | "codex" => Some(Self::Certified),
            "grok-build" | "opencode" | "pi" | "antigravity" | "kimi-code" | "openclaw"
            | "hermes" | "qoder" | "tencent-codebuddy" => Some(Self::Beta),
            "aider" | "cline" | "cursor" => Some(Self::Experimental),
            // deepseek-harness / zcode: 无证据 deferred，不设目标
            _ => None,
        }
    }
}

/// 单项能力级别（逐字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityLevel {
    /// provider 原生提供。
    Native,
    /// 由内在内容确定性派生。
    Derived,
    /// 部分场景可得。
    Partial,
    /// 该 provider 无此概念。
    Unsupported,
    /// 尚未评估。
    #[default]
    Unknown,
}

/// 单个 provider 的能力声明。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderCapability {
    /// Canonical provider id（如 `claude-code`、`codex`）。
    pub provider_id: String,
    /// Variant 标识（如 `claude-code/jsonl-v1`）。
    pub variant_id: String,
    /// 当前成熟度（事实，非目标）。
    pub maturity: ProviderMaturity,
    /// 发现源目录的能力。
    pub discover: CapabilityLevel,
    /// Probe 能力。
    pub probe: CapabilityLevel,
    /// 解析能力。
    pub parse: CapabilityLevel,
    /// 搜索能力。
    pub search: CapabilityLevel,
    /// 上下文图能力。
    pub context: CapabilityLevel,
    /// Resume 能力。
    pub resume: CapabilityLevel,
    /// Handoff 能力（能否为该 provider 的会话装出带原文证据的 handoff pack）。
    ///
    /// 诚实口径：`handoff` 不是 per-provider 特性。生成器
    /// （`application::handoff_pack::generate_deterministic`）只消费 `SearchHit`
    /// 与权威 source placement，`provenance` 与 `matched_sessions[].provider_id`
    /// 一律为 `None`（搜索型 pack 无单一提供商，不臆造），全流程不读 provider
    /// 身份、无 per-provider 分支。因此"能否装出带证据的 pack"只取决于消息是否
    /// 落库并带 source placement——`parse` 可用即成立。
    ///
    /// 故 14 个已实现 provider 一律 `Derived`（由已落库内容确定性派生），
    /// 此前全列 `Unsupported` 是少报：`asg handoff` 是已发布命令，对 codex
    /// （JSONL）、aider（markdown，native_id 恒空）、opencode（SQLite）三种结构
    /// 迥异的真实 golden 源实测均产出 `confidence: high` 的带证据 pack。
    /// 由 `handoff_pack_generation_is_provider_independent` 守护。
    pub handoff: CapabilityLevel,
    /// 工具活动提取能力。
    pub tool_activity: CapabilityLevel,
    /// Source span 精度。
    pub source_span: CapabilityLevel,
    /// 增量同步能力（未变化的源 resync 是否为 no-op）。
    ///
    /// 诚实口径：与 [`Self::handoff`] 同理，`incremental` 也不是 per-provider
    /// 特性。判定链全在 composition root + store 层：`sync` 先读 `source_scans`
    /// 的 `(len_bytes, fingerprint)` 缓存，与当次快照的 BLAKE3 指纹比对，相同则
    /// **跳过 parse**（`unchanged` 上报已存消息数），再由
    /// `commit_source_batches_if_changed` 做内容级 no-op 判定、不推进 generation。
    /// 这条链上没有任何 per-provider 分支，adapter 也不参与。
    ///
    /// 故 14 个已实现 provider 一律 `Derived`——由源字节确定性派生，而非 provider
    /// 原生提供。此前 claude-code/codex 记 `Native`、其余 12 个记 `Unsupported`
    /// 都不准：前者把 store 层能力误记为 provider 原生，后者是少报（12 个
    /// provider 的真实 golden 源实测 resync 均为 `committed=0` /
    /// `unchanged=N` / generation 不变）。由 e2e
    /// `capability_incremental_claim_matches_real_resync_for_every_provider`
    /// 逐 provider 实测守护。
    pub incremental: CapabilityLevel,
}

impl ProviderCapability {
    #[allow(dead_code)]
    fn unknown(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            variant_id: String::new(),
            maturity: ProviderMaturity::Unsupported,
            discover: CapabilityLevel::Unknown,
            probe: CapabilityLevel::Unknown,
            parse: CapabilityLevel::Unknown,
            search: CapabilityLevel::Unknown,
            context: CapabilityLevel::Unknown,
            resume: CapabilityLevel::Unknown,
            handoff: CapabilityLevel::Unknown,
            tool_activity: CapabilityLevel::Unknown,
            source_span: CapabilityLevel::Unknown,
            incremental: CapabilityLevel::Unknown,
        }
    }
}

/// 全部 provider 的能力矩阵。入口层渲染 provider 能力的唯一来源。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderCapabilityMatrix {
    pub providers: Vec<ProviderCapability>,
}

impl ProviderCapabilityMatrix {
    /// 返回当前 16 个 provider 的能力矩阵（evidence wave 08-15 全量：
    /// 14 个已实现 + 2 个 deferred）。deferred provider（deepseek-harness/zcode）
    /// 无 transcript 证据，保持 Unsupported 不宣传。
    pub fn current() -> Self {
        Self {
            providers: vec![
                ProviderCapability {
                    provider_id: "aider".into(),
                    variant_id: "aider/chat-history-md-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 按构造是 home 相对的单根表，而
                    // aider 的 canonical 源是**每个 repo 各一份**的
                    // `<repo>/.aider.chat.history.md`——home 下没有汇总目录可登记。
                    // 本 adapter 所引上游 agentsview 也不是查表：它从任意用户给定
                    // 根递归找该文件名，并显式跳过 macOS 受保护的一级 home 目录
                    // （`aiderProtectedHomeDirs`）。这是"结构上不适用本表"，不是
                    // "根未知"：真要支持得先有 repo 集合来源（workspace 列表），
                    // 属独立任务。当前经显式 `sync <file>` 路径完整可用。
                    discover: CapabilityLevel::Unsupported,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unsupported,
                    handoff: CapabilityLevel::Derived,
                    // adapter 从不调用 `emit_activity`（零调用点），且消息以空
                    // native id 上报——composition root 对空锚点 fail-closed 丢弃，
                    // 故即便未来 emit 也无法附着。如实降级为 unsupported。
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Derived,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "claude-code".into(),
                    variant_id: "claude-code/jsonl-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Native,
                    resume: CapabilityLevel::Derived,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Partial,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "codex".into(),
                    variant_id: "codex/rollout-jsonl-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    // Codex rollout 是线性序列，不带显式 threading 边：adapter 对每条
                    // 消息硬编码 `parent_native_id: None`（见 provider-codex 模块文档
                    // "无 `parentUuid`……故 parent 一律 `None`（诚实：不编造上层可推断
                    // 的线性链）"）。没有边就没有上下文图可组装——`context` 从
                    // Native 降级为 Unsupported，此前的 Native 是虚报，由
                    // `capability_context_claim_matches_pinned_golden_parent_links` 抓出。
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Derived,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Partial,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "grok-build".into(),
                    variant_id: "grok-build/acp-updates-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.grok/sessions`（JSONL 源）。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    // resume 命令已由 application::resume builder 支持（grok --resume），
                    // 与矩阵一致标记 Derived（audit P1-2 drift 测试守护）。
                    resume: CapabilityLevel::Derived,
                    handoff: CapabilityLevel::Derived,
                    // ACP 流确有结构化工具记录（user_message_chunk 的
                    // `content._meta.bashCommand`，adapter 作为非对话元 chunk 跳过），
                    // 但格式无 per-message native id（promptId/promptIndex 是
                    // prompt 级分组键）——消息以空 native id 上报，活动无法锚定
                    // （staging fail-closed 丢弃）。如实保持 Unsupported。
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "pi".into(),
                    variant_id: "pi/session-jsonl-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.pi/agent/sessions`（JSONL 源，
                    // 按 cwd 编码分子目录，递归扫描覆盖）。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Derived,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "kimi-code".into(),
                    variant_id: "kimi-code/wire-jsonl-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.kimi-code/sessions`（JSONL 源）。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "qoder".into(),
                    variant_id: "qoder/transcript-jsonl-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.qoder/projects`（transcript
                    // JSONL 树）。该 root 只覆盖官方 transcript 面，不含 Qoder
                    // Electron 端的 SQLite 会话库（见 adapter 模块文档）。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "openclaw".into(),
                    variant_id: "openclaw/session-jsonl-v3".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.openclaw/agents`，JSONL 源可被
                    // discover 扫描收集；此前记为 unsupported 与代码相反。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unsupported,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "tencent-codebuddy".into(),
                    variant_id: "tencent-codebuddy/cli-jsonl-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.codebuddy/projects`（JSONL 源）。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "opencode".into(),
                    variant_id: "opencode/sqlite-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.local/share/opencode` 并声明
                    // 扩展名 `db`：源是单个 SQLite `opencode.db`，`-wal`/`-shm` 旁文件的
                    // extension 不是 `db`，精确匹配天然排除。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    // resume 命令尚无权威模板（builder 未支持），如实标记 Unknown——
                    // 曾误标 Derived（audit P1-2 drift 测试守护）。
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Unsupported,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "cline".into(),
                    variant_id: "cline/api-conversation-history-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.cline/data/tasks`（扩展名
                    // `json`）：ctx 的 history_locations 与其 fixture 布局一致地把
                    // task 目录放在该根下。同目录的三个旁文件都不是 role 数组，
                    // 本 adapter 的 probe 如实拒绝，故登记该根不会误收。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unsupported,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Unsupported,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "hermes".into(),
                    variant_id: "hermes/session-json-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.hermes/sessions`（扩展名
                    // `json`）：该 root 由 adapter 所引 hstry 上游硬编码并自证归属，
                    // 不是本机推测。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Unsupported,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "antigravity".into(),
                    variant_id: "antigravity/transcript-jsonl-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // `PROVIDER_DISCOVERY_ROOTS` 注册 `~/.gemini/antigravity-cli/brain`（JSONL 源）。
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Derived,
                    // step 记录确带 `tool_calls` 字段（adapter 忽略），但
                    // `step_index` 是文件内序号而非跨文档 durable id（synthetic
                    // 值会碰撞），消息以空 native id 上报——活动无法锚定
                    // （staging fail-closed 丢弃）。如实保持 Unsupported。
                    tool_activity: CapabilityLevel::Unsupported,
                    // 行式 JSONL：adapter 逐记录发 `span: Some((start, end))`，
                    // golden `golden_spans_slice_back_to_exact_source_lines`
                    // 逐字节校验切片。此前记为 unsupported 与代码相反。
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "cursor".into(),
                    variant_id: "cursor/vscdb-chat-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    // 本 adapter 的面是 VS Code workspaceStorage 的 `state.vscdb`
                    // （`ItemTable` KV + chatdata/prompts 键），其 workspaceStorage
                    // 布局无本机证据，不猜路径。
                    //
                    // 另有一个**看似可登记但实则错位**的根：fast-resume 的
                    // `cursor_chats_dir()` = `~/.cursor/chats`，下面是
                    // `<id>/store.db`。那是 Cursor **CLI** 的库，schema 为
                    // `meta`/`blobs` 两张 KV 表，没有 `ItemTable`——本 adapter 的
                    // probe 会一律 `AmbiguousVariant` 拒绝。登记它只会让扫描
                    // "完整地"收下一批注定解析失败的源，并对外宣称 discover 可用，
                    // 比留 unsupported 更不诚实。该面需要独立的 CLI variant，未实现。
                    discover: CapabilityLevel::Unsupported,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Derived,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Unsupported,
                    incremental: CapabilityLevel::Derived,
                },
                ProviderCapability {
                    provider_id: "deepseek-harness".into(),
                    variant_id: String::new(),
                    maturity: ProviderMaturity::Unsupported,
                    discover: CapabilityLevel::Unknown,
                    probe: CapabilityLevel::Unknown,
                    parse: CapabilityLevel::Unknown,
                    search: CapabilityLevel::Unknown,
                    context: CapabilityLevel::Unknown,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Unknown,
                    tool_activity: CapabilityLevel::Unknown,
                    source_span: CapabilityLevel::Unknown,
                    incremental: CapabilityLevel::Unknown,
                },
                ProviderCapability {
                    provider_id: "zcode".into(),
                    variant_id: String::new(),
                    maturity: ProviderMaturity::Unsupported,
                    discover: CapabilityLevel::Unknown,
                    probe: CapabilityLevel::Unknown,
                    parse: CapabilityLevel::Unknown,
                    search: CapabilityLevel::Unknown,
                    context: CapabilityLevel::Unknown,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Unknown,
                    tool_activity: CapabilityLevel::Unknown,
                    source_span: CapabilityLevel::Unknown,
                    incremental: CapabilityLevel::Unknown,
                },
            ],
        }
    }

    /// 按名称查找 provider 能力。
    pub fn find(&self, provider_id: &str) -> Option<&ProviderCapability> {
        self.providers.iter().find(|p| p.provider_id == provider_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_matrix_has_implemented_providers() {
        let m = ProviderCapabilityMatrix::current();
        assert!(m.providers.len() >= 2);
        assert!(m.find("claude-code").is_some());
        assert!(m.find("codex").is_some());
        assert!(m.find("grok-build").is_some());
    }

    #[test]
    fn implemented_providers_are_experimental() {
        let m = ProviderCapabilityMatrix::current();
        for p in &m.providers {
            // Deferred providers (deepseek-harness/zcode) 保持 Unsupported 无证据，跳过。
            if p.maturity == ProviderMaturity::Unsupported {
                continue;
            }
            assert_eq!(
                p.maturity,
                ProviderMaturity::Experimental,
                "{} 应为 experimental（当前事实，非目标）",
                p.provider_id
            );
        }
    }
    #[test]
    fn target_maturity_is_higher_than_current() {
        let m = ProviderCapabilityMatrix::current();
        for p in &m.providers {
            // Deferred providers (no evidence yet) have no declared target;
            // they stay Unsupported until evidence arrives.
            if p.maturity == ProviderMaturity::Unsupported {
                assert!(
                    ProviderMaturity::target_for(&p.provider_id).is_none(),
                    "{} is unsupported/deferred and must not claim a target",
                    p.provider_id
                );
                continue;
            }
            let target = ProviderMaturity::target_for(&p.provider_id);
            assert!(target.is_some(), "{} should have a target", p.provider_id);
            // Providers whose target IS experimental (cline, aider, zcode, cursor)
            // may have target == current; all others should have target strictly
            // higher than the current experimental maturity.
            if target.unwrap() != ProviderMaturity::Experimental {
                assert_ne!(
                    p.maturity,
                    target.unwrap(),
                    "{} target should differ from current experimental",
                    p.provider_id
                );
            }
        }
    }

    #[test]
    fn unknown_provider_has_no_target() {
        assert!(ProviderMaturity::target_for("nonexistent").is_none());
    }

    #[test]
    fn beta_targets_are_correct() {
        assert_eq!(
            ProviderMaturity::target_for("grok-build"),
            Some(ProviderMaturity::Beta)
        );
        assert_eq!(
            ProviderMaturity::target_for("opencode"),
            Some(ProviderMaturity::Beta)
        );
    }

    #[test]
    fn experimental_targets_are_correct() {
        for id in &["aider", "cline", "cursor"] {
            assert_eq!(
                ProviderMaturity::target_for(id),
                Some(ProviderMaturity::Experimental)
            );
        }
        // zcode 无证据 deferred，与 deepseek-harness 一致：不声明目标
        assert!(ProviderMaturity::target_for("zcode").is_none());
    }

    #[test]
    fn current_matrix_has_all_sixteen_providers() {
        let m = ProviderCapabilityMatrix::current();
        assert_eq!(
            m.providers.len(),
            16,
            "matrix must list all 16 providers (evidence wave 08-15), got {}",
            m.providers.len()
        );
    }

    #[test]
    fn deepseek_harness_and_zcode_are_deferred() {
        let m = ProviderCapabilityMatrix::current();
        for id in &["deepseek-harness", "zcode"] {
            let p = m
                .find(id)
                .unwrap_or_else(|| panic!("{id} should be in matrix"));
            assert_eq!(
                p.maturity,
                ProviderMaturity::Unsupported,
                "{id} must remain unsupported/deferred until real transcript evidence exists"
            );
            assert!(
                p.variant_id.is_empty(),
                "{id} variant must be empty when deferred, got {}",
                p.variant_id
            );
        }
    }

    #[test]
    fn hermes_antigravity_cursor_are_experimental() {
        let m = ProviderCapabilityMatrix::current();
        for id in &["hermes", "antigravity", "cursor"] {
            let p = m
                .find(id)
                .unwrap_or_else(|| panic!("{id} should be in matrix"));
            assert_eq!(
                p.maturity,
                ProviderMaturity::Experimental,
                "{id} should be experimental (evidence wave 08-16)"
            );
            assert!(
                !p.variant_id.is_empty(),
                "{id} should have a variant id after evidence-wave implementation"
            );
        }
    }

    #[test]
    fn matrix_serializes_to_json() {
        let m = ProviderCapabilityMatrix::current();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("claude-code"));
        assert!(json.contains("grok-build"));
        assert!(json.contains("experimental"));
        let back: ProviderCapabilityMatrix = serde_json::from_str(&json).unwrap();
        assert_eq!(back.providers.len(), m.providers.len());
    }

    #[test]
    fn maturity_as_str_round_trips() {
        for &m in &[
            ProviderMaturity::Experimental,
            ProviderMaturity::Beta,
            ProviderMaturity::Ga,
            ProviderMaturity::Certified,
            ProviderMaturity::Unsupported,
        ] {
            let s = m.as_str();
            let json = serde_json::to_string(&m).unwrap();
            assert!(json.contains(s), "{json} should contain {s}");
        }
    }
}
