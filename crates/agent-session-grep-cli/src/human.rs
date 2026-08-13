//! Human 渲染器：把成功结果投影为人类可读文本行（无 envelope、无颜色）。
//!
//! 见 `.trellis/tasks/07-26-human-robot-protocol/design.md` §2。
//! search/list/get/show/context/status 各有专属版式，sync 与 ingest 各有统计
//! 版式，其余命令（index/index.rebuild/doctor/config.paths 及未知命令）共用
//! 同一条排序 `key: value` 兜底路径。约束：无颜色、无新依赖；任何输入不
//! panic——缺失或异常字段降级为 `?` 占位或空态措辞；每个返回元素都是单行
//! 可打印文本，无尾随空行。

use crate::protocol::{Outcome, Page};
use serde_json::Value;

/// `list` payload 预览的最大字符数（design §2）。
const LIST_PREVIEW_CHARS: usize = 60;
/// `context` 消息正文预览的最大字符数（design §2）。
const CONTEXT_TEXT_CHARS: usize = 80;
/// `search` 命中正文预览的最大字符数（10 角色体验测试缺陷修复：命中只有
/// UUID+score 时新手无从判断哪条有用）。
const SNIPPET_PREVIEW_CHARS: usize = 120;

/// 把成功结果渲染为人类可读行（无 envelope、无颜色）。
///
/// `data` 是 main.rs `render()` 为各命令构建的 JSON 形状（child 3 冻结）。
/// `outcome` 仅在 `data` 缺失截断元数据时兜底标注 partial；截断与续页
/// 提示以 `data.truncation` 与 `page` 为权威。
pub fn render_success(command: &str, outcome: Outcome, data: &Value, page: &Page) -> Vec<String> {
    match command {
        "search" => {
            let mut lines = render_search(data);
            push_footer(&mut lines, outcome, data, page);
            lines
        }
        "list" => {
            let mut lines = render_list(data);
            push_footer(&mut lines, outcome, data, page);
            lines
        }
        "context" => {
            let mut lines = render_context(data);
            push_footer(&mut lines, outcome, data, page);
            lines
        }
        "get" => render_get(data),
        "show" => render_show(data),
        "status" => render_status(data),
        "sync" => render_sync(data),
        "ingest" => render_ingest(data),
        "config.paths" => {
            // config paths 报告的是"默认位置"，未用到就不会创建；新手照着找会扑空
            // （10 角色体验测试缺陷）。加一句说明，结构本身保持稳定。
            let mut lines = kv_lines(data);
            lines.push("说明：以上是默认位置，未创建过的目录表示尚未使用，属正常。".into());
            lines
        }
        _ => kv_lines(data),
    }
}

/// `search`：头行 `N hit(s) (generation G)` + 每命中 `  <rank>. <id>  score <s>`，
/// 若命中有正文预览（human 渲染前由 CLI 附加）则追加一行缩进的片段；
/// 零命中给措辞 `no hits` 并提示换词。
fn render_search(data: &Value) -> Vec<String> {
    let hits = data
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if hits.is_empty() {
        return vec![
            "no hits".into(),
            "提示：试试更短或更少的关键词（如只搜一个词）。".into(),
        ];
    }
    let mut lines = vec![format!(
        "{} hit(s) (generation {})",
        hits.len(),
        number_text(data, "generation")
    )];
    for (index, hit) in hits.iter().enumerate() {
        let id = hit
            .get("id")
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_else(|| "?".into());
        let score = hit
            .get("score")
            .and_then(Value::as_f64)
            .map(|score| format!("{score:.2}"))
            .unwrap_or_else(|| "?".into());
        lines.push(format!("  {}. {id}  score {score}", index + 1));
        // 正文预览：命中是否有用一瞥即知。取不到 preview 的命中不补行。
        // `snippet` 是 application 装配后接管的字段名，`text` 是旧路径；
        // 两者任一存在即渲染，都不存在则省略该行。
        if let Some(text) = hit
            .get("snippet")
            .or_else(|| hit.get("text"))
            .and_then(Value::as_str)
            .map(|text| preview(text, SNIPPET_PREVIEW_CHARS))
            && !text.is_empty()
        {
            lines.push(format!("     {text}"));
        }
    }
    lines
}

