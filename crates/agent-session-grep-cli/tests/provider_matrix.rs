//! Provider maturity/capability matrix 防漂移测试。
//!
//! matrix 文档（`docs/product/PROVIDER-MATURITY-MATRIX.md`）声明每个 provider 的
//! `provider_id` 与 `variant_id`。这里把文档 include 进测试二进制，并用真实 adapter
//! probe 出的标识对照——文档与代码任一改动而未同步，CI 立即失败（与 Robot envelope
//! schema 用 include_str! 交叉验证同一纪律）。
//!
//! 下半部分是 beta-readiness ledger（`docs/product/PROVIDER-BETA-READINESS.md`）的
//! 可执行漂移测试：14 个已实现 adapter 的真实 `AdapterManifest`（经由各 adapter 的
//! `manifest()` → `manifest_for`）必须与 ledger 的本地列和全局 blocker 一致，且
//! ledger 必须恰好列出 capability.rs 的 14 个已实现 + 2 个 deferred provider。

use agent_session_grep_ports::ProviderAdapter;
use agent_session_grep_ports::capability::{
    CapabilityLevel, ProviderCapabilityMatrix, ProviderMaturity,
};
use agent_session_grep_provider_aider::AiderAdapter;
use agent_session_grep_provider_antigravity::AntigravityAdapter;
use agent_session_grep_provider_claude::ClaudeCodeAdapter;
use agent_session_grep_provider_cline::ClineAdapter;
use agent_session_grep_provider_codebuddy::CodeBuddyAdapter;
use agent_session_grep_provider_codex::CodexAdapter;
use agent_session_grep_provider_cursor::CursorAdapter;
use agent_session_grep_provider_grok::GrokBuildAdapter;
use agent_session_grep_provider_hermes::OpenHermesAdapter;
use agent_session_grep_provider_kimi::KimiCodeAdapter;
use agent_session_grep_provider_openclaw::OpenClawAdapter;
use agent_session_grep_provider_opencode::OpenCodeAdapter;
use agent_session_grep_provider_pi::PiAdapter;
use agent_session_grep_provider_qoder::QoderAdapter;

/// matrix 文档原文（相对本源文件路径）。
const MATRIX: &str = include_str!("../../../docs/product/PROVIDER-MATURITY-MATRIX.md");

/// 一段最小的 Claude Code JSONL（触发 confirmed probe）。
const CLAUDE_SAMPLE: &str =
    r#"{"type":"user","uuid":"u-1","message":{"role":"user","content":"hi"}}"#;

/// 一段最小的 Codex rollout（session_meta + 一条权威 message）。
const CODEX_SAMPLE: &str = concat!(
    r#"{"timestamp":"2026-07-19T15:40:00.000Z","type":"session_meta","payload":{"session_id":"s-1"}}"#,
    "\n",
    r#"{"timestamp":"2026-07-19T15:41:00.000Z","type":"response_item","payload":{"type":"message","id":"m-1","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#,
);

/// 解析 "成熟度总览" 表的数据行：`(provider 名, provider_id, variant, maturity)`。
/// 列值去掉 markdown 修饰（反引号/加粗），便于与 adapter 的裸标识比对。
fn matrix_rows() -> Vec<(String, String, String, String)> {
    let mut rows = Vec::new();
    for line in MATRIX.lines() {
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().trim_matches('`').trim_matches('*'))
            .collect();
        // 表头（Provider 列）与分隔行（--- 列）跳过。
        if cells.len() < 4 || cells[0] == "Provider" || cells[1].starts_with("---") {
            continue;
        }
        rows.push((
            cells[0].to_string(),
            cells[1].to_string(),
            cells[2].to_string(),
            cells[3].to_string(),
        ));
    }
    rows
}

