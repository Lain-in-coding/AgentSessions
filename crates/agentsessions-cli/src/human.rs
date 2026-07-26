//! Human 渲染器：把成功结果投影为人类可读文本行（无 envelope、无颜色）。
//!
//! 见 `.trellis/tasks/07-26-human-robot-protocol/design.md` §2。
//! search/list/get/show/context/status 各有专属版式，其余命令（ingest/sync/
//! index/index.rebuild/doctor/config.paths 及未知命令）共用同一条排序
//! `key: value` 兜底路径。约束：无颜色、无新依赖；任何输入不 panic——缺失
//! 或异常字段降级为 `?` 占位或空态措辞；每个返回元素都是单行可打印文本，
//! 无尾随空行。

use crate::protocol::{Outcome, Page};
use serde_json::Value;

/// `list` payload 预览的最大字符数（design §2）。
const LIST_PREVIEW_CHARS: usize = 60;
/// `context` 消息正文预览的最大字符数（design §2）。
const CONTEXT_TEXT_CHARS: usize = 80;

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
        _ => kv_lines(data),
    }
}

/// `search`：头行 `N hit(s) (generation G)` + 每命中 `  <rank>. <id>  score <s>`；
/// 零命中给措辞 `no hits`。
fn render_search(data: &Value) -> Vec<String> {
    let hits = data
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if hits.is_empty() {
        return vec!["no hits".into()];
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
        "{} entrie(s) (generation {})",
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

/// `show`：实体按顶层字段展开为排序 `key: value` 行；null 或缺失 → `not found`。
fn render_show(data: &Value) -> Vec<String> {
    match data.get("entity") {
        None | Some(Value::Null) => vec!["not found".into()],
        Some(entity) => kv_lines(entity),
    }
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
        let text = payload
            .and_then(|payload| payload.get("text"))
            .and_then(Value::as_str)
            .map(|text| preview(text, CONTEXT_TEXT_CHARS))
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
        lines.push(format!("more: pass --cursor {}", sanitize(token)));
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
        assert_eq!(lines, ["no hits"]);
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
                "more: pass --cursor tok.abc",
            ]
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
            ["no hits"]
        );
        // 有令牌但 has_more=false：同样不给续页行。
        let page = Page {
            next_cursor: Some("tok".into()),
            has_more: false,
        };
        assert_eq!(
            render_success("search", Outcome::Success, &data, &page),
            ["no hits"]
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
        assert_eq!(lines, ["no hits", "truncated: ?"]);
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
                "2 entrie(s) (generation 4)".to_string(),
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
    fn show_renders_sorted_top_level_fields() {
        let data = json!({
            "entity": {
                "text": "hello",
                "role": "user",
                "span": { "start": 10 },
                "is_sidechain": false,
                "timestamp": null,
            }
        });
        let lines = render_success("show", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "is_sidechain: false",
                "role: user",
                "span: {\"start\":10}",
                "text: hello",
                "timestamp: null",
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
    fn context_renders_header_numbered_messages_and_evidence() {
        let data = json!({
            "session_id": "ses_v1_abc",
            "session": { "document": "doc_v1_x", "messages": ["msg_v1_a", "msg_v1_b"] },
            "branch_leaf": "msg_v1_b",
            "messages": [
                {
                    "id": "msg_v1_a",
                    "payload": { "role": "user", "text": "hello\nworld", "is_sidechain": false },
                },
                { "id": "msg_v1_b", "payload": { "role": "assistant", "text": "t".repeat(100) } },
                { "id": "msg_v1_c", "payload": { "role": null } },
            ],
            "evidence": [
                { "occurrence_id": "aa", "message_id": "msg_v1_a", "generation": 9 },
                { "occurrence_id": "bb", "message_id": "msg_v1_b", "generation": 9 },
            ],
            "truncation": { "truncated": true, "reason": "max_messages" },
            "generation": 9,
        });
        let lines = render_success("context", Outcome::Partial, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "session ses_v1_abc  branch leaf msg_v1_b  (generation 9)".to_string(),
                "  1. [user] hello world".to_string(),
                format!("  2. [assistant] {}", "t".repeat(CONTEXT_TEXT_CHARS)),
                "  3. [?] ?".to_string(),
                "evidence: 2 span(s)".to_string(),
                "truncated: max_messages".to_string(),
            ]
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
    fn builtin_write_commands_share_the_generic_path() {
        let data = json!({
            "sources": 2,
            "messages": 10,
            "committed": 10,
            "unchanged": 0,
            "generation": 4,
        });
        let lines = render_success("sync", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "committed: 10",
                "generation: 4",
                "messages: 10",
                "sources: 2",
                "unchanged: 0",
            ]
        );
        let data = json!({
            "tool": "agentsessions",
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
                "tool: agentsessions",
                "version: 0.3.0",
            ]
        );
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
            "search", "list", "get", "show", "context", "status", "sync", "wat",
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