/// `list`：头行 `N entrie(s) (generation G)` + 每条 `  <id>  <payload 预览>`；
/// 零条目给措辞 `catalog is empty`。
fn render_list(data: &Value) -> Vec<String> {
    let entries = data
        .get("entries")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if entries.is_empty() {
        return vec!["catalog is empty".into()];
    }
    let mut lines = vec![format!(
        "{} entries (generation {})",
        entries.len(),
        number_text(data, "generation")
    )];
    for entry in entries {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_else(|| "?".into());
        let payload = entry
            .get("payload")
            .and_then(Value::as_str)
            .map(|payload| preview(payload, LIST_PREVIEW_CHARS))
            .unwrap_or_else(|| "?".into());
        lines.push(format!("  {id}  {payload}"));
    }
    lines
}

/// `get`：payload 字符串原样逐行输出（尾部空白行剔除，保证无尾随空行）；
/// null 或缺失 → `not found`。
fn render_get(data: &Value) -> Vec<String> {
    match data.get("payload") {
        None | Some(Value::Null) => vec!["not found".into()],
        Some(Value::String(payload)) => {
            let mut lines: Vec<String> = payload.lines().map(str::to_string).collect();
            while lines.last().is_some_and(|line| line.trim().is_empty()) {
                lines.pop();
            }
            lines
        }
        Some(other) => vec![value_inline(other)],
    }
}

/// `show`：消息实体（payload 带 `role`/`text`）只展示新手关心的字段
/// （role/text/timestamp/session），内部结构（span/placement/fingerprint）留给
/// json 模式——新手看到整墙内部字段无从下手（10 角色体验测试缺陷）；会话/文档
/// 等容器实体没有这些字段，退回按顶层字段展开的通用 `key: value` 版式：不套
/// 消息投影、不补 `?`、不编造 `context` 提示。null 或缺失 → `not found`。
fn render_show(data: &Value) -> Vec<String> {
    match data.get("entity") {
        None | Some(Value::Null) => vec!["not found".into()],
        Some(entity) if is_message_entity(entity) => {
            let role = entity
                .get("role")
                .and_then(Value::as_str)
                .map(sanitize)
                .unwrap_or_else(|| "?".into());
            let timestamp = entity
                .get("timestamp")
                .and_then(Value::as_str)
                .map(sanitize)
                .unwrap_or_else(|| "?".into());
            let session = entity.get("session").and_then(Value::as_str).map(sanitize);
            let text = entity
                .get("text")
                .and_then(Value::as_str)
                .map(sanitize)
                .unwrap_or_else(|| "?".into());
            let mut lines = vec![
                format!("role: {role}"),
                format!("timestamp: {timestamp}"),
                format!("session: {}", session.as_deref().unwrap_or("?")),
                format!("text: {text}"),
            ];
            // 会话引用未知时 `context <session>` 提示无从执行，不渲染。
            if session.is_some() {
                lines.push("用 context <session> 展开这个会话的完整上下文。".into());
            }
            lines
        }
        Some(entity) => kv_lines(entity),
    }
}

/// 消息实体判定：消息 payload 恒带 `role`（非 JSON 的兜底形状为
/// `role`/`text`）；会话/文档容器 payload 没有这两个字段。
fn is_message_entity(entity: &Value) -> bool {
    entity.get("role").is_some() || entity.get("text").is_some()
}