#[test]
fn matrix_rows_declare_exact_provider_ids_variants_and_maturity() {
    let claude = ClaudeCodeAdapter::new();
    let codex = CodexAdapter::new();
    let rows = matrix_rows();
    let claude_variant = claude.probe(CLAUDE_SAMPLE.as_bytes()).unwrap().variant_id;
    let codex_variant = codex.probe(CODEX_SAMPLE.as_bytes()).unwrap().variant_id;

    let claude_row = rows
        .iter()
        .find(|(_, provider_id, _, _)| provider_id == claude.provider_id())
        .unwrap_or_else(|| panic!("matrix 缺少 provider_id `{}` 的行", claude.provider_id()));
    assert_eq!(
        claude_row.2, claude_variant,
        "variant 列必须与 adapter 一致"
    );
    let codex_row = rows
        .iter()
        .find(|(_, provider_id, _, _)| provider_id == codex.provider_id())
        .unwrap_or_else(|| panic!("matrix 缺少 provider_id `{}` 的行", codex.provider_id()));
    assert_eq!(codex_row.2, codex_variant, "variant 列必须与 adapter 一致");
}

#[test]
fn matrix_marks_both_providers_experimental_not_beta() {
    // 诚实门：两个 provider 尚未有 property/fuzz/golden 覆盖，只能是 Experimental。
    // 若有人把文档改成 Beta/GA 却没补证据，此断言提醒回到晋级标准。
    let rows = matrix_rows();
    for provider in [
        ClaudeCodeAdapter::new().provider_id(),
        CodexAdapter::new().provider_id(),
    ] {
        let row = rows
            .iter()
            .find(|(_, provider_id, _, _)| provider_id == provider)
            .unwrap_or_else(|| panic!("matrix 缺少 provider_id `{provider}` 的行"));
        assert_eq!(
            row.3, "Experimental",
            "provider `{provider}` 当前只能 Experimental，晋级必须有证据"
        );
    }
}

#[test]
fn matrix_rows_match_capability_matrix_all_sixteen() {
    // capability.rs 是单源权威（本文件头注释声明同一纪律）；矩阵文档必须与它
    // 逐行一致。任一侧增删 provider 或改 variant/maturity 而未同步，立即失败。
    let matrix = ProviderCapabilityMatrix::current();
    let rows = matrix_rows();
    assert_eq!(matrix.providers.len(), 16, "capability.rs 应有 16 行");
    assert_eq!(rows.len(), 16, "矩阵文档应有 16 行");

    for cap in &matrix.providers {
        let row = rows
            .iter()
            .find(|(_, provider_id, _, _)| provider_id == &cap.provider_id)
            .unwrap_or_else(|| panic!("矩阵文档缺少 provider_id `{}` 的行", cap.provider_id));
        // deferred provider 无 variant，文档以 — 占位。
        let expected_variant = if cap.variant_id.is_empty() {
            "—"
        } else {
            cap.variant_id.as_str()
        };
        assert_eq!(
            row.2, expected_variant,
            "variant 列必须与 capability.rs 一致 (provider `{}`)",
            cap.provider_id
        );
        // maturity 列文档用首字母大写，Unsupported 行带（deferred）注解，
        // 因此按 capability.rs 的 as_str 前缀匹配。
        assert!(
            row.3.to_lowercase().starts_with(cap.maturity.as_str()),
            "maturity 列必须与 capability.rs 一致 (provider `{}`，文档为 `{}`)",
            cap.provider_id,
            row.3
        );
    }
}

// ---- PROVIDER-BETA-READINESS.md ledger 防漂移 ----

/// ledger 文档原文（相对本源文件路径）。
const BETA_READINESS: &str = include_str!("../../../docs/product/PROVIDER-BETA-READINESS.md");

/// 解析 ledger 中指定 `## ` 小节下的 markdown 表数据行，每行拆成 cell。
/// 表头行（`provider_id` 列）与分隔行（`---` 列）跳过；小节之外的表格
/// （如 Global external blockers）不收集。
fn beta_ledger_rows(section: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut in_section = false;
    for line in BETA_READINESS.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_section = heading.starts_with(section);
            continue;
        }
        if !in_section || !line.starts_with('|') {
            continue;
        }
        let cells: Vec<String> = line
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().trim_matches('`').to_string())
            .collect();
        if cells
            .first()
            .is_none_or(|first| first == "provider_id" || first.starts_with("---"))
        {
            continue;
        }
        rows.push(cells);
    }
    rows
}

