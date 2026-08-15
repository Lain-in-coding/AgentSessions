//! Provider Capability Matrix（能力矩阵单源权威）。
//!
//! 所有入口（CLI/MCP/Robot/Web）渲染 provider 能力必须读本矩阵，禁止硬编码。
//! 当前已实现的 2 个 provider（Claude Code、Codex）maturity=experimental。
//! 其余 14 个 provider 在 `08-15-sixteen-provider-evidence-wave` 任务中逐步实现。
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
    pub fn target_for(provider_id: &str) -> Option<Self> {
        match provider_id {
            "claude-code" | "codex" => Some(Self::Certified),
            "grok-build" | "opencode" | "pi" | "antigravity" | "kimi-code" | "openclaw"
            | "hermes" | "qoder" | "tencent-codebuddy" | "deepseek-harness" => Some(Self::Beta),
            "zcode" | "aider" | "cline" | "cursor" => Some(Self::Experimental),
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
    /// Handoff 能力。
    pub handoff: CapabilityLevel,
    /// 工具活动提取能力。
    pub tool_activity: CapabilityLevel,
    /// Source span 精度。
    pub source_span: CapabilityLevel,
    /// 增量同步能力。
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
    /// 返回当前已实现的 2 个 provider 的能力矩阵。
    /// 其余 14 个 provider 在 #2 任务实现后加入。
    pub fn current() -> Self {
        Self {
            providers: vec![
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
                    handoff: CapabilityLevel::Unsupported,
                    tool_activity: CapabilityLevel::Partial,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Native,
                },
                ProviderCapability {
                    provider_id: "codex".into(),
                    variant_id: "codex/rollout-jsonl-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    discover: CapabilityLevel::Native,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Native,
                    resume: CapabilityLevel::Derived,
                    handoff: CapabilityLevel::Unsupported,
                    tool_activity: CapabilityLevel::Partial,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Native,
                },
                ProviderCapability {
                    provider_id: "grok-build".into(),
                    variant_id: "grok-build/acp-updates-v1".into(),
                    maturity: ProviderMaturity::Experimental,
                    discover: CapabilityLevel::Unsupported,
                    probe: CapabilityLevel::Native,
                    parse: CapabilityLevel::Native,
                    search: CapabilityLevel::Native,
                    context: CapabilityLevel::Unsupported,
                    resume: CapabilityLevel::Unknown,
                    handoff: CapabilityLevel::Unsupported,
                    tool_activity: CapabilityLevel::Unsupported,
                    source_span: CapabilityLevel::Native,
                    incremental: CapabilityLevel::Unsupported,
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
            let target = ProviderMaturity::target_for(&p.provider_id);
            assert!(target.is_some(), "{} should have a target", p.provider_id);
            // 当前 maturity 均为 experimental；目标分级严格更高。
            assert_ne!(p.maturity, target.unwrap());
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
        for id in &["zcode", "aider", "cline", "cursor"] {
            assert_eq!(
                ProviderMaturity::target_for(id),
                Some(ProviderMaturity::Experimental)
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
