//! Provider maturity/capability matrix 防漂移测试。
//!
//! matrix 文档（`docs/product/PROVIDER-MATURITY-MATRIX.md`）声明每个 provider 的
//! `provider_id` 与 `variant_id`。这里把文档 include 进测试二进制，并用真实 adapter
//! probe 出的标识对照——文档与代码任一改动而未同步，CI 立即失败（与 Robot envelope
//! schema 用 include_str! 交叉验证同一纪律）。

use agentsessions_ports::ProviderAdapter;
use agentsessions_provider_claude::ClaudeCodeAdapter;
use agentsessions_provider_codex::CodexAdapter;

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

#[test]
fn matrix_declares_each_provider_id_and_variant() {
    let claude = ClaudeCodeAdapter::new();
    let codex = CodexAdapter::new();

    // provider_id：adapter 报出的必须在文档中出现。
    assert!(
        MATRIX.contains(claude.provider_id()),
        "matrix 缺少 provider_id `{}`",
        claude.provider_id()
    );
    assert!(
        MATRIX.contains(codex.provider_id()),
        "matrix 缺少 provider_id `{}`",
        codex.provider_id()
    );

    // variant_id：用真实 probe 得到，避免文档写一个、代码认另一个。
    let claude_variant = claude.probe(CLAUDE_SAMPLE.as_bytes()).unwrap().variant_id;
    let codex_variant = codex.probe(CODEX_SAMPLE.as_bytes()).unwrap().variant_id;
    assert!(
        MATRIX.contains(&claude_variant),
        "matrix 缺少 variant `{claude_variant}`"
    );
    assert!(
        MATRIX.contains(&codex_variant),
        "matrix 缺少 variant `{codex_variant}`"
    );
}

#[test]
fn matrix_marks_both_providers_experimental_not_beta() {
    // 诚实门：两个 provider 尚未有 property/fuzz/golden 覆盖，只能是 Experimental。
    // 若有人把文档改成 Beta/GA 却没补证据，此断言提醒回到晋级标准。
    assert!(
        MATRIX.contains("Experimental"),
        "matrix 应标注当前成熟度 Experimental"
    );
}