/// `context`：会话头行 + 编号消息（role/text 取自各消息 payload，缺失 → `?`）
/// + 证据计数行 `evidence: N span(s)`。
fn render_context(data: &Value) -> Vec<String> {
    let session_id = data
        .get("session_id")
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_else(|| "?".into());
    let leaf = data
        .get("branch_leaf")
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_else(|| "none".into());
    let mut lines = vec![format!(
        "session {session_id}  branch leaf {leaf}  (generation {})",
        number_text(data, "generation")
    )];
    let messages = data
        .get("messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for (index, message) in messages.iter().enumerate() {
        let payload = message.get("payload");
        let role = payload
            .and_then(|payload| payload.get("role"))
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_else(|| "?".into());
        // 截断时给出真实消息 wire id，`show <id>` 提示可直接复制执行
        // （10 角色体验测试缺陷：context 静默截断到 80 字符无标记，且提示里
        // `<id>` 只是占位符）。
        let wire_id = message
            .get("message_id")
            .or_else(|| message.get("id"))
            .and_then(Value::as_str)
            .map(sanitize);
        let text = payload
            .and_then(|payload| payload.get("text"))
            .and_then(Value::as_str)
            .map(|text| {
                if text.chars().count() > CONTEXT_TEXT_CHARS {
                    let truncated = preview(text, CONTEXT_TEXT_CHARS);
                    match &wire_id {
                        Some(wire_id) => {
                            format!("{truncated}…(已截断,用 show {wire_id} 看全文)")
                        }
                        None => format!("{truncated}…(已截断)"),
                    }
                } else {
                    preview(text, CONTEXT_TEXT_CHARS)
                }
            })
            .unwrap_or_else(|| "?".into());
        lines.push(format!("  {}. [{role}] {text}", index + 1));
    }
    match data.get("evidence").and_then(Value::as_array) {
        Some(evidence) => lines.push(format!("evidence: {} span(s)", evidence.len())),
        None => lines.push("evidence: ? span(s)".into()),
    }
    lines
}

/// `status`：`entities: N` + `generation: G` 两行。
fn render_status(data: &Value) -> Vec<String> {
    vec![
        format!("entities: {}", number_text(data, "catalog_count")),
        format!("generation: {}", number_text(data, "generation")),
    ]
}

/// `sync`/`ingest`：字段统计 + 一句人话总结。unchanged 是消息条数而非文件数，
/// 新手会把 882 读成文件数而困惑（10 角色体验测试缺陷）；这里显式标注单位，
/// 并给出"新增/无变化"的结论行。
fn render_sync(data: &Value) -> Vec<String> {
    let num = |key: &str| number_text(data, key);
    let mut lines = vec![
        format!("sources: {}（本次扫描的源文件数）", num("sources")),
        format!("emitted: {}（本次新解析的消息条数）", num("emitted")),
        format!(
            "unchanged: {}（未变化的已有消息条数，不是文件数）",
            num("unchanged")
        ),
        format!("skipped: {}（因格式无法入库的消息条数）", num("skipped")),
        format!("generation: {}（当前入库代次）", num("generation")),
    ];
    let unchanged = data.get("unchanged").and_then(Value::as_u64);
    let emitted = data.get("emitted").and_then(Value::as_u64);
    match (emitted, unchanged) {
        (Some(0), Some(_)) => lines.push("总结：没有新增消息（源文件未变化）。".into()),
        (Some(e), Some(_)) => lines.push(format!("总结：新增 {e} 条消息。")),
        _ => {}
    }
    lines
}

/// `ingest`：单文件入库的专属版式。ingest 只解析一个文件，`sources` 恒为 1；
/// 不复用 sync 的统计版式——ingest 响应里没有 `sources` 字段，旧实现会渲染
/// 出 `sources: ?`。只渲染响应实际携带的字段（variant/source_fp/committed/
/// diagnostics），缺失的字段不补 `?` 也不显示。
fn render_ingest(data: &Value) -> Vec<String> {
    let mut lines = vec!["sources: 1（本次扫描的源文件数）".into()];
    if let Some(variant) = data.get("variant").and_then(Value::as_str) {
        lines.push(format!("variant: {}", sanitize(variant)));
    }
    if let Some(source_fp) = data.get("source_fp").and_then(Value::as_str) {
        lines.push(format!("source_fp: {}", sanitize(source_fp)));
    }
    if let Some(committed) = data.get("committed").and_then(Value::as_u64) {
        lines.push(format!("committed: {committed}（本次新入库的消息条数）"));
    }
    if let Some(diagnostics) = data.get("diagnostics").and_then(Value::as_u64) {
        lines.push(format!("diagnostics: {diagnostics}（解析诊断条数）"));
    }
    // 与 sync 一致给结论行；ingest 的"新增"以 committed（实际入库数）为准：
    // 重复 ingest 未变化的文件时 committed 为 0。
    match data.get("committed").and_then(Value::as_u64) {
        Some(0) => lines.push("总结：没有新增消息（源文件未变化）。".into()),
        Some(count) => lines.push(format!("总结：新增 {count} 条消息。")),
        None => {}
    }
    lines
}