/// ledger 本地列取值 → capability.rs 的 `CapabilityLevel`。
/// legend：`ok (native)`/`native` = Native；`missing` = capability matrix 中
/// 未实现/不支持（即 Unsupported）；其余与等级名一一对应。
fn beta_ledger_cell_to_level(cell: &str) -> Option<CapabilityLevel> {
    let value = cell.strip_prefix("ok").map(str::trim).unwrap_or(cell);
    let value = value.trim_start_matches('(').trim_end_matches(')').trim();
    Some(match value {
        "native" => CapabilityLevel::Native,
        "derived" => CapabilityLevel::Derived,
        "partial" => CapabilityLevel::Partial,
        "unsupported" | "missing" => CapabilityLevel::Unsupported,
        "unknown" => CapabilityLevel::Unknown,
        _ => return None,
    })
}

/// 与 CLI `provider_registry()` 同序的 14 个已实现 adapter。
fn implemented_adapters() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(ClaudeCodeAdapter::new()),
        Box::new(AiderAdapter::new()),
        Box::new(CodexAdapter::new()),
        Box::new(GrokBuildAdapter::new()),
        Box::new(PiAdapter::new()),
        Box::new(QoderAdapter::new()),
        Box::new(KimiCodeAdapter::new()),
        Box::new(OpenClawAdapter::new()),
        Box::new(OpenCodeAdapter::new()),
        Box::new(CodeBuddyAdapter::new()),
        Box::new(ClineAdapter::new()),
        Box::new(AntigravityAdapter::new()),
        Box::new(OpenHermesAdapter::new()),
        Box::new(CursorAdapter::new()),
    ]
}

/// ledger 实现表/ deferred 表的 provider_id 列。
fn beta_ledger_ids(section: &str) -> Vec<String> {
    beta_ledger_rows(section)
        .into_iter()
        .map(|row| row.into_iter().next().unwrap_or_default())
        .collect()
}

#[test]
fn beta_readiness_manifests_match_ledger_evidence_columns() {
    // ledger 的本地列（golden / local Beta blockers）与全局 blocker 必须与真实
    // adapter manifest 一致：golden fixture revision 1、无跨平台认证 target、
    // maturity=Experimental、known_limitations 非空。manifest() 经由 manifest_for
    // 携带各 adapter 自己的声明，因此空限制声明/伪造 revision 会在此失败。
    let ledger_rows = beta_ledger_rows("Per-provider local readiness");
    let ledger_implemented: Vec<&str> = ledger_rows.iter().map(|row| row[0].as_str()).collect();
    assert_eq!(
        ledger_implemented.len(),
        14,
        "ledger 实现表应恰有 14 行，实际 {} 行",
        ledger_implemented.len()
    );

    let adapters = implemented_adapters();
    assert_eq!(adapters.len(), 14, "应恰好实例化 14 个已实现 adapter");
    for adapter in &adapters {
        let manifest = adapter.manifest();
        let id = manifest.provider_id.as_str();
        assert_eq!(
            manifest.fixture_revision,
            Some(1),
            "{id}: golden fixture revision 必须为 Some(1)"
        );
        assert!(
            manifest.last_certified_targets.is_empty(),
            "{id}: 无跨平台认证 CI run 时 last_certified_targets 必须为空"
        );
        assert_eq!(
            manifest.maturity,
            ProviderMaturity::Experimental,
            "{id}: owner 晋级决策前必须保持 Experimental"
        );
        assert!(
            !manifest.known_limitations.is_empty(),
            "{id}: 必须声明非空 known_limitations"
        );
        assert!(
            ledger_implemented.contains(&id),
            "{id}: ledger 实现表缺少该 provider 行"
        );
    }
}

