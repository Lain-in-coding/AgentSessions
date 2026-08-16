//! Provider maturity/capability matrix 防漂移测试。
//!
//! matrix 文档（`docs/product/PROVIDER-MATURITY-MATRIX.md`）声明每个 provider 的
//! `provider_id` 与 `variant_id`。这里把文档 include 进测试二进制，并用真实 adapter
//! probe 出的标识对照——文档与代码任一改动而未同步，CI 立即失败（与 Robot envelope
//! schema 用 include_str! 交叉验证同一纪律）。

use agent_session_grep_ports::capability::ProviderCapabilityMatrix;
use agent_session_grep_ports::ProviderAdapter;
use agent_session_grep_provider_claude::ClaudeCodeAdapter;
use agent_session_grep_provider_codex::CodexAdapter;

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