/// 截断与续页提示行。`data.truncation` 为权威；元数据缺失但 outcome 已声明
/// partial 时仍以 `?` 占位暴露截断事实。续页行要求 has_more 且携带令牌。
fn push_footer(lines: &mut Vec<String>, outcome: Outcome, data: &Value, page: &Page) {
    let truncation = data.get("truncation");
    let truncated = truncation
        .and_then(|truncation| truncation.get("truncated"))
        .and_then(Value::as_bool);
    if truncated == Some(true) {
        let reason = truncation
            .and_then(|truncation| truncation.get("reason"))
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_else(|| "?".into());
        lines.push(format!("truncated: {reason}"));
    } else if truncated.is_none() && matches!(outcome, Outcome::Partial) {
        lines.push("truncated: ?".into());
    }
    if let Some(token) = page.next_cursor.as_deref().filter(|_| page.has_more) {
        // 新手会把超长 cursor 误判为报错或结果只有一页（10 角色体验测试缺陷）。
        // 明确告知"还有结果"并给出可直接复制的完整命令。
        lines.push(
            "还有更多结果：复制下面这行追加 --cursor 即可翻页（cursor 约 15 分钟有效）".into(),
        );
        lines.push(format!("  --cursor {token}"));
    }
}

/// 兜底路径：对象展开为按 key 排序的 `key: value` 行（嵌套值紧凑 JSON）；
/// 非对象输入降级为单行内联值。所有无专属版式的命令共用此函数。
fn kv_lines(data: &Value) -> Vec<String> {
    match data.as_object() {
        Some(map) => {
            let mut pairs: Vec<(&String, &Value)> = map.iter().collect();
            pairs.sort_by_key(|(key, _)| *key);
            pairs
                .into_iter()
                .map(|(key, value)| format!("{}: {}", sanitize(key), value_inline(value)))
                .collect()
        }
        None => vec![value_inline(data)],
    }
}

/// 值的单行内联展示：字符串裸出（压成单行），其余值一律紧凑 JSON
/// （serde_json 会转义控制字符，天然单行）。
fn value_inline(value: &Value) -> String {
    match value {
        Value::String(text) => sanitize(text),
        other => other.to_string(),
    }
}

/// 数字字段的展示文本；缺失或非数字降级为 `?`。
fn number_text(data: &Value, key: &str) -> String {
    match data.get(key) {
        Some(Value::Number(number)) => number.to_string(),
        _ => "?".into(),
    }
}

/// 单行化预览：控制字符替换为空格，超出 `max` 字符即截断（字符边界安全）。
fn preview(text: &str, max: usize) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect()
}