#[test]
fn beta_readiness_ledger_lists_exactly_implemented_and_deferred_ids() {
    // 文档若增删 provider 行或拼错 provider_id 而未同步 capability.rs，立即失败。
    let matrix = ProviderCapabilityMatrix::current();
    let mut implemented: Vec<String> = matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
        .map(|p| p.provider_id.clone())
        .collect();
    let mut deferred: Vec<String> = matrix
        .providers
        .iter()
        .filter(|p| p.maturity == ProviderMaturity::Unsupported)
        .map(|p| p.provider_id.clone())
        .collect();
    implemented.sort_unstable();
    deferred.sort_unstable();

    let mut ledger_implemented = beta_ledger_ids("Per-provider local readiness");
    ledger_implemented.sort_unstable();
    assert_eq!(
        ledger_implemented, implemented,
        "ledger 实现表必须与 capability.rs 的 14 个已实现 provider 逐一一致"
    );

    let mut ledger_deferred = beta_ledger_ids("Deferred providers");
    ledger_deferred.sort_unstable();
    assert_eq!(
        ledger_deferred, deferred,
        "ledger deferred 表必须与 capability.rs 的 2 个 deferred provider 逐一一致"
    );
}

// ---- capability.rs ↔ adapter 真实行为 防漂移 ----
//
// 此前 `resume` 列有 `application::resume` 里的
// `capability_matrix_resume_level_matches_builder_support` 守护（枚举全部 provider
// 双向断言），而 `source_span` / `tool_activity` 两列只有"文档 ↔ capability.rs"的
// 一致性测试，没有任何测试把声明对照 adapter 的真实输出。这个缺口已漏过两个真实
// 缺陷：aider 虚报 `tool_activity: Partial`（crate 内 0 个 `emit_activity`），
// antigravity 少报 `source_span: Unsupported`（实际发出经 golden 校验的字节 span）。
// 下面三个测试把这条关系补成可执行断言。

/// 每个已实现 provider 的 pinned golden 期望输出（编译期内联，不依赖运行时路径）。
///
/// 这些文件是各 adapter 真实 parse 结果的 pinned 投影（`golden::canonical_json`
/// 逐字段写出 native_id / span），因此可以当作"adapter 实际发出了什么"的可复核
/// 证据，用来交叉验证 capability.rs 的声明。
const PINNED_GOLDEN: &[(&str, &str)] = &[
    (
        "aider",
        include_str!("../../agent-session-grep-provider-aider/tests/golden/basic.expected.json"),
    ),
    (
        "antigravity",
        include_str!(
            "../../agent-session-grep-provider-antigravity/tests/golden/basic.expected.json"
        ),
    ),
    (
        "claude-code",
        include_str!("../../agent-session-grep-provider-claude/tests/golden/basic.expected.json"),
    ),
    (
        "cline",
        include_str!("../../agent-session-grep-provider-cline/tests/golden/basic.expected.json"),
    ),
    (
        "codex",
        include_str!("../../agent-session-grep-provider-codex/tests/golden/basic.expected.json"),
    ),
    (
        "cursor",
        include_str!("../../agent-session-grep-provider-cursor/tests/golden/basic.expected.json"),
    ),
    (
        "grok-build",
        include_str!("../../agent-session-grep-provider-grok/tests/golden/basic.expected.json"),
    ),
    (
        "hermes",
        include_str!("../../agent-session-grep-provider-hermes/tests/golden/basic.expected.json"),
    ),
    (
        "kimi-code",
        include_str!("../../agent-session-grep-provider-kimi/tests/golden/basic.expected.json"),
    ),
    (
        "openclaw",
        include_str!("../../agent-session-grep-provider-openclaw/tests/golden/basic.expected.json"),
    ),
    (
        "opencode",
        include_str!("../../agent-session-grep-provider-opencode/tests/golden/basic.expected.json"),
    ),
    (
        "pi",
        include_str!("../../agent-session-grep-provider-pi/tests/golden/basic.expected.json"),
    ),
    (
        "qoder",
        include_str!("../../agent-session-grep-provider-qoder/tests/golden/basic.expected.json"),
    ),
    (
        "tencent-codebuddy",
        include_str!(
            "../../agent-session-grep-provider-codebuddy/tests/golden/basic.expected.json"
        ),
    ),
];

