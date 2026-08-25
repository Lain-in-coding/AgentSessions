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

use agent_session_grep_domain::TokenSource;
use agent_session_grep_ports::Confidence;
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
fn capability_tool_activity_unsupported_claim_is_not_an_under_claim() {
    // 上面那条只堵虚报（锚不住却声称支持）。少报方向此前没有守护：某个 adapter
    // 真的开始 `emit_activity` 了，而 capability.rs 仍写 Unsupported，不会有任何
    // 测试失败——这正是 handoff 与 incremental 两次少报的成因（一致性守护只保证
    // 文档与代码互相同意，不保证同意的值为真）。
    //
    // 这里对每个声明 Unsupported 的已实现 provider 跑真实 parse，若它对自己的
    // golden 实际发出了可锚定的活动，则声明为假。
    let matrix = ProviderCapabilityMatrix::current();

    for (adapter, fixture) in &implemented_adapters_with_fixtures() {
        let provider_id = adapter.provider_id();
        let cap = matrix
            .providers
            .iter()
            .find(|p| p.provider_id == provider_id)
            .unwrap_or_else(|| panic!("capability.rs 缺少 provider `{provider_id}`"));
        if cap.tool_activity != CapabilityLevel::Unsupported {
            continue;
        }

        let mut sink = agent_session_grep_testkit::golden::CapturingSink::default();
        let _ = adapter.parse(fixture, &mut sink);
        let anchored = sink
            .activities
            .iter()
            .filter(|a| !a.message_native_id.trim().is_empty())
            .count();
        assert_eq!(
            anchored, 0,
            "{provider_id}: capability.rs 声明 tool_activity=Unsupported，但对自己的 golden \
             实测发出了 {anchored} 条可锚定活动（少报）——声明必须升到 Partial/Native"
        );
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
fn capability_context_claim_matches_pinned_golden_parent_links() {
    // `context` 声明的是"能否重建会话上下文图"。图的边只有一个来源：adapter 发出的
    // `parent_native_id`（sqlite 的 `message_edges` 由它写入，`context_mainline` 沿这些
    // 边逐级 `resolve_parent` 回溯）。代码里没有任何按序号合成边的回退路径，因此
    // "pinned golden 里没有任何一条消息带 parent_native_id" 的 provider 结构上无法
    // 重建图——声明必须是 Unsupported；反之声明可用就必须真有边。
    //
    // 这条与 `capability_source_span_claim_matches_pinned_golden_span_presence` 是同一
    // 纪律的第三条：能力列不能靠人工填，必须由真实 adapter 输出反证。
    let matrix = ProviderCapabilityMatrix::current();
    for cap in matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
    {
        let messages = pinned_messages(&cap.provider_id);
        let with_parent = messages
            .iter()
            .filter(|m| {
                !m["parent_native_id"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            })
            .count();
        match cap.context {
            CapabilityLevel::Native | CapabilityLevel::Derived | CapabilityLevel::Partial => {
                assert!(
                    with_parent > 0,
                    "{}: capability.rs 声明 context={:?}，但 pinned golden 里没有任何消息带 \
                     parent_native_id——没有边就没有上下文图（虚报）",
                    cap.provider_id,
                    cap.context
                );
            }
            CapabilityLevel::Unsupported => {
                assert_eq!(
                    with_parent, 0,
                    "{}: capability.rs 声明 context=Unsupported，但 pinned golden 里有 \
                     {with_parent} 条消息带 parent_native_id（少报，图其实可建）",
                    cap.provider_id
                );
            }
            CapabilityLevel::Unknown => panic!(
                "{}: 已实现 provider 的 context 不得为 Unknown",
                cap.provider_id
            ),
        }
    }
}

/// 一段最小 Codex rollout：assistant 消息（提供锚点）+ 配对的
/// `custom_tool_call` / `function_call_output`。codex 的 golden fixture 只有
/// `session_meta` + `message`，不触发 activity，故这里用测试局部输入驱动——它
/// 不是 golden fixture，无需 `fixture_revision` 递增。
const CODEX_TOOL_ROLLOUT: &str = concat!(
    r#"{"timestamp":"2026-07-26T08:00:00.000Z","type":"session_meta","payload":{"session_id":"0198aaaa-bbbb-7ccc-8ddd-eeeeffff0002"}}"#,
    "\n",
    r#"{"timestamp":"2026-07-26T08:00:01.000Z","type":"response_item","payload":{"type":"message","id":"msg-tool-anchor","role":"assistant","content":[{"type":"output_text","text":"checking the sandbox"}]}}"#,
    "\n",
    r#"{"timestamp":"2026-07-26T08:00:02.000Z","type":"response_item","payload":{"type":"custom_tool_call","id":"call_probe","tool_call_id":"call_probe","name":"shell","arguments":"{\"command\":\"cat config.toml\"}"}}"#,
    "\n",
    r#"{"timestamp":"2026-07-26T08:00:03.000Z","type":"response_item","payload":{"type":"function_call_output","id":"fco_probe","call_id":"call_probe","output":"policy = \"safe\"","is_error":false}}"#,
    "\n",
);

#[test]
fn codex_tool_activity_claim_is_backed_by_observed_emissions() {
    // 正向验证（与 claude-code 对称）：codex 声明 tool_activity 可用，其 adapter
    // 必须真的经 `emit_activity` 发出活动，且活动锚点非空（否则被 staging 丢弃）。
    // codex 的 golden fixture 不含工具记录，若无本断言，删掉 adapter 的
    // `custom_tool_call` / `function_call_output` 配对逻辑不会有任何测试失败在
    // capability 层面报警——这正是 aider 虚报能存活的同一类缺口。
    let matrix = ProviderCapabilityMatrix::current();
    let codex_cap = matrix
        .providers
        .iter()
        .find(|p| p.provider_id == "codex")
        .expect("capability.rs 必须有 codex 行");
    assert_ne!(
        codex_cap.tool_activity,
        CapabilityLevel::Unsupported,
        "codex adapter 会配对发出 activity，声明不得为 Unsupported"
    );

    let (_, sink) = agent_session_grep_testkit::golden::parse_golden(
        &CodexAdapter::new(),
        CODEX_TOOL_ROLLOUT.as_bytes(),
    );
    assert_eq!(
        sink.activities.len(),
        1,
        "配对的 custom_tool_call/function_call_output 必须恰好产出一条 activity"
    );
    let activity = &sink.activities[0];
    assert!(
        !activity.message_native_id.trim().is_empty(),
        "codex 发出的活动必须带非空锚点 native id，否则会被 staging 丢弃"
    );
    assert_eq!(
        activity.message_native_id, "msg-tool-anchor",
        "活动必须锚定到发出调用前最近 emit 的助理消息"
    );
}

// ---- capability.rs `usage` 列 ↔ adapter 真实行为 防漂移 ----
//
// `usage` 是 v15 新增列：claude-code 声明 Native（assistant 记录自带
// `message.usage`），codex 声明 Derived（token_count 累计量转增量）。同
// source_span/tool_activity 的纪律：声明必须由 adapter 真实 emit 反证，
// 两个方向都要堵——虚报（声称提取却没 emit）与少报（emit 了却写 Unsupported）。

/// 合成 Claude Code usage 输入（测试局部输入，非 golden fixture，无需
/// fixture_revision 递增）：assistant 记录带 `message.usage` 四桶 + uuid 锚点。
const CLAUDE_USAGE_TRANSCRIPT: &str = concat!(
    r#"{"type":"user","uuid":"u-1","sessionId":"sess-usage","message":{"role":"user","content":"hi"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"a-1","parentUuid":"u-1","sessionId":"sess-usage","message":{"role":"assistant","content":[{"type":"text","text":"answer"}],"usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":30,"cache_creation_input_tokens":20}}}"#,
    "\n",
);

/// 合成 Codex usage 输入：两条 token_count 累计事件（第二条对第一条取增量）。
const CODEX_USAGE_ROLLOUT: &str = concat!(
    r#"{"timestamp":"t1","type":"session_meta","payload":{"session_id":"sess-usage-codex"}}"#,
    "\n",
    r#"{"timestamp":"t2","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"cached_input_tokens":2,"output_tokens":3,"reasoning_output_tokens":1}}}}"#,
    "\n",
    r#"{"timestamp":"t3","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":15,"cached_input_tokens":3,"output_tokens":5,"reasoning_output_tokens":1}}}}"#,
    "\n",
);

#[test]
fn claude_usage_claim_is_backed_by_observed_emissions() {
    // 正向验证：claude-code 声明 usage=Native，其 adapter 必须真的经
    // `emit_usage` 发出 Observed 事件，且锚点非空（否则被 staging 丢弃）。
    let matrix = ProviderCapabilityMatrix::current();
    let cap = matrix
        .providers
        .iter()
        .find(|p| p.provider_id == "claude-code")
        .expect("capability.rs 必须有 claude-code 行");
    assert_eq!(cap.usage, CapabilityLevel::Native);

    let (_, sink) = agent_session_grep_testkit::golden::parse_golden(
        &ClaudeCodeAdapter::new(),
        CLAUDE_USAGE_TRANSCRIPT.as_bytes(),
    );
    assert_eq!(
        sink.usages.len(),
        1,
        "带 message.usage 的 assistant 记录必须产出一条 usage"
    );
    let usage = &sink.usages[0];
    assert_eq!(usage.message_native_id, "a-1", "usage 必须锚定该记录 uuid");
    assert_eq!(usage.usage.token_source, TokenSource::Observed);
    assert_eq!(usage.usage.input_tokens, 100);
    assert_eq!(usage.usage.output_tokens, 50);
    assert_eq!(usage.usage.cache_read_tokens, 30);
    assert_eq!(usage.usage.cache_write_tokens, 20);
}

#[test]
fn codex_usage_claim_is_backed_by_derived_emissions() {
    // 正向验证：codex 声明 usage=Derived，其 adapter 必须真的从 token_count
    // 累计量派生增量事件（session 级：锚点为空串是契约，不是缺陷）。
    let matrix = ProviderCapabilityMatrix::current();
    let cap = matrix
        .providers
        .iter()
        .find(|p| p.provider_id == "codex")
        .expect("capability.rs 必须有 codex 行");
    assert_eq!(cap.usage, CapabilityLevel::Derived);

    let (_, sink) = agent_session_grep_testkit::golden::parse_golden(
        &CodexAdapter::new(),
        CODEX_USAGE_ROLLOUT.as_bytes(),
    );
    assert_eq!(sink.usages.len(), 2, "两条累计事件必须派生两条增量");
    for usage in &sink.usages {
        assert_eq!(
            usage.message_native_id, "",
            "token_count 不关联具体消息，锚点必须为空串（session 级观察）"
        );
        assert_eq!(usage.usage.token_source, TokenSource::Derived);
    }
    // 首条：total 本身（input 10 − cached 2 = 8）；第二条：单调差（5 − 1 = 4）。
    assert_eq!(sink.usages[0].usage.input_tokens, 8);
    assert_eq!(sink.usages[0].usage.cache_read_tokens, 2);
    assert_eq!(sink.usages[0].usage.output_tokens, 3);
    assert_eq!(sink.usages[1].usage.input_tokens, 4);
    assert_eq!(sink.usages[1].usage.cache_read_tokens, 1);
    assert_eq!(sink.usages[1].usage.output_tokens, 2);
}

#[test]
fn capability_usage_unsupported_claim_is_not_an_under_claim() {
    // 少报方向守护：声明 Unsupported 的已实现 provider 对自己的 golden fixture
    // 实测不得发出任何 usage 事件——adapter 若开始提取 usage 而不升级声明，
    // 这里会立即失败（与 tool_activity 少报守护同一纪律）。
    let matrix = ProviderCapabilityMatrix::current();

    for (adapter, fixture) in &implemented_adapters_with_fixtures() {
        let provider_id = adapter.provider_id();
        let cap = matrix
            .providers
            .iter()
            .find(|p| p.provider_id == provider_id)
            .unwrap_or_else(|| panic!("capability.rs 缺少 provider `{provider_id}`"));
        if cap.usage != CapabilityLevel::Unsupported {
            continue;
        }
        let mut sink = agent_session_grep_testkit::golden::CapturingSink::default();
        let _ = adapter.parse(fixture, &mut sink);
        assert_eq!(
            sink.usages.len(),
            0,
            "{provider_id}: capability.rs 声明 usage=Unsupported，但对自己的 golden \
             实测发出了 {} 条 usage 事件（少报）——声明必须升级",
            sink.usages.len()
        );
    }
}

#[test]
fn beta_readiness_ledger_capability_columns_match_capability_matrix() {
    // ledger 本地能力列（discover/source_span/tool_activity/resume/incremental）
    // 必须与 capability.rs 权威行一致；golden/read-only 列由 manifest 测试覆盖。
    let matrix = ProviderCapabilityMatrix::current();
    let rows = beta_ledger_rows("Per-provider local readiness");
    assert_eq!(rows.len(), 14, "ledger 实现表应恰有 14 行");

    // 列序：provider_id | golden | read-only | property | discover | source_span
    // | tool_activity | resume | incremental | local Beta blockers。
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
            10,
            "{}: ledger 行应恰有 10 列，实际 {} 列",
            cap.provider_id,
            row.len()
        );
        for (column, column_name, expected) in [
            (4usize, "discover", cap.discover),
            (5usize, "source_span", cap.source_span),
            (6usize, "tool_activity", cap.tool_activity),
            (7usize, "resume", cap.resume),
            (8usize, "incremental", cap.incremental),
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

/// ledger 的 provider_id → crate 目录名映射：三个 provider 的 crate 目录与
/// provider_id 不同（历史命名），其余同名。
fn provider_crate_dir(provider_id: &str) -> &str {
    match provider_id {
        "claude-code" => "claude",
        "grok-build" => "grok",
        "kimi-code" => "kimi",
        "tencent-codebuddy" => "codebuddy",
        other => other,
    }
}

#[test]
fn beta_readiness_property_column_matches_properties_test_existence() {
    // ledger 的 `property` 列声明"seeded 随机化 property 套件"
    // （`crates/agent-session-grep-provider-<dir>/tests/properties.rs`）是否存在。
    // 与 golden/read-only 两列同一纪律：证据列不得人工填写而不被反证——
    // 声明 ok 却没有文件 = 虚报覆盖；文件存在却声明 missing = 少报。
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("必须能从 crate 目录上溯到仓库根")
        .to_path_buf();
    let rows = beta_ledger_rows("Per-provider local readiness");
    assert_eq!(rows.len(), 14, "ledger 实现表应恰有 14 行");
    let mut checked = 0usize;
    for row in &rows {
        let provider_id = &row[0];
        let cell = &row[3];
        let path = repo_root
            .join("crates")
            .join(format!(
                "agent-session-grep-provider-{}",
                provider_crate_dir(provider_id)
            ))
            .join("tests/properties.rs");
        let exists = path.is_file();
        match cell.as_str() {
            "ok" => assert!(
                exists,
                "{provider_id}: ledger property 列声明 ok 但 `{path:?}` 不存在（虚报覆盖）"
            ),
            "missing" => assert!(
                !exists,
                "{provider_id}: `{path:?}` 已存在但 ledger property 列仍为 missing（少报）"
            ),
            other => panic!("{provider_id}: property 列取值 `{other}` 非法（只能 ok/missing）"),
        }
        checked += 1;
    }
    assert_eq!(checked, 14, "必须逐一核对 14 行的 property 列");
}

// ---- README.md provider 表防漂移 ----

/// README 原文（相对本源文件路径）。README 是访客读到的第一份能力声明，
/// 却是手写表格；这里把它纳入同一条漂移纪律。
const README: &str = include_str!("../../../README.md");

/// 解析 README "## Providers" 小节的表格数据行：`(Provider 名, Status, Format)`。
/// 表头与分隔行跳过；小节之外的表格不收集。
fn readme_provider_rows() -> Vec<(String, String, String)> {
    let mut rows = Vec::new();
    let mut in_section = false;
    for line in README.lines() {
        if line.starts_with("## ") {
            in_section = line.trim() == "## Providers";
            continue;
        }
        if !in_section || !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim())
            .collect();
        if cells.len() < 3 || cells[0] == "Provider" || cells[1].starts_with("---") {
            continue;
        }
        rows.push((
            cells[0].to_string(),
            cells[1].to_string(),
            cells[2].to_string(),
        ));
    }
    rows
}

#[test]
fn readme_provider_table_matches_capability_matrix_maturity() {
    // README 的 Status 列必须与 capability.rs 的 maturity 一致，且行数与
    // 16 行权威矩阵相同。任一侧增删 provider 或提级而未同步 README，立即失败——
    // 对外第一眼看到的表格不能比内部台账更乐观。
    let matrix = ProviderCapabilityMatrix::current();
    let rows = readme_provider_rows();
    assert_eq!(
        rows.len(),
        matrix.providers.len(),
        "README Providers 表应有 {} 行（与 capability.rs 一致），实际 {} 行",
        matrix.providers.len(),
        rows.len()
    );

    // capability.rs 的 provider_id 不是 README 的展示名（README 用 "Claude Code"、
    // "Tencent CodeBuddy" 等人读名称），因此按 maturity 计数比对：README 不得
    // 出现比矩阵更高的等级，也不得少记 deferred 行。
    let matrix_experimental = matrix
        .providers
        .iter()
        .filter(|p| p.maturity == ProviderMaturity::Experimental)
        .count();
    let matrix_deferred = matrix
        .providers
        .iter()
        .filter(|p| p.maturity == ProviderMaturity::Unsupported)
        .count();
    let readme_experimental = rows
        .iter()
        .filter(|(_, status, _)| status == "Experimental")
        .count();
    let readme_deferred = rows
        .iter()
        .filter(|(_, status, _)| status == "Deferred")
        .count();
    assert_eq!(
        readme_experimental, matrix_experimental,
        "README 的 Experimental 行数与 capability.rs 漂移"
    );
    assert_eq!(
        readme_deferred, matrix_deferred,
        "README 的 Deferred 行数与 capability.rs 漂移"
    );

    // 诚实门：只要还没有 provider 晋级，README 不得出现 Beta/GA/Certified 字样。
    let has_promoted = matrix.providers.iter().any(|p| {
        p.maturity != ProviderMaturity::Experimental && p.maturity != ProviderMaturity::Unsupported
    });
    if !has_promoted {
        for (name, status, _) in &rows {
            assert!(
                status == "Experimental" || status == "Deferred",
                "README provider `{name}` 声明为 `{status}`，但 capability.rs 尚无任何晋级 provider"
            );
        }
    }
}

/// README Providers 表里 Format 列描述的源格式家族 → 该家族在
/// `AdapterManifest::streaming_support` 上的权威分类。
///
/// README 的 Format 列是访客判断"我的历史能不能被索引"的唯一依据，却是手写散文。
/// 它不能逐字对照代码，但它声明的**格式家族**是可判定的：`manifest.rs` 的
/// `source_consumption()` 按格式把每个 provider 分为 record-stream（逐行 JSONL）
/// 与 bounded-whole-source（整份 JSON / Markdown / SQLite）。README 说 "SQLite"
/// 而 manifest 分类为 record stream（或反之），两者必有一个是假的。
fn readme_format_is_whole_source(format: &str) -> Option<bool> {
    let lowered = format.to_ascii_lowercase();
    if lowered.starts_with('—') || lowered.starts_with("— ") {
        return None; // deferred provider：无格式声明
    }
    // SQLite 与整份 JSON/Markdown 需要完整源；JSONL 是逐行流式。
    if lowered.contains("sqlite") || lowered.contains("vscdb") {
        return Some(true);
    }
    if lowered.contains("jsonl") {
        return Some(false);
    }
    if lowered.contains("markdown") || lowered.contains("json") {
        return Some(true);
    }
    None
}

#[test]
fn readme_provider_format_column_matches_manifest_source_consumption() {
    // README 的 Status 列已被 `readme_provider_table_matches_capability_matrix_maturity`
    // 守护，Format 列此前没有任何守护——而它比 Status 更容易悄悄失真：adapter 换了
    // 解析路径（例如从 JSONL 改为整份 JSON）时，capability.rs 的 maturity 不变，
    // README 的散文描述也不会有人想起来改。
    //
    // 可判定的部分是格式家族：`AdapterManifest::streaming_support` 由 manifest.rs 的
    // `source_consumption()` 按真实解析方式给出（record stream vs bounded whole
    // source），是代码事实而非文档声明。这里把 README 的 Format 文字归一化成同一个
    // 二分类再比对。
    use agent_session_grep_ports::manifest::StreamingSupport;

    let rows = readme_provider_rows();
    let adapters = implemented_adapters_with_fixtures();
    assert_eq!(
        adapters.len(),
        14,
        "implemented_adapters_with_fixtures 必须覆盖 14 个已实现 provider"
    );

    // README 用人读展示名（"Claude Code"），capability.rs 用 provider_id
    // （"claude-code"）。归一化后按名字对齐，避免再手抄一张映射表。
    let normalize = |name: &str| {
        name.to_ascii_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
    };

    let mut checked = 0usize;
    for (adapter, _) in &adapters {
        let provider_id = adapter.provider_id();
        let manifest = adapter.manifest();
        let expects_whole_source = match manifest.streaming_support {
            StreamingSupport::BoundedWholeSource => true,
            StreamingSupport::RecordStream => false,
        };

        // provider_id 去掉分隔符后必须是某个 README 展示名的前缀或相等
        // （"claude-code"→"claudecode" 对 "Claude Code"→"claudecode"；
        // "codex"→"codex" 对 "Codex CLI"→"codexcli"）。
        let id_key = normalize(provider_id);
        let row = rows.iter().find(|(name, _, _)| {
            let name_key = normalize(name);
            name_key == id_key || name_key.starts_with(&id_key) || id_key.starts_with(&name_key)
        });
        let Some((name, _, format)) = row else {
            panic!("README Providers 表缺少 provider `{provider_id}` 对应行");
        };

        let Some(readme_whole_source) = readme_format_is_whole_source(format) else {
            panic!(
                "README provider `{name}` 的 Format 列 `{format}` 无法归类为 \
                 record-stream 或 whole-source——请让描述明确包含格式家族关键词"
            );
        };
        assert_eq!(
            readme_whole_source, expects_whole_source,
            "README provider `{name}` 的 Format 列 `{format}` 描述的格式家族与 \
             manifest.streaming_support ({:?}) 矛盾——两者必有一个是假的",
            manifest.streaming_support
        );
        checked += 1;
    }
    assert_eq!(
        checked, 14,
        "必须逐一核对 14 个已实现 provider 的 Format 列"
    );
}

/// 每个已实现 provider 的 (adapter, golden fixture 字节)：`probe`/`parse` 两列的
/// 声明必须靠真实 adapter 跑真实 fixture 反证，故这里同时要 adapter 实例与输入字节。
///
/// 与 [`PINNED_GOLDEN`]（expected.json，判定输出形状）和 e2e 的 `GOLDEN_SOURCES`
/// （源路径，判定增量）互补：这一张要的是"喂进 adapter 的字节"。
fn implemented_adapters_with_fixtures() -> Vec<(Box<dyn ProviderAdapter>, &'static [u8])> {
    vec![
        (
            Box::new(ClaudeCodeAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-claude/tests/golden/basic.jsonl")
                .as_slice(),
        ),
        (
            Box::new(AiderAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-aider/tests/golden/basic.md")
                .as_slice(),
        ),
        (
            Box::new(CodexAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-codex/tests/golden/basic.jsonl")
                .as_slice(),
        ),
        (
            Box::new(GrokBuildAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-grok/tests/golden/basic.jsonl")
                .as_slice(),
        ),
        (
            Box::new(PiAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-pi/tests/golden/basic.jsonl")
                .as_slice(),
        ),
        (
            Box::new(QoderAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-qoder/tests/golden/basic.jsonl")
                .as_slice(),
        ),
        (
            Box::new(KimiCodeAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-kimi/tests/golden/basic.jsonl")
                .as_slice(),
        ),
        (
            Box::new(OpenClawAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-openclaw/tests/golden/basic.jsonl")
                .as_slice(),
        ),
        (
            Box::new(OpenCodeAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-opencode/tests/golden/basic.db")
                .as_slice(),
        ),
        (
            Box::new(CodeBuddyAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-codebuddy/tests/golden/basic.jsonl")
                .as_slice(),
        ),
        (
            Box::new(ClineAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-cline/tests/golden/basic.json")
                .as_slice(),
        ),
        (
            Box::new(AntigravityAdapter::new()),
            include_bytes!(
                "../../agent-session-grep-provider-antigravity/tests/golden/basic.jsonl"
            )
            .as_slice(),
        ),
        (
            Box::new(OpenHermesAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-hermes/tests/golden/basic.json")
                .as_slice(),
        ),
        (
            Box::new(CursorAdapter::new()),
            include_bytes!("../../agent-session-grep-provider-cursor/tests/golden/basic.db")
                .as_slice(),
        ),
    ]
}

#[test]
fn capability_probe_claim_matches_real_probe_on_own_golden() {
    // `probe` 列声明的是"能否在字节上判定 variant"。此前它是全 14 行 `Native`
    // 但没有任何测试把该声明对照真实 probe 调用——与 handoff/incremental 少报
    // 同一类缺口：doc↔capability.rs 一致性守护只保证三份文件互相同意，不保证
    // 同意的值为真。
    //
    // 诚实口径：声明可用（Native/Derived/Partial）则 adapter 必须对自己的 golden
    // 字节返回 Ok 且置信度不是 Ambiguous——RFC-0002 §3 规定歧义即拒绝解析，
    // 所以 Ambiguous 等于探测不出来。同时 variant_id 必须与 capability.rs 该行
    // 的 variant_id 逐字相同：probe 报出别的 variant 意味着注册表选型会错挂
    // adapter。声明 Unsupported 则必须真的判不出（Err 或 Ambiguous）。
    let matrix = ProviderCapabilityMatrix::current();
    let pairs = implemented_adapters_with_fixtures();
    assert_eq!(
        pairs.len(),
        14,
        "implemented_adapters_with_fixtures 必须覆盖 14 个已实现 provider"
    );

    for (adapter, fixture) in &pairs {
        let provider_id = adapter.provider_id();
        let cap = matrix
            .providers
            .iter()
            .find(|p| p.provider_id == provider_id)
            .unwrap_or_else(|| panic!("capability.rs 缺少 provider `{provider_id}`"));
        assert_ne!(
            cap.maturity,
            ProviderMaturity::Unsupported,
            "{provider_id}: 有 adapter 实例却记为 deferred"
        );

        let probed = adapter.probe(fixture);
        match cap.probe {
            CapabilityLevel::Native | CapabilityLevel::Derived | CapabilityLevel::Partial => {
                let result = probed.unwrap_or_else(|error| {
                    panic!(
                        "{provider_id}: capability.rs 声明 probe={:?}，但对自己的 golden \
                         字节 probe 失败（虚报）：{error:?}",
                        cap.probe
                    )
                });
                assert_ne!(
                    result.confidence,
                    Confidence::Ambiguous,
                    "{provider_id}: probe 返回 Ambiguous 等于判不出 variant（RFC-0002 §3 \
                     歧义即拒绝解析），与声明 {:?} 矛盾",
                    cap.probe
                );
                assert_eq!(
                    result.variant_id, cap.variant_id,
                    "{provider_id}: probe 报出的 variant_id 与 capability.rs 声明不一致——\
                     注册表选型会按声明挂错 adapter"
                );
                assert!(
                    !result.matched_evidence.is_empty(),
                    "{provider_id}: 判定成立必须给出可诊断的 matched_evidence"
                );
            }
            CapabilityLevel::Unsupported => {
                let judged = probed
                    .as_ref()
                    .is_ok_and(|r| r.confidence != Confidence::Ambiguous);
                assert!(
                    !judged,
                    "{provider_id}: capability.rs 声明 probe=Unsupported，但对自己的 golden \
                     字节实测判出了 variant（少报）：{probed:?}"
                );
            }
            CapabilityLevel::Unknown => {
                panic!("{provider_id}: 已实现 provider 的 probe 不得为 Unknown")
            }
        }
    }
}

#[test]
fn capability_parse_claim_matches_real_parse_on_own_golden() {
    // `parse` 列声明的是"能否把字节流规范化成 canonical 消息"。与上面的 probe
    // 同一纪律：声明可用则 adapter 必须对自己的 golden 真的 parse 成功并至少
    // committed 一条消息，且发出的消息数与 report.committed 自洽——report 说
    // committed=N 却只 emit 了 M 条，会让上层的提交计数与实际入库量脱节。
    // 声明 Unsupported 则必须真的解析不出（Err 或 committed=0）。
    let matrix = ProviderCapabilityMatrix::current();

    for (adapter, fixture) in &implemented_adapters_with_fixtures() {
        let provider_id = adapter.provider_id();
        let cap = matrix
            .providers
            .iter()
            .find(|p| p.provider_id == provider_id)
            .unwrap_or_else(|| panic!("capability.rs 缺少 provider `{provider_id}`"));

        let mut sink = agent_session_grep_testkit::golden::CapturingSink::default();
        let report = adapter.parse(fixture, &mut sink);

        match cap.parse {
            CapabilityLevel::Native | CapabilityLevel::Derived | CapabilityLevel::Partial => {
                let report = report.unwrap_or_else(|error| {
                    panic!(
                        "{provider_id}: capability.rs 声明 parse={:?}，但对自己的 golden \
                         字节 parse 失败（虚报）：{error:?}",
                        cap.parse
                    )
                });
                assert!(
                    report.committed > 0,
                    "{provider_id}: 声明 parse={:?} 却一条都没 committed：{report:?}",
                    cap.parse
                );
                assert_eq!(
                    sink.messages.len(),
                    report.committed,
                    "{provider_id}: report.committed={} 与实际 emit 的消息数 {} 不符——\
                     计数与入库量脱节",
                    report.committed,
                    sink.messages.len()
                );
                // 空正文的消息检索不到（FTS 无 token），committed 却把它算进去，
                // 等于宣称索引了检索不到的内容。
                for message in &sink.messages {
                    assert!(
                        !message.text.trim().is_empty(),
                        "{provider_id}: seq={} 的消息正文为空，committed 计入它等于宣称\
                         索引了检索不到的内容",
                        message.seq
                    );
                }
            }
            CapabilityLevel::Unsupported => {
                let parsed = report.as_ref().is_ok_and(|r| r.committed > 0);
                assert!(
                    !parsed,
                    "{provider_id}: capability.rs 声明 parse=Unsupported，但对自己的 golden \
                     字节实测解析出了消息（少报）：{report:?}"
                );
            }
            CapabilityLevel::Unknown => {
                panic!("{provider_id}: 已实现 provider 的 parse 不得为 Unknown")
            }
        }
    }
}

// ---- 能力列必须都有"声明 ↔ 真实行为"守护（元守护）----

/// `capability.rs` 原文：用来从 `ProviderCapability` 结构体里解析出真实的能力列
/// 集合，而不是在测试里手抄一份列名。
const CAPABILITY_SOURCE: &str = include_str!("../../agent-session-grep-ports/src/capability.rs");

/// 各能力列的守护测试所在源文件原文。守护分散在四个 crate 里（就近于被守护的
/// 行为），故这里逐个 include。
const GUARD_SOURCE_THIS: &str = include_str!("provider_matrix.rs");
const GUARD_SOURCE_E2E: &str = include_str!("e2e.rs");
const GUARD_SOURCE_MAIN: &str = include_str!("../src/main.rs");
const GUARD_SOURCE_RESUME: &str =
    include_str!("../../agent-session-grep-application/src/resume.rs");
const GUARD_SOURCE_HANDOFF: &str =
    include_str!("../../agent-session-grep-application/src/handoff_pack.rs");
const GUARD_SOURCE_MANIFEST: &str = include_str!("../../agent-session-grep-ports/src/manifest.rs");

/// ADR-0010（provider maturity 降级与回滚治理）原文。
const ADR_0010: &str = include_str!("../../../docs/adr/ADR-0010-provider-maturity-rollback.md");

/// ADR-0010 §1 证据清单里引用的实现标识 → 该标识所在源文件原文。
///
/// 治理记录把具体测试/常量名当作"可验证证据"写进表格。若这些名字被改名或删除，
/// ADR 就从证据退化为断言，而且没有任何东西会失败——治理文档最需要的正是这种
/// 不会静默腐坏的保证。
const ADR_0010_CITED_EVIDENCE: &[(&str, &str)] = &[
    (
        "pinned_golden_table_covers_exactly_the_implemented_providers",
        GUARD_SOURCE_THIS,
    ),
    (
        "capability_probe_claim_matches_real_probe_on_own_golden",
        GUARD_SOURCE_THIS,
    ),
    (
        "capability_parse_claim_matches_real_parse_on_own_golden",
        GUARD_SOURCE_THIS,
    ),
    ("CAPABILITY_BEHAVIOR_GUARDS", GUARD_SOURCE_THIS),
    (
        "every_capability_column_has_a_behavior_guard",
        GUARD_SOURCE_THIS,
    ),
    (
        "readme_provider_table_matches_capability_matrix_maturity",
        GUARD_SOURCE_THIS,
    ),
    (
        "beta_readiness_manifests_match_ledger_evidence_columns",
        GUARD_SOURCE_THIS,
    ),
    (
        "provider_output_has_every_current_matrix_row_and_enum_maturity",
        GUARD_SOURCE_MAIN,
    ),
    (
        "implemented_manifests_match_authoritative_capability_rows",
        GUARD_SOURCE_MANIFEST,
    ),
    ("manifest_for", GUARD_SOURCE_MANIFEST),
];

#[test]
fn adr_0010_cited_evidence_exists_in_source_and_is_actually_cited() {
    // 双向对齐：表里的每个名字都必须 (a) 真实存在于所声明的源文件，
    // (b) 真的被 ADR-0010 引用。(a) 防止改名后治理文档静默变成谎言；
    // (b) 防止本表在 ADR 删掉引用后继续声称"文档有这条证据"。
    assert_eq!(
        ADR_0010_CITED_EVIDENCE.len(),
        10,
        "ADR-0010 引用证据表被改动——请同步确认 ADR §1 的引用集合"
    );

    for (name, source) in ADR_0010_CITED_EVIDENCE {
        let defined = source.contains(&format!("fn {name}("))
            || source.contains(&format!("const {name}:"))
            || source.contains(&format!("const {name} "));
        assert!(
            defined,
            "ADR-0010 §1 引用的 `{name}` 在其声明的源文件里没有定义——\
             治理记录的证据链已断裂，必须同时修 ADR 与代码"
        );
        assert!(
            ADR_0010.contains(name),
            "`{name}` 已不再被 ADR-0010 引用，本表必须同步删除该行"
        );
    }
}

/// 每个能力列 → 把该列声明对照真实行为的守护测试名 + 该测试所在文件原文。
///
/// 这张表是"每列都必须有可执行反证"这条纪律的落地：新增能力列时，
/// [`every_capability_column_has_a_behavior_guard`] 会因为列名不在表里而失败，
/// 迫使新列先有守护再上线。
const CAPABILITY_BEHAVIOR_GUARDS: &[(&str, &str, &str)] = &[
    (
        "discover",
        "discover_roots_match_capability_discover_claims",
        GUARD_SOURCE_MAIN,
    ),
    (
        "probe",
        "capability_probe_claim_matches_real_probe_on_own_golden",
        GUARD_SOURCE_THIS,
    ),
    (
        "parse",
        "capability_parse_claim_matches_real_parse_on_own_golden",
        GUARD_SOURCE_THIS,
    ),
    (
        "search",
        "capability_search_claim_matches_real_retrieval_for_every_provider",
        GUARD_SOURCE_E2E,
    ),
    (
        "context",
        "capability_context_claim_matches_pinned_golden_parent_links",
        GUARD_SOURCE_THIS,
    ),
    (
        "resume",
        "capability_matrix_resume_level_matches_builder_support",
        GUARD_SOURCE_RESUME,
    ),
    (
        "handoff",
        "handoff_pack_generation_is_provider_independent",
        GUARD_SOURCE_HANDOFF,
    ),
    (
        "tool_activity",
        "capability_tool_activity_claim_respects_fail_closed_anchoring",
        GUARD_SOURCE_THIS,
    ),
    (
        "usage",
        "claude_usage_claim_is_backed_by_observed_emissions",
        GUARD_SOURCE_THIS,
    ),
    (
        "source_span",
        "capability_source_span_claim_matches_pinned_golden_span_presence",
        GUARD_SOURCE_THIS,
    ),
    (
        "incremental",
        "capability_incremental_claim_matches_real_resync_for_every_provider",
        GUARD_SOURCE_E2E,
    ),
];

/// 从 `capability.rs` 的 `ProviderCapability` 结构体里解析出所有 `CapabilityLevel`
/// 类型的字段名（即能力列）。`provider_id`/`variant_id`/`maturity` 不是能力列，
/// 类型不同故天然排除。
fn capability_column_names() -> Vec<String> {
    let mut names = Vec::new();
    let mut in_struct = false;
    for line in CAPABILITY_SOURCE.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub struct ProviderCapability") {
            in_struct = true;
            continue;
        }
        if in_struct {
            if trimmed == "}" {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("pub ")
                && let Some(name) = rest.strip_suffix(": CapabilityLevel,")
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

#[test]
fn every_capability_column_has_a_behavior_guard() {
    // 这条纪律是从两次真实缺陷里长出来的：`handoff` 与 `incremental` 曾各有
    // 文档↔capability.rs 的一致性守护，却没有任何测试把声明对照真实行为——
    // 一致性守护只保证三份文件互相同意，不保证同意的那个值为真。结果两列都
    // 少报（handoff 全 14 行记 Unsupported，而 `asg handoff` 是已发布命令；
    // incremental 12 行记 Unsupported，而未变化的 resync 实测是 no-op）。
    //
    // 因此：能力列集合从 capability.rs 结构体解析（不手抄），每列都必须在
    // CAPABILITY_BEHAVIOR_GUARDS 里登记一个真实存在的守护测试。新增第 11 列
    // 会在此失败，直到它也有可执行反证。
    let columns = capability_column_names();
    assert!(
        columns.len() >= 10,
        "从 capability.rs 解析出的能力列只有 {} 个，解析逻辑可能失效：{columns:?}",
        columns.len()
    );

    let mut guarded: Vec<&str> = CAPABILITY_BEHAVIOR_GUARDS
        .iter()
        .map(|(column, _, _)| *column)
        .collect();
    guarded.sort_unstable();
    let mut declared: Vec<&str> = columns.iter().map(String::as_str).collect();
    declared.sort_unstable();
    assert_eq!(
        guarded, declared,
        "CAPABILITY_BEHAVIOR_GUARDS 必须恰好覆盖 capability.rs 的能力列——\
         新增列必须同时新增把声明对照真实行为的守护测试"
    );

    // 登记的守护测试必须真实存在于所声明的源文件里（防止改名/删除后表变成谎言）。
    for (column, guard, source) in CAPABILITY_BEHAVIOR_GUARDS {
        let needle = format!("fn {guard}(");
        assert!(
            source.contains(&needle),
            "能力列 `{column}` 登记的守护测试 `{guard}` 在其声明的源文件里找不到——\
             守护被改名或删除后本表即失效"
        );
        assert!(
            source.contains("#[test]"),
            "能力列 `{column}` 的守护源文件必须含测试标注"
        );
    }
}

#[test]
fn readme_implemented_provider_count_matches_capability_matrix() {
    // README 正文写着 "Currently implemented (14/16 planned; 2 deferred ...)"。
    // 这两个数字必须由 capability.rs 推出，而不是手写后忘记更新。
    let matrix = ProviderCapabilityMatrix::current();
    let implemented = matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
        .count();
    let total = matrix.providers.len();
    let deferred = total - implemented;
    let expected =
        format!("Currently implemented ({implemented}/{total} planned; {deferred} deferred");
    assert!(
        README.contains(&expected),
        "README 的实现计数与 capability.rs 漂移，应包含 `{expected}`"
    );
}

/// workspace 根 `Cargo.toml` 原文：README 的版本声明必须与之一致。
const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");

#[test]
fn readme_release_status_and_maturity_tiers_match_authoritative_sources() {
    // README 里剩下两处可腐坏的事实断言：
    //
    // (1) "the first public version is planned as `0.1.0`, matching the Cargo
    //     workspace version" —— 这句话自己声称与 workspace 版本一致，却没有任何
    //     测试核对。发版时 bump 了 workspace 版本而漏改 README，这句话就从事实
    //     变成谎言，而且是访客读到的第 11 行。
    //
    // (2) "providers are graded certified/GA/beta/experimental/unsupported" ——
    //     等级名单必须与 `ProviderMaturity` 的真实变体一致。少一档或多一档都会
    //     让"诚实成熟度"这条宣传语本身不诚实。
    let version = WORKSPACE_MANIFEST
        .lines()
        .skip_while(|line| line.trim() != "[workspace.package]")
        .find_map(|line| line.trim().strip_prefix("version = "))
        .map(|value| value.trim().trim_matches('"').to_string())
        .expect("workspace Cargo.toml 必须在 [workspace.package] 下声明 version");
    // README 是 CRLF 且这句话跨行折行，故先把所有空白折叠成单空格再比对——
    // 断言要盯的是"版本号一致"这件事实，不是折行位置。
    let readme_flat = README.split_whitespace().collect::<Vec<_>>().join(" ");
    let claim = format!("planned as `{version}`, matching the Cargo workspace version");
    assert!(
        readme_flat.contains(&claim),
        "README 的版本声明与 workspace 版本 `{version}` 漂移——\
         这句话自称与 Cargo 版本一致，必须真的一致"
    );

    // 等级名单从 ProviderMaturity 的 as_str() 取真值，而不是在测试里手抄。
    for tier in [
        ProviderMaturity::Certified,
        ProviderMaturity::Ga,
        ProviderMaturity::Beta,
        ProviderMaturity::Experimental,
        ProviderMaturity::Unsupported,
    ] {
        // README 用人读大小写（certified/GA/beta/...），故按大小写不敏感匹配。
        let name = tier.as_str();
        assert!(
            README.to_lowercase().contains(name),
            "README 的成熟度等级名单缺少 `{name}`——\
             ProviderMaturity 有该变体，对外名单不得漏档"
        );
    }
    // 反向：名单不得声称存在 ProviderMaturity 没有的档位。
    for fabricated in ["stable", "production-ready", "verified"] {
        assert!(
            !README.to_lowercase().contains(&format!("/{fabricated}")),
            "README 的等级名单出现 `{fabricated}`，但 ProviderMaturity 无此档位"
        );
    }
}

/// CHANGELOG 原文：`[Unreleased]` 段落逐一点名 provider，与 README 同属对外声明。
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

#[test]
fn changelog_provider_claims_match_capability_matrix() {
    // CHANGELOG 的 `[Unreleased]` 段落写着 "16-provider capability matrix"、
    // "the 14 implemented, Experimental providers" 并逐一点名 provider id，还点名
    // 两个 deferred。这三处都是手写事实：新增或降级一个 provider 而漏改这里，
    // 发版说明就会比 capability.rs 更乐观（或更保守），且没有任何测试会失败。
    //
    // 与 README 的三条守护同一纪律，只是对象换成发版说明——发版说明是外部读者
    // 判断"这个版本支持什么"的第一手材料，不能靠人记得同步。
    let matrix = ProviderCapabilityMatrix::current();
    let total = matrix.providers.len();
    let implemented: Vec<&str> = matrix
        .providers
        .iter()
        .filter(|p| p.maturity != ProviderMaturity::Unsupported)
        .map(|p| p.provider_id.as_str())
        .collect();
    let deferred: Vec<&str> = matrix
        .providers
        .iter()
        .filter(|p| p.maturity == ProviderMaturity::Unsupported)
        .map(|p| p.provider_id.as_str())
        .collect();

    // 只在 [Unreleased] 段落内比对：历史版本段落记录的是当时的事实，不该被
    // 今天的矩阵改写。
    let unreleased: String = CHANGELOG
        .lines()
        .skip_while(|line| !line.starts_with("## [Unreleased]"))
        .skip(1)
        .take_while(|line| !line.starts_with("## ["))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !unreleased.trim().is_empty(),
        "CHANGELOG 必须有非空的 [Unreleased] 段落"
    );

    assert!(
        unreleased.contains(&format!("{total}-provider capability matrix")),
        "CHANGELOG 的 provider 总数与 capability.rs（{total}）漂移"
    );
    assert!(
        unreleased.contains(&format!("the {} implemented", implemented.len())),
        "CHANGELOG 的已实现 provider 数与 capability.rs（{}）漂移",
        implemented.len()
    );

    // 逐个点名：已实现的必须都在，deferred 的必须都被标注为 deferred。
    for id in &implemented {
        assert!(
            unreleased.contains(&format!("`{id}`")),
            "CHANGELOG 的 [Unreleased] 未点名已实现 provider `{id}`"
        );
    }
    for id in &deferred {
        assert!(
            unreleased.contains(&format!("`{id}`")),
            "CHANGELOG 的 [Unreleased] 未点名 deferred provider `{id}`"
        );
    }
    assert!(
        unreleased.contains("deferred"),
        "CHANGELOG 必须说明 deferred provider 的状态"
    );

    // 诚实门：尚无 provider 晋级时，发版说明不得出现 Beta/GA/Certified 口径。
    let has_promoted = matrix.providers.iter().any(|p| {
        p.maturity != ProviderMaturity::Experimental && p.maturity != ProviderMaturity::Unsupported
    });
    if !has_promoted {
        for inflated in ["Beta providers", "GA providers", "certified providers"] {
            assert!(
                !unreleased.contains(inflated),
                "CHANGELOG 出现 `{inflated}`，但 capability.rs 尚无任何晋级 provider"
            );
        }
    }
}

/// 根目录对外文档 → 原文。这些文件是访客与安全研究者的第一手材料，里面用反引号
/// 引用的仓库路径若失效，读者会按错误路径去核对，等于把可核查的证据变成噪音。
const ROOT_DOCS: &[(&str, &str)] = &[
    ("README.md", README),
    ("CHANGELOG.md", CHANGELOG),
    ("SECURITY.md", include_str!("../../../SECURITY.md")),
    ("NOTICE", include_str!("../../../NOTICE")),
    ("CONTRIBUTING.md", include_str!("../../../CONTRIBUTING.md")),
    (
        "CODE_OF_CONDUCT.md",
        include_str!("../../../CODE_OF_CONDUCT.md"),
    ),
];

/// 反引号内容里，哪些看起来像仓库路径但其实是标识符（schema 名、HF 模型 id、
/// 协议方法名、gitignore 条目）。这些不该被当成文件路径核对。
///
/// 判定标准写在这里而不是靠正则猜：白名单是显式的，新增一条必须有理由。
const NON_PATH_IDENTIFIERS: &[&str] = &[
    // Handoff pack schema 名（`handoff-pack/v1`），不是目录。
    "handoff-pack/v1",
    // Hugging Face 模型 id（`intfloat/multilingual-e5-small`）。
    "intfloat/multilingual-e5-small",
    // ACP 协议方法名（`session/update`）。
    "session/update",
    // 被 .gitignore 排除的文件，按契约不应存在于工作树。
    ".mcp.json",
    ".env",
    "credentials*",
    // 发版时生成的产物，不在源码树里。
    "THIRD-PARTY-DEPENDENCIES.json",
    "THIRD-PARTY-DEPENDENCIES.csv",
    // CHANGELOG 用通配符指代 14 个 provider crate 的 properties.rs 族：
    // 任何单一具体路径都无法指代整个集合，而逐条列出会让条目失去可读性。
    "crates/agent-session-grep-provider-*/tests/properties.rs",
];

/// 从文档原文里抽出所有"看起来是仓库路径"的反引号片段。
///
/// 只认包含 `/` 且以已知源码/文档扩展名结尾的片段——这样既不会漏掉真实路径，
/// 也不会把命令行、JSON 字段名之类的反引号内容误判成文件。
fn cited_repo_paths(doc: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut rest = doc;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        let candidate = &after[..close];
        rest = &after[close + 1..];

        if !candidate.contains('/') || candidate.contains(' ') {
            continue;
        }
        let looks_like_file = [
            ".rs", ".py", ".md", ".json", ".yml", ".yaml", ".toml", ".sh", ".ps1",
        ]
        .iter()
        .any(|ext| candidate.ends_with(ext));
        if !looks_like_file {
            continue;
        }
        if NON_PATH_IDENTIFIERS.contains(&candidate) {
            continue;
        }
        paths.push(candidate.to_string());
    }
    paths
}

#[test]
fn root_docs_cited_repo_paths_all_resolve() {
    // 前面几条守护盯的是"文档声明的事实是否为真"。这一条盯的是更基础的一层：
    // 文档指给读者的路径是否还存在。重构搬走一个文件、重命名一个脚本，文档里的
    // 引用就静默失效——读者按路径去核对却找不到，可核查性归零，而且没有任何测试
    // 会失败。NOTICE 的 attribution、SECURITY.md 的 redaction 实现位置都属于这类
    // "必须能被追到源码"的引用。
    //
    // `CARGO_MANIFEST_DIR` 指向 crates/agent-session-grep-cli，故上溯两级到仓库根。
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("必须能从 crate 目录上溯到仓库根")
        .to_path_buf();
    assert!(
        repo_root.join("Cargo.toml").is_file(),
        "仓库根定位失败：{repo_root:?} 下没有 Cargo.toml"
    );

    let mut checked = 0usize;
    for (doc_name, doc) in ROOT_DOCS {
        for cited in cited_repo_paths(doc) {
            // CHANGELOG 里 `tests/network_egress.rs` 这类引用是相对 crate 的写法，
            // 故既接受仓库根下的路径，也接受任一 crate 下的同名路径。
            let direct = repo_root.join(&cited);
            let resolved = direct.exists()
                || std::fs::read_dir(repo_root.join("crates"))
                    .map(|entries| {
                        entries
                            .filter_map(Result::ok)
                            .any(|entry| entry.path().join(&cited).exists())
                    })
                    .unwrap_or(false);
            assert!(
                resolved,
                "{doc_name} 引用的仓库路径 `{cited}` 不存在——\
                 文档指给读者的证据必须真的能被追到；若该文件已搬迁请同步文档，\
                 若它是生成产物或标识符请加入 NON_PATH_IDENTIFIERS 并说明理由"
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 10,
        "只核对到 {checked} 条被引用路径，抽取逻辑可能失效"
    );
}

#[test]
fn every_adapter_leaves_its_source_directory_untouched_through_probe_and_parse() {
    // RFC-0002 §7 与 ADR-0010 §2 触发条件 3 把"只读"列为硬契约：扫描/解析绝不
    // 修改、删除、移动或锁定上游源。既有证据只到 golden 的 `fixture_blake3`，
    // 那钉的是"fixture 没被 git 改坏"，不是"parse 没动过源"。
    //
    // 只断言入参切片没变是同义反复：`probe`/`parse` 都只收 `&[u8]`，类型系统已经
    // 排除了原地改写。真实风险在文件系统——两个 SQLite adapter（opencode/cursor）
    // 会把源拷进临时文件再只读打开，§6 row 11 的临时副本泄漏就出在这条路径上；
    // 若哪天有人改成就地打开源文件、或在源目录旁落下 `-wal`/`-shm`/`.bak`，
    // 现有测试全都不会失败。
    //
    // 因此这里把每个 provider 的 golden 字节写进一个独立临时目录，跑真实
    // probe + parse，再要求该目录的完整快照（文件名 + 全部字节）逐项不变：
    // 源内容不得改写，且不得新增或删除任何兄弟文件。
    let matrix = ProviderCapabilityMatrix::current();
    let mut checked = 0usize;

    /// 目录快照：相对文件名 → 完整字节，递归收集。
    ///
    /// 直接存字节而不是摘要：fixture 都是 KiB 量级，逐字节比较比引入 blake3
    /// dev-dependency 更直接，且失配时能看出是内容变了还是文件增删。
    fn snapshot(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut out = std::collections::BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let key = path
                    .strip_prefix(dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(key, bytes);
            }
        }
        out
    }

    for (adapter, fixture) in &implemented_adapters_with_fixtures() {
        let provider_id = adapter.provider_id();
        assert!(
            matrix
                .providers
                .iter()
                .any(|p| p.provider_id == provider_id),
            "{provider_id}: capability.rs 缺少该 provider"
        );

        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("source.bin");
        std::fs::write(&source, fixture).expect("write source");
        let before = snapshot(dir.path());
        assert_eq!(before.len(), 1, "{provider_id}: 初始目录应只有源文件");

        // 真实调用两条读路径；返回值不参与断言（虚报/少报由别的守护负责），
        // 这里只问一件事：源目录有没有被动过。
        let _ = adapter.probe(fixture);
        let mut sink = agent_session_grep_testkit::golden::CapturingSink::default();
        let _ = adapter.parse(fixture, &mut sink);

        let after = snapshot(dir.path());
        assert_eq!(
            after, before,
            "{provider_id}: probe/parse 之后源目录发生变化——只读契约被破坏\
             （内容改写，或落下了 -wal/-shm/临时副本之类的兄弟文件）"
        );
        checked += 1;
    }

    assert_eq!(checked, 14, "必须逐一核对 14 个已实现 adapter 的只读契约");
}