/// 仅单行化、不截断。
fn sanitize(text: &str) -> String {
    preview(text, usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page_more(token: &str) -> Page {
        Page {
            next_cursor: Some(token.into()),
            has_more: true,
        }
    }

    #[test]
    fn search_renders_ranked_hits_with_generation_header() {
        let data = json!({
            "hits": [
                { "id": "msg_v1_aaaa", "score": 1.234 },
                { "id": "msg_v1_bbbb", "score": 0.5 },
            ],
            "generation": 7,
            "truncation": { "truncated": false, "reason": null },
        });
        let lines = render_success("search", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "2 hit(s) (generation 7)",
                "  1. msg_v1_aaaa  score 1.23",
                "  2. msg_v1_bbbb  score 0.50",
            ]
        );
    }

    #[test]
    fn search_zero_hits_is_worded() {
        let data = json!({
            "hits": [],
            "generation": 1,
            "truncation": { "truncated": false, "reason": null },
        });
        let lines = render_success("search", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            ["no hits", "提示：试试更短或更少的关键词（如只搜一个词）。",]
        );
    }

    #[test]
    fn search_appends_truncation_and_cursor_hint() {
        let data = json!({
            "hits": [{ "id": "msg_v1_aaaa", "score": 2.0 }],
            "generation": 3,
            "truncation": { "truncated": true, "reason": "max_items" },
        });
        let lines = render_success("search", Outcome::Partial, &data, &page_more("tok.abc"));
        assert_eq!(
            lines,
            [
                "1 hit(s) (generation 3)",
                "  1. msg_v1_aaaa  score 2.00",
                "truncated: max_items",
                "还有更多结果：复制下面这行追加 --cursor 即可翻页（cursor 约 15 分钟有效）",
                "  --cursor tok.abc",
            ]
        );
    }

    #[test]
    fn search_renders_hit_text_preview_when_present() {
        // human 渲染前由 CLI 附加 text 字段：命中带上正文预览时显示片段行。
        let data = json!({
            "hits": [{ "id": "msg_v1_aaaa", "score": 2.0, "text": "这是一条关于配置的正文" }],
            "generation": 3,
            "truncation": { "truncated": false, "reason": null },
        });
        let lines = render_success("search", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "1 hit(s) (generation 3)",
                "  1. msg_v1_aaaa  score 2.00",
                "     这是一条关于配置的正文",
            ]
        );
    }

    #[test]
    fn search_renders_snippet_field_when_present() {
        // application 装配的 snippet 字段名：渲染器对 snippet/text 两者都认。
        let data = json!({
            "hits": [{ "id": "msg_v1_aaaa", "score": 2.0, "snippet": "新的 snippet 字段" }],
            "generation": 3,
            "truncation": { "truncated": false, "reason": null },
        });
        let lines = render_success("search", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "1 hit(s) (generation 3)",
                "  1. msg_v1_aaaa  score 2.00",
                "     新的 snippet 字段",
            ]
        );
    }

    #[test]
    fn search_hit_without_preview_omits_snippet_line() {
        let data = json!({
            "hits": [{ "id": "msg_v1_aaaa", "score": 2.0 }],
            "generation": 3,
            "truncation": { "truncated": false, "reason": null },
        });
        let lines = render_success("search", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            ["1 hit(s) (generation 3)", "  1. msg_v1_aaaa  score 2.00",]
        );
    }

    #[test]
    fn cursor_hint_requires_both_has_more_and_token() {
        let data = json!({
            "hits": [],
            "generation": 1,
            "truncation": { "truncated": false, "reason": null },
        });
        // has_more 但无令牌：宁缺续页行，也不渲染无法使用的提示。
        let page = Page {
            next_cursor: None,
            has_more: true,
        };
        assert_eq!(
            render_success("search", Outcome::Success, &data, &page),
            ["no hits", "提示：试试更短或更少的关键词（如只搜一个词）。",]
        );
        // 有令牌但 has_more=false：同样不给续页行。
        let page = Page {
            next_cursor: Some("tok".into()),
            has_more: false,
        };
        assert_eq!(
            render_success("search", Outcome::Success, &data, &page),
            ["no hits", "提示：试试更短或更少的关键词（如只搜一个词）。",]
        );
    }

    #[test]
    fn partial_outcome_without_truncation_metadata_degrades_to_placeholder() {
        let lines = render_success(
            "search",
            Outcome::Partial,
            &json!({ "hits": [] }),
            &Page::default(),
        );
        assert_eq!(
            lines,
            [
                "no hits",
                "提示：试试更短或更少的关键词（如只搜一个词）。",
                "truncated: ?"
            ]
        );
    }

    #[test]
    fn list_renders_id_and_sanitized_payload_preview() {
        let long_payload = format!("a\tb\nc{}", "d".repeat(100));
        let data = json!({
            "entries": [
                { "id": "msg_v1_aaaa", "payload": long_payload },
                { "id": "ses_v1_bbbb", "payload": "{\"document\":\"doc_v1_x\"}" },
            ],
            "generation": 4,
            "truncation": { "truncated": false, "reason": null },
        });
        let lines = render_success("list", Outcome::Success, &data, &Page::default());
        // 控制字符替换为空格后按 60 字符截断："a b c" 5 字符 + 55 个 d。
        let expected_preview = format!("a b c{}", "d".repeat(55));
        assert_eq!(expected_preview.chars().count(), LIST_PREVIEW_CHARS);
        assert_eq!(
            lines,
            [
                "2 entries (generation 4)".to_string(),
                format!("  msg_v1_aaaa  {expected_preview}"),
                "  ses_v1_bbbb  {\"document\":\"doc_v1_x\"}".to_string(),
            ]
        );
    }

    #[test]
    fn list_zero_entries_is_worded() {
        let data = json!({
            "entries": [],
            "generation": 0,
            "truncation": { "truncated": false, "reason": null },
        });
        let lines = render_success("list", Outcome::Success, &data, &Page::default());
        assert_eq!(lines, ["catalog is empty"]);
    }

    #[test]
    fn get_returns_payload_lines_verbatim_without_trailing_blanks() {
        let data = json!({ "payload": "alpha\nbeta\n\n" });
        let lines = render_success("get", Outcome::Success, &data, &Page::default());
        assert_eq!(lines, ["alpha", "beta"]);
    }

    #[test]
    fn get_null_payload_is_not_found() {
        let data = json!({ "payload": null });
        let lines = render_success("get", Outcome::Success, &data, &Page::default());
        assert_eq!(lines, ["not found"]);
    }

    #[test]
    fn show_renders_curated_fields_only() {
        // human 模式只展示 role/text/timestamp/session；内部字段留给 json 模式。
        let data = json!({
            "entity": {
                "text": "hello",
                "role": "user",
                "span": { "start": 10 },
                "is_sidechain": false,
                "timestamp": "2026-07-28T00:00:00Z",
                "session": "ses_v1_abc",
                "parent": null,
                "spans": [],
            }
        });
        let lines = render_success("show", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "role: user",
                "timestamp: 2026-07-28T00:00:00Z",
                "session: ses_v1_abc",
                "text: hello",
                "用 context <session> 展开这个会话的完整上下文。",
            ]
        );
    }

    #[test]
    fn show_null_entity_is_not_found() {
        let data = json!({ "entity": null });
        let lines = render_success("show", Outcome::Success, &data, &Page::default());
        assert_eq!(lines, ["not found"]);
    }

    #[test]
    fn show_session_entity_falls_back_to_generic_kv() {
        // 会话容器没有 role/text：不套消息投影，按顶层字段展开；无 `?`、
        // 无编造的 context 提示。
        let data = json!({
            "entity": {
                "document": "doc_v1_x",
                "documents": ["doc_v1_x"],
                "messages": ["msg_v1_a", "msg_v1_b"],
            }
        });
        let lines = render_success("show", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "document: doc_v1_x",
                "documents: [\"doc_v1_x\"]",
                "messages: [\"msg_v1_a\",\"msg_v1_b\"]",
            ]
        );
    }

    #[test]
    fn show_document_entity_falls_back_to_generic_kv() {
        let data = json!({
            "entity": {
                "provider": "claude-code",
                "variant": "claude-code/jsonl-v1",
                "fingerprint": "0123abcd",
                "len": 128,
            }
        });
        let lines = render_success("show", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "fingerprint: 0123abcd",
                "len: 128",
                "provider: claude-code",
                "variant: claude-code/jsonl-v1",
            ]
        );
    }

    #[test]
    fn show_message_without_session_omits_fabricated_hint() {
        // 会话引用未知时 `context <session>` 提示无从执行：保留四行投影，
        // 不渲染提示行。
        let data = json!({
            "entity": {
                "role": "user",
                "text": "hello",
                "timestamp": "2026-07-28T00:00:00Z",
            }
        });
        let lines = render_success("show", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "role: user",
                "timestamp: 2026-07-28T00:00:00Z",
                "session: ?",
                "text: hello",
            ]
        );
    }

    #[test]
    fn show_legacy_bare_text_payload_gets_curated_projection() {
        // main.rs 对非 JSON payload 的兜底形状 {role: null, text: <bytes>}：
        // 仍是消息实体，走 curated 投影而非 kv。
        let data = json!({ "entity": { "role": null, "text": "raw bytes" } });
        let lines = render_success("show", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            ["role: ?", "timestamp: ?", "session: ?", "text: raw bytes",]
        );
    }

    #[test]
    fn context_renders_header_numbered_messages_and_evidence() {
        let data = json!({
            "session_id": "ses_v1_abc",
            "session": { "document": "doc_v1_x", "messages": ["msg_v1_a", "msg_v1_b"] },
            "branch_leaf": "msg_v1_b",
            "messages": [
                {
                    "id": "msg_v1_a",
                    "message_id": "msg_v1_a",
                    "payload": { "role": "user", "text": "hello\nworld", "is_sidechain": false },
                },
                {
                    "id": "msg_v1_b",
                    "message_id": "msg_v1_b",
                    "payload": { "role": "assistant", "text": "t".repeat(100) },
                },
                { "id": "msg_v1_c", "message_id": "msg_v1_c", "payload": { "role": null } },
            ],
            "evidence": [
                { "occurrence_id": "aa", "message_id": "msg_v1_a", "generation": 9 },
                { "occurrence_id": "bb", "message_id": "msg_v1_b", "generation": 9 },
            ],
            "truncation": { "truncated": true, "reason": "max_messages" },
            "generation": 9,
        });
        let lines = render_success("context", Outcome::Partial, &data, &Page::default());
        // msg_v1_b 的 100 个 t 超过 CONTEXT_TEXT_CHARS：截断并标注真实 wire id。
        let truncated_text = format!(
            "{}…(已截断,用 show msg_v1_b 看全文)",
            "t".repeat(CONTEXT_TEXT_CHARS)
        );
        assert_eq!(
            lines,
            [
                "session ses_v1_abc  branch leaf msg_v1_b  (generation 9)".to_string(),
                "  1. [user] hello world".to_string(),
                format!("  2. [assistant] {truncated_text}"),
                "  3. [?] ?".to_string(),
                "evidence: 2 span(s)".to_string(),
                "truncated: max_messages".to_string(),
            ]
        );
    }

    #[test]
    fn context_truncation_hint_falls_back_to_id_when_message_id_missing() {
        let data = json!({
            "session_id": "ses_v1_abc",
            "messages": [
                { "id": "msg_v1_only_id", "payload": { "role": "user", "text": "t".repeat(100) } },
            ],
            "evidence": [],
            "truncation": { "truncated": false, "reason": null },
            "generation": 1,
        });
        let lines = render_success("context", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines[1],
            format!(
                "  1. [user] {}…(已截断,用 show msg_v1_only_id 看全文)",
                "t".repeat(CONTEXT_TEXT_CHARS)
            )
        );
    }

    #[test]
    fn context_truncation_without_any_id_omits_show_hint() {
        let data = json!({
            "session_id": "ses_v1_abc",
            "messages": [
                { "payload": { "role": "user", "text": "t".repeat(100) } },
            ],
            "evidence": [],
            "truncation": { "truncated": false, "reason": null },
            "generation": 1,
        });
        let lines = render_success("context", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines[1],
            format!("  1. [user] {}…(已截断)", "t".repeat(CONTEXT_TEXT_CHARS))
        );
    }

    #[test]
    fn context_zero_messages_keeps_worded_header_and_evidence() {
        let data = json!({
            "session_id": "ses_v1_abc",
            "session": { "document": "doc_v1_x", "messages": [] },
            "branch_leaf": null,
            "messages": [],
            "evidence": [],
            "truncation": { "truncated": false, "reason": null },
            "generation": 2,
        });
        let lines = render_success("context", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "session ses_v1_abc  branch leaf none  (generation 2)",
                "evidence: 0 span(s)",
            ]
        );
    }

    #[test]
    fn status_renders_entities_and_generation() {
        let data = json!({ "catalog_count": 5, "generation": 2 });
        let lines = render_success("status", Outcome::Success, &data, &Page::default());
        assert_eq!(lines, ["entities: 5", "generation: 2"]);
    }

    #[test]
    fn status_missing_fields_degrade_to_placeholders() {
        let lines = render_success("status", Outcome::Success, &json!({}), &Page::default());
        assert_eq!(lines, ["entities: ?", "generation: ?"]);
    }

    #[test]
    fn unknown_command_falls_back_to_sorted_key_value_lines() {
        let data = json!({
            "variant": "claude-code/v1",
            "committed": 3,
            "unchanged": 0,
            "skipped": 0,
            "generation": 4,
            "source_fp": "b3:abcd",
        });
        let lines = render_success("frobnicate", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "committed: 3",
                "generation: 4",
                "skipped: 0",
                "source_fp: b3:abcd",
                "unchanged: 0",
                "variant: claude-code/v1",
            ]
        );
    }

    #[test]
    fn sync_renders_field_stats_with_unit_annotations() {
        let data = json!({
            "sources": 2,
            "messages": 10,
            "committed": 10,
            "emitted": 3,
            "unchanged": 7,
            "skipped": 0,
            "diagnostics": 0,
            "generation": 4,
        });
        let lines = render_success("sync", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "sources: 2（本次扫描的源文件数）",
                "emitted: 3（本次新解析的消息条数）",
                "unchanged: 7（未变化的已有消息条数，不是文件数）",
                "skipped: 0（因格式无法入库的消息条数）",
                "generation: 4（当前入库代次）",
                "总结：新增 3 条消息。",
            ]
        );
        let data = json!({
            "tool": "agent-session-grep",
            "version": "0.3.0",
            "db": "not-checked",
            "schema": null,
        });
        let lines = render_success("doctor", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "db: not-checked",
                "schema: null",
                "tool: agent-session-grep",
                "version: 0.3.0",
            ]
        );
    }

    #[test]
    fn ingest_renders_dedicated_single_source_fields() {
        // ingest 响应没有 `sources` 字段：专属版式恒报 1（单文件），只渲染
        // 响应实际携带的字段，无 `sources: ?`。
        let data = json!({
            "variant": "claude-code/jsonl-v1",
            "emitted": 3,
            "committed": 3,
            "unchanged": 0,
            "skipped": 0,
            "diagnostics": 1,
            "generation": 4,
            "source_fp": "b3:abcd",
        });
        let lines = render_success("ingest", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "sources: 1（本次扫描的源文件数）",
                "variant: claude-code/jsonl-v1",
                "source_fp: b3:abcd",
                "committed: 3（本次新入库的消息条数）",
                "diagnostics: 1（解析诊断条数）",
                "总结：新增 3 条消息。",
            ]
        );
    }

    #[test]
    fn ingest_unchanged_file_summarizes_no_new_messages() {
        // 重复 ingest 未变化的文件：committed 为 0，结论行如实报"无变化"。
        let data = json!({
            "variant": "claude-code/jsonl-v1",
            "emitted": 3,
            "committed": 0,
            "unchanged": 3,
            "skipped": 0,
            "diagnostics": 0,
            "generation": 4,
            "source_fp": "b3:abcd",
        });
        let lines = render_success("ingest", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "sources: 1（本次扫描的源文件数）",
                "variant: claude-code/jsonl-v1",
                "source_fp: b3:abcd",
                "committed: 0（本次新入库的消息条数）",
                "diagnostics: 0（解析诊断条数）",
                "总结：没有新增消息（源文件未变化）。",
            ]
        );
    }

    #[test]
    fn ingest_missing_fields_render_nothing_not_question_marks() {
        let lines = render_success("ingest", Outcome::Success, &json!({}), &Page::default());
        assert_eq!(lines, ["sources: 1（本次扫描的源文件数）"]);
    }

    #[test]
    fn non_object_data_never_panics_and_stays_single_line() {
        let odd_values = [
            json!(null),
            json!("x\r\ny"),
            json!(3),
            json!([1, 2]),
            json!({}),
        ];
        let commands = [
            "search", "list", "get", "show", "context", "status", "sync", "ingest", "wat",
        ];
        for command in commands {
            for data in &odd_values {
                for outcome in [Outcome::Success, Outcome::Partial] {
                    let lines = render_success(command, outcome, data, &page_more("tok"));
                    for line in &lines {
                        assert!(
                            !line.contains('\n') && !line.contains('\r'),
                            "control char leaked from {command}: {line:?}"
                        );
                    }
                    assert!(lines.last().is_none_or(|line| !line.trim().is_empty()));
                }
            }
        }
    }
}