/// Claude Code 的 golden fixture 字节（唯一一个真正携带 tool_use/tool_result 的
/// fixture；codex 的 fixture 只有 session_meta + message，不触发 activity）。
const CLAUDE_GOLDEN_FIXTURE: &[u8] =
    include_bytes!("../../agent-session-grep-provider-claude/tests/golden/basic.jsonl");

/// 取某 provider 的 pinned golden 消息数组。
fn pinned_messages(provider_id: &str) -> Vec<serde_json::Value> {
    let raw = PINNED_GOLDEN
        .iter()
        .find(|(id, _)| *id == provider_id)
        .unwrap_or_else(|| panic!("PINNED_GOLDEN 缺少 provider `{provider_id}`"))
        .1;
    let doc: serde_json::Value =
        serde_json::from_str(raw).expect("basic.expected.json must be valid JSON");
    doc["messages"]
        .as_array()
        .unwrap_or_else(|| panic!("{provider_id}: expected.json 缺少 messages 数组"))
        .clone()
}

#[test]
fn pinned_golden_table_covers_exactly_the_implemented_providers() {
    // 新增/删除 provider 而未同步本表时立即失败，避免下面两个断言静默漏检。
    let matrix = ProviderCapabilityMatrix::current();
    let mut implemented: Vec<String> = matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
        .map(|p| p.provider_id.clone())
        .collect();
    let mut covered: Vec<String> = PINNED_GOLDEN
        .iter()
        .map(|(id, _)| (*id).to_string())
        .collect();
    implemented.sort_unstable();
    covered.sort_unstable();
    assert_eq!(
        covered, implemented,
        "PINNED_GOLDEN 必须恰好覆盖 capability.rs 的 14 个已实现 provider"
    );
}

#[test]
fn capability_source_span_claim_matches_pinned_golden_span_presence() {
    // `source_span` 声明必须与 adapter 真实发出的 span 一致：声明可归因
    // （Native/Derived/Partial）则 pinned 输出里每条消息都必须带 span；声明
    // Unsupported 则每条消息的 span 必须为 null。antigravity 少报就是被这条抓住的
    // 那类缺陷（它的 golden 里 4 条消息全部带经字节校验的 span，却声明 Unsupported）。
    let matrix = ProviderCapabilityMatrix::current();
    for cap in matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
    {
        let messages = pinned_messages(&cap.provider_id);
        assert!(
            !messages.is_empty(),
            "{}: golden fixture 必须至少发出一条消息",
            cap.provider_id
        );
        let with_span = messages.iter().filter(|m| !m["span"].is_null()).count();
        let total = messages.len();
        match cap.source_span {
            CapabilityLevel::Native | CapabilityLevel::Derived | CapabilityLevel::Partial => {
                assert_eq!(
                    with_span, total,
                    "{}: capability.rs 声明 source_span={:?}，但 pinned golden 里只有 {with_span}/{total} 条消息带 span",
                    cap.provider_id, cap.source_span
                );
            }
            CapabilityLevel::Unsupported => {
                assert_eq!(
                    with_span, 0,
                    "{}: capability.rs 声明 source_span=Unsupported，但 pinned golden 里有 {with_span}/{total} 条消息带 span（少报）",
                    cap.provider_id
                );
            }
            CapabilityLevel::Unknown => panic!(
                "{}: 已实现 provider 的 source_span 不得为 Unknown",
                cap.provider_id
            ),
        }
    }
}

#[test]
fn capability_tool_activity_claim_respects_fail_closed_anchoring() {
    // 活动锚定是 fail-closed 的：CLI staging 会丢弃 `message_native_id` 为空的活动
    // （否则会错挂到第一条同样空 id 的消息上）。因此"pinned golden 里所有消息的
    // native_id 都为空"的 provider 结构上不可能支持 tool_activity——声明必须是
    // Unsupported。aider 虚报 `Partial` 正是违反了这条（它 crate 内 0 个
    // `emit_activity`，且 native_id 恒为空）。
    let matrix = ProviderCapabilityMatrix::current();
    for cap in matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
    {
        let messages = pinned_messages(&cap.provider_id);
        let anchorable = messages
            .iter()
            .filter(|m| !m["native_id"].as_str().unwrap_or("").trim().is_empty())
            .count();
        if anchorable == 0 {
            assert_eq!(
                cap.tool_activity,
                CapabilityLevel::Unsupported,
                "{}: pinned golden 里所有消息 native_id 均为空，活动无法锚定（会被 staging 丢弃），\
                 tool_activity 只能声明 Unsupported，实际声明 {:?}",
                cap.provider_id,
                cap.tool_activity
            );
        }
    }
}

#[test]
fn claude_tool_activity_claim_is_backed_by_observed_emissions() {
    // 正向验证：claude-code 声明 tool_activity 可用，其 golden fixture 必须真的
    // 经 `emit_activity` 发出活动，且每条活动都带可锚定（非空）的 native id。
    // 这是唯一一个 fixture 覆盖 tool_use/tool_result 的 provider——codex 同样声明
    // Partial，但其 fixture 只有 session_meta + message，故只能靠上面的锚定断言守护。
    let matrix = ProviderCapabilityMatrix::current();
    let claude_cap = matrix
        .providers
        .iter()
        .find(|p| p.provider_id == "claude-code")
        .expect("capability.rs 必须有 claude-code 行");
    assert_ne!(
        claude_cap.tool_activity,
        CapabilityLevel::Unsupported,
        "claude-code 的 golden fixture 会发出 activity，声明不得为 Unsupported"
    );

    let (_, sink) = agent_session_grep_testkit::golden::parse_golden(
        &ClaudeCodeAdapter::new(),
        CLAUDE_GOLDEN_FIXTURE,
    );
    assert!(
        !sink.activities.is_empty(),
        "claude-code golden fixture 必须至少发出一条 tool activity"
    );
    for activity in &sink.activities {
        assert!(
            !activity.message_native_id.trim().is_empty(),
            "claude-code 发出的活动必须带非空锚点 native id，否则会被 staging 丢弃"
        );
    }
}

#[test]
fn beta_readiness_ledger_capability_columns_match_capability_matrix() {
    // ledger 本地能力列（source_span/tool_activity/resume/incremental）必须与
    // capability.rs 权威行一致；golden/read-only 列由 manifest 测试覆盖。
    let matrix = ProviderCapabilityMatrix::current();
    let rows = beta_ledger_rows("Per-provider local readiness");
    assert_eq!(rows.len(), 14, "ledger 实现表应恰有 14 行");

    // 列序：provider_id | golden | read-only | source_span | tool_activity
    // | resume | incremental | local Beta blockers。
    for cap in matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
    {
        let row = rows
            .iter()
            .find(|row| row[0] == cap.provider_id)
            .unwrap_or_else(|| panic!("ledger 缺少 provider `{}` 的行", cap.provider_id));
        assert_eq!(
            row.len(),
            8,
            "{}: ledger 行应恰有 8 列，实际 {} 列",
            cap.provider_id,
            row.len()
        );
        for (column, column_name, expected) in [
            (3usize, "source_span", cap.source_span),
            (4usize, "tool_activity", cap.tool_activity),
            (5usize, "resume", cap.resume),
            (6usize, "incremental", cap.incremental),
        ] {
            let actual = beta_ledger_cell_to_level(&row[column]).unwrap_or_else(|| {
                panic!(
                    "{}: ledger `{}` 列取值 `{}` 无法映射到 CapabilityLevel",
                    cap.provider_id, column_name, row[column]
                )
            });
            assert_eq!(
                actual, expected,
                "{}: ledger `{}` 列与 capability.rs 漂移（ledger=`{}`，capability=`{:?}`）",
                cap.provider_id, column_name, row[column], expected
            );
        }
    }
}
