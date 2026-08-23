//! Human 渲染器：把成功结果投影为人类可读文本行（无 envelope、无颜色）。
//!
//! search/list/get/show/context/status/providers/doctor 各有专属版式，sync 与 ingest
//! 各有统计版式，其余命令（index/index.rebuild/config.paths 及未知命令）共用
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
        "stats" => render_stats(data),
        "sync" => render_sync(data),
        "ingest" => render_ingest(data),
        "handoff" => render_handoff(data),
        "resume" => render_resume(data),
        "forget" | "prune" => render_forget(data),
        "providers" => render_providers(data),
        "doctor" => render_doctor(data),
        "config.paths" => {
            // config paths 报告的是"默认位置"，未用到就不会创建；新手照着找会扑空
            // （10 角色体验测试缺陷）。加一句说明，结构本身保持稳定。
            //
            // `config_is_read` 是给机器面的布尔字段，人类版式里由下面那句话表达，
            // 不再作为一行 `config_is_read: false` 混在路径列表中间。
            let mut filtered = data.clone();
            if let Some(obj) = filtered.as_object_mut() {
                obj.remove("config_is_read");
            }
            let mut lines = kv_lines(&filtered);
            lines.push(
                "Note: these are default locations. A directory that does not exist yet simply \
                 has not been used."
                    .into(),
            );
            // M3-6：`config` 那一行是**保留路径**，当前没有任何代码读它
            // （依赖图里没有 TOML 解析器，全仓库零处读取 config.toml）。
            // 不加说明就是在宣传一个不存在的功能：用户会去写那个文件然后
            // 困惑为什么没生效。宁可如实标注，也不为一个尚未实现的功能引入依赖。
            lines.push(
                "Note: `config` is a reserved path. This build reads no configuration file -- \
                 every setting comes from flags and environment variables. Writing a file there \
                 has no effect."
                    .into(),
            );
            lines
        }
        _ => kv_lines(data),
    }
}

/// `doctor`：先按既有 `key: value` 版式报事实，再逐条列出引导式检查（M4-6）。
///
/// 通过的检查只占一行（`ok  <name>  <detail>`），未通过的检查追加一行缩进的
/// `-> <next_step>`——doctor 过去只报事实，把"那我该干什么"留给读过源码的人。
///
/// 事实行保持原样（`checks` 键从 kv 兜底里摘掉，否则会渲染成一行巨大的 JSON），
/// 因此没有 `checks` 的旧响应（以及机器面）版式完全不变。
fn render_doctor(data: &Value) -> Vec<String> {
    let mut facts = data.clone();
    let checks = facts
        .as_object_mut()
        .and_then(|object| object.remove("checks"))
        .unwrap_or(Value::Null);
    let mut lines = kv_lines(&facts);
    let checks = checks.as_array().map(Vec::as_slice).unwrap_or_default();
    if checks.is_empty() {
        return lines;
    }
    let field = |check: &Value, key: &str| {
        check
            .get(key)
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_default()
    };
    lines.push(String::new());
    lines.push("CHECKS:".into());
    for check in checks {
        let status = field(check, "status");
        let status = if status.is_empty() {
            "?".into()
        } else {
            status
        };
        lines.push(format!(
            "  {status:<4} {}  {}",
            field(check, "name"),
            field(check, "detail")
        ));
        let next_step = field(check, "next_step");
        if !next_step.is_empty() {
            lines.push(format!("       -> {next_step}"));
        }
    }
    lines
}

/// `providers`：每个 provider 用两行展示成熟度/路线目标及完整逐字段能力。
/// `maturity_target: null`（deferred）与空 variant 都显示为破折号，不把未知目标
/// 伪装为当前事实。
fn render_providers(data: &Value) -> Vec<String> {
    let providers = data
        .get("providers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if providers.is_empty() {
        return vec!["providers: none".into()];
    }

    let field = |provider: &Value, key: &str| {
        provider
            .get(key)
            .and_then(Value::as_str)
            .map(sanitize)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| MISSING.into())
    };
    let mut lines = Vec::with_capacity(1 + providers.len() * 2);
    lines.push(format!("providers: {}", providers.len()));
    for provider in providers {
        lines.push(format!(
            "{}  maturity={}  target={}  variant={}",
            field(provider, "provider_id"),
            field(provider, "maturity"),
            field(provider, "maturity_target"),
            field(provider, "variant_id"),
        ));
        lines.push(format!(
            "  discover={} probe={} parse={} search={} context={} resume={} handoff={} tool_activity={} source_span={} incremental={}",
            field(provider, "discover"),
            field(provider, "probe"),
            field(provider, "parse"),
            field(provider, "search"),
            field(provider, "context"),
            field(provider, "resume"),
            field(provider, "handoff"),
            field(provider, "tool_activity"),
            field(provider, "source_span"),
            field(provider, "incremental"),
        ));
    }
    lines
}

/// `resume`：dry-run 预览把要执行的命令摊开给人看——可恢复性、完整命令、
/// 原工作目录、权限模式，以及是否已执行。不可恢复时给出原因与措辞，
/// 不打印空命令行。
fn render_resume(data: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    let session = data
        .get("session_id")
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_else(|| "?".into());
    let provider = data
        .get("provider_id")
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_else(|| MISSING.into());
    lines.push(format!("session {session}  (provider {provider})"));

    let available = data
        .get("available")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !available {
        let reason = data
            .get("unavailable_reason")
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_else(|| "no reason recorded".into());
        lines.push(format!("not resumable: {reason}"));
        lines.push(
            "Note: the history is still searchable; only in-place resume is unavailable.".into(),
        );
        return lines;
    }

    let command = data
        .get("command")
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_else(|| "?".into());
    lines.push(format!("command: {command}"));
    if let Some(dir) = data.get("working_directory").and_then(Value::as_str) {
        lines.push(format!("working directory: {}", sanitize(dir)));
    }
    // 权限模式恒如实标注：未核验时明示"未核验"，绝不静默省略（audit P1-2）。
    let permission_mode = data
        .get("permission_mode")
        .and_then(Value::as_str)
        .map(sanitize);
    let permission_verified = data
        .get("permission_mode_verified")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match permission_mode {
        Some(mode) => lines.push(format!("permission mode: {}", mode)),
        None if permission_verified => {
            lines.push("permission mode: default (no yolo/full-auto)".into())
        }
        None => lines.push(
            "permission mode: not verified (nothing is passed; yolo/full-auto is never added)"
                .into(),
        ),
    }
    let executed = data
        .get("executed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if executed {
        lines.push("executed: the provider process started and exited.".into());
    } else if data
        .get("first_run_preview")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        lines.push("first use: preview was forced and nothing ran (--yes was ignored). Run resume --yes again to confirm before it really executes.".into());
    } else {
        lines.push("dry-run: nothing ran. Add --yes to really resume.".into());
    }
    lines
}

/// `handoff`：把 handoff pack 投影为可读分栏——pack 元信息、matched
/// sessions、原文证据（evidence）、推断（inference）与预算/截断状态。
/// 任何字段缺失都降级为 `?` 或空态措辞（与其余渲染器同一约束）。
fn render_handoff(data: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    let pack_id = data
        .get("pack_id")
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_else(|| "?".into());
    let created_at = data
        .get("created_at")
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_else(|| "?".into());
    let generation = number_text(data, "catalog_generation");
    let schema_version = data
        .get("schema_version")
        .and_then(Value::as_str)
        .unwrap_or("?");
    lines.push(format!(
        "handoff pack {pack_id} (schema {schema_version}; generation {generation})"
    ));
    lines.push(format!("created_at: {created_at}"));

    let confidence = data
        .get("confidence")
        .and_then(|c| c.get("overall"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    lines.push(format!("confidence: {confidence}"));

    // Matched sessions: id + occurrences.
    let matched = data
        .get("matched_sessions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if matched.is_empty() {
        lines.push("matched_sessions: none".into());
    } else {
        lines.push(format!("matched_sessions: {}", matched.len()));
        for session in matched {
            let id = session
                .get("session_id")
                .and_then(Value::as_str)
                .map(sanitize)
                .unwrap_or_else(|| "?".into());
            let occurrences = session
                .get("occurrences")
                .and_then(Value::as_u64)
                .unwrap_or(1);
            lines.push(format!("  {id}  ({occurrences} occurrence(s))"));
        }
    }

    // Evidence: original text spans, one line each (truncated preview).
    let evidence = data
        .get("evidence")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    lines.push(format!("evidence: {}", evidence.len()));
    for entry in evidence {
        let text = entry
            .get("text")
            .and_then(Value::as_str)
            .map(|text| preview(text, CONTEXT_TEXT_CHARS))
            .unwrap_or_default();
        let id = entry
            .get("message_id")
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_else(|| "?".into());
        lines.push(format!("  [{id}] {text}"));
    }

    // Inference: deterministic generator emits none; render honestly.
    let inference = data
        .get("inference")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if inference.is_empty() {
        lines.push("inference: none (deterministic)".into());
    } else {
        lines.push(format!("inference: {}", inference.len()));
        for entry in inference {
            let text = entry
                .get("text")
                .and_then(Value::as_str)
                .map(|text| preview(text, CONTEXT_TEXT_CHARS))
                .unwrap_or_default();
            lines.push(format!("  {text}"));
        }
    }

    // Truncation: only surface when the pack is incomplete.
    if let Some(truncation) = data.get("truncation") {
        let truncated = truncation
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if truncated {
            let reason = truncation
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let dropped = truncation
                .get("dropped_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            lines.push(format!("truncated: reason={reason} dropped={dropped}"));
        }
    }
    lines
}

/// `search`：头行 `N hit(s) (generation G)` + 每命中 `  <rank>. <id>  score <s>`，
/// 若命中有正文预览（human 渲染前由 CLI 附加）则追加一行缩进的片段；
/// 零命中给措辞 `no hits` 并提示换词。
fn render_search(data: &Value) -> Vec<String> {
    if let Some(rows) = session_resume_rows(data) {
        let mut lines: Vec<String> = render_session_resume_table(&rows)
            .lines()
            .map(str::to_string)
            .collect();
        // 表格下方给出可直接执行的下一步命令（M2P-1）：Session ID 列是 canonical
        // `ses_v1_…`，这些命令用的是同一个 id，照抄即通。
        let commands = data
            .get("suggested_next_commands_human")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let commands: Vec<&str> = commands
            .iter()
            .filter_map(Value::as_str)
            .filter(|command| !command.is_empty())
            .collect();
        if !commands.is_empty() {
            lines.push(String::new());
            lines.push("Next:".into());
            for command in commands {
                lines.push(format!("  {}", sanitize(command)));
            }
        }
        return lines;
    }
    let hits = data
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if hits.is_empty() {
        // 全新库上"换个关键词"是错的建议——真正的下一步是先建索引（M2P-4）。
        if let Some(lines) = empty_catalog_lines(data) {
            return lines;
        }
        return vec![
            "no hits".into(),
            "Hint: try a shorter query, or fewer words (a single word often works).".into(),
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
        if let Some(project) = hit.get("project_name").and_then(Value::as_str)
            && !project.is_empty()
        {
            lines.push(format!("     project · {}", sanitize(project)));
        }
        // 正文预览：命中是否有用一瞥即知。取不到 preview 的命中不补行。
        // ADR-0008 后摘要统一由命中对象的 `text` 字段承载（application 装配，
        // 按 max_snippet_chars 截前缀）；`snippet` 是旧字段名，为兼容旧形状
        // 仍作为回退读取。两者都不存在则省略该行。
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

/// 解析 main.rs 附加的 `session_resume_rows`（human 专用投影）为表格行。
/// 缺失或空数组返回 `None`——调用方据此回落到各命令自己的默认版式。
fn session_resume_rows(data: &Value) -> Option<Vec<SessionResumeTableRow>> {
    let rows = data.get("session_resume_rows").and_then(Value::as_array)?;
    if rows.is_empty() {
        return None;
    }
    Some(
        rows.iter()
            .map(|row| SessionResumeTableRow {
                date_ymd: row
                    .get("date")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                provider: row
                    .get("provider")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                title: row.get("title").and_then(Value::as_str).map(str::to_string),
                working_directory: row
                    .get("working_directory")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                session_id: row
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
            .collect(),
    )
}

/// `list`：头行 `N entrie(s) (generation G)` + 每条 `  <id>  <payload 预览>`；
/// 零条目给措辞 `catalog is empty`。
///
/// 本页全是会话实体时（`list --sessions`），main.rs 会附加 human 专用的
/// `session_resume_rows`，此时改用与 search 相同的会话表格：
/// `--sort recency` 的第一行就是最近一次活动，"我昨天干了什么"由此可答。
fn render_list(data: &Value) -> Vec<String> {
    if let Some(rows) = session_resume_rows(data) {
        return render_session_resume_table(&rows)
            .lines()
            .map(str::to_string)
            .collect();
    }
    let entries = data
        .get("entries")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if entries.is_empty() {
        if let Some(lines) = empty_catalog_lines(data) {
            return lines;
        }
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
                lines.push(
                    "Run `context <session>` to expand the full context of this session.".into(),
                );
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

/// `stats`：历史构成普查的可扫描 human 投影。项目目录按 ADR-0004
/// 原样显示；unknown / ambiguous 计数单独列出，避免把"无记录"误读成"无冲突"。
fn render_stats(data: &Value) -> Vec<String> {
    let number = |key: &str| number_text(data, key);
    let bucket_lines = |label: &str, key: &str, project: bool| {
        let buckets = data
            .get(key)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut lines = vec![format!("{label}:")];
        for bucket in buckets {
            let value = bucket.get("key").and_then(Value::as_str).unwrap_or(MISSING);
            let sessions = bucket.get("sessions").and_then(Value::as_u64).unwrap_or(0);
            let messages = bucket.get("messages").and_then(Value::as_u64).unwrap_or(0);
            let suffix = if project && value != MISSING {
                " (resolved Original Working Directory)"
            } else {
                ""
            };
            lines.push(format!(
                "  {value}: sessions={sessions} messages={messages}{suffix}"
            ));
        }
        lines
    };
    let mut lines = vec![
        format!("sessions: {}", number("total_sessions")),
        format!("messages: {}", number("total_messages")),
        format!("documents: {}", number("total_documents")),
        format!("tool activities: {}", number("total_tool_activities")),
        format!("generation: {}", number("generation")),
    ];
    lines.extend(bucket_lines("by provider", "by_provider", false));
    lines.extend(bucket_lines("by month", "by_month", false));
    lines.extend(bucket_lines("by project", "by_project", true));
    lines.push(format!(
        "project unknown sessions: {}",
        number("project_unknown_sessions")
    ));
    lines.push(format!(
        "project ambiguous sessions: {}",
        number("project_ambiguous_sessions")
    ));
    lines.extend(bucket_lines("by session size", "by_session_size", false));
    lines
}

/// `status`：`entities: N` + `generation: G` 两行。空库时补上下一步动作（M2P-4）。
fn render_status(data: &Value) -> Vec<String> {
    let mut lines = vec![
        format!("entities: {}", number_text(data, "catalog_count")),
        format!("generation: {}", number_text(data, "generation")),
    ];
    if let Some(command) = empty_catalog_next_command(data) {
        lines.push(format!("The catalog is empty — run: {command}"));
    }
    lines
}

/// 空 catalog 的统一措辞（M2P-4）：`search`/`list` 在全新库上共用这两行。
///
/// 仅当 CLI 已确认 catalog 真为空时（`empty_catalog_next_command` 在场）才生效——
/// 否则"没搜到"会被误报成"没索引"。
fn empty_catalog_lines(data: &Value) -> Option<Vec<String>> {
    let command = empty_catalog_next_command(data)?;
    Some(vec![
        "the catalog is empty (nothing has been indexed yet)".into(),
        format!("Next: {command}"),
    ])
}

/// CLI 在 Human 模式注入的建索引命令；catalog 非空时不在场。
fn empty_catalog_next_command(data: &Value) -> Option<&str> {
    data.get("empty_catalog_next_command")
        .and_then(Value::as_str)
        .filter(|command| !command.is_empty())
}

/// `sync`/`ingest`：字段统计 + 一句人话总结。unchanged 是消息条数而非文件数，
/// 新手会把 882 读成文件数而困惑（10 角色体验测试缺陷）；这里显式标注单位，
/// 并给出"新增/无变化"的结论行。
///
/// `sync --discover` 的 per-provider 报告在此展开（M2P-6）：过去 CLI 真的算出了
/// 每个 adapter 的 `{id, found, removed, complete, root_state}`，而渲染器从不读
/// `data.discovery`，用户因此不知道扫了哪些 provider、哪些根不存在、
/// 哪些根本不支持自动发现。
fn render_sync(data: &Value) -> Vec<String> {
    let num = |key: &str| number_text(data, key);
    let mut lines = vec![
        format!("sources: {}（本次扫描的源文件数）", num("sources")),
        format!("emitted: {}（本次新解析的消息条数）", num("emitted")),
        format!("committed: {}（本次实际入库的消息条数）", num("committed")),
        format!(
            "unchanged: {}（未变化的已有消息条数，不是文件数）",
            num("unchanged")
        ),
        format!("skipped: {}（因格式无法入库的消息条数）", num("skipped")),
        format!("generation: {}（当前入库代次）", num("generation")),
    ];
    lines.extend(render_discovery(data));
    // 结论行以 committed（实际入库数）为准，不能用 emitted：源文件有跳过行时
    // emitted > 0 而 committed == 0，旧实现会对什么都没提交的重跑谎报"新增 N 条"
    // ——而真实 transcript 带跳过行正是常态。committed 缺失时不编造结论。
    match data.get("committed").and_then(Value::as_u64) {
        Some(0) => lines.push("总结：没有新增消息（源文件未变化）。".into()),
        Some(count) => lines.push(format!("总结：新增 {count} 条消息。")),
        None => {}
    }
    lines
}

/// `sync --discover` 的 per-provider 覆盖面报告。
///
/// 非 discover 的 `sync <file>` 响应没有 `discovery` 字段，返回空。
///
/// **渐进式披露(M3-17)**:仓库支持 16 个 provider,而用户通常只装 1–3 个。
/// 逐个列出会让一个只装了 Claude Code 的用户读到 13 行"你没装这个"——真正
/// 有信息量的那一行被噪声埋掉。所以默认只展开**值得看的**行,其余折叠成一句
/// 计数并给出展开办法。
///
/// 折叠的判据是"这一行有没有可行动信息",不是"有没有找到文件":
/// - **有发现**(found > 0)→ 展开:这是本次工作的实际来源。
/// - **异常**(home 解析失败、扫描不完整、有 removed)→ 展开:这些影响
///   tombstone 推导与结果完整性,折叠掉会把问题藏起来。
/// - **root 不存在 / 不支持自动发现**且一切正常 → 折叠:对没装该 provider
///   的用户这只是噪声。
///
/// 注意本函数只作用于**人类版式**。`--robot`/JSON 始终携带完整矩阵——机器
/// 消费者需要全量,且契约禁止 CLI 与机器面分叉。
fn render_discovery(data: &Value) -> Vec<String> {
    let Some(discovery) = data.get("discovery") else {
        return Vec::new();
    };
    let providers = discovery
        .get("providers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if providers.is_empty() {
        return Vec::new();
    }
    let complete = discovery.get("complete").and_then(Value::as_bool);
    // `--all` 的展开开关由 CLI 放进 data（纯呈现提示，不影响 providers 矩阵）。
    let expand_all = discovery
        .get("expand_all")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut lines = vec![format!(
        "discovery: 扫描了 {} 个 provider{}",
        providers.len(),
        match complete {
            Some(false) => "（扫描不完整——本轮不推导 tombstone）",
            _ => "",
        }
    )];
    // 折叠的那些 provider：只记 id，末尾汇总成一行。
    let mut quiet: Vec<String> = Vec::new();
    for provider in providers {
        let id = provider
            .get("id")
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_else(|| MISSING.into());
        let found = provider.get("found").and_then(Value::as_u64).unwrap_or(0);
        let removed = provider.get("removed").and_then(Value::as_u64).unwrap_or(0);
        let scan_complete = provider.get("complete").and_then(Value::as_bool);
        let root_state = provider.get("root_state").and_then(Value::as_str);
        let detail = match root_state {
            // 不支持自动发现时必须说出替代动作，否则用户只看到 found: 0。
            Some("unsupported") => {
                "no discovery root — pass transcripts explicitly: asg sync <file>...".to_string()
            }
            Some("home_unresolved") => {
                "cannot resolve your home directory — discovery skipped".to_string()
            }
            Some("missing") => "data root not present — nothing to scan".to_string(),
            _ => {
                let mut detail = format!("{found} source(s) found");
                if removed > 0 {
                    detail.push_str(&format!(", {removed} removed"));
                }
                if scan_complete == Some(false) {
                    detail.push_str(" (scan incomplete)");
                }
                detail
            }
        };
        // 折叠判据只看"这一行有没有可行动信息"。
        //
        // ⚠️ 这里刻意**不看** `complete`:实测(伪 HOME,一个 provider)
        // `unsupported`/`missing` 的行**全都是** `complete: false` ——
        // 该字段的含义是"本轮不能据此推导 tombstone",不是"扫描出错"。
        // 第一版把它当成异常信号,于是 14 行一行都没折叠掉。
        // 整体的 `complete: false` 已经在上面的表头里说了一次,不必每行重复。
        //
        // `home_unresolved` 不在折叠集合内:那是真的出了问题(算不出 home),
        // 且它影响的是**所有** provider,必须直说。
        let uneventful = !expand_all
            && matches!(root_state, Some("unsupported") | Some("missing"))
            && found == 0
            && removed == 0;
        if uneventful {
            quiet.push(id);
        } else {
            lines.push(format!("  {id}: {detail}"));
        }
    }
    if !quiet.is_empty() {
        lines.push(format!(
            "  （另有 {} 个 provider 未检测到数据：{}。用 --all 查看每个的状态）",
            quiet.len(),
            quiet.join("、")
        ));
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

/// `forget` / `prune`：把删除计划渲染成"要删什么、删了没有"两件事。
///
/// dry-run 与已执行用同一张表，只有第一行的动词不同——用户读到的清单与
/// `--yes` 之后真正删掉的东西逐字对应。连带删除的会话单独一行列出：删除以
/// source 为单位，共用同一源文件的其它会话会一起消失，这件事不能藏在 JSON 里。
fn render_forget(data: &Value) -> Vec<String> {
    match data.get("mode").and_then(Value::as_str) {
        Some("list") => {
            let sources = data
                .get("sources")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if sources.is_empty() {
                return vec!["forgotten sources: 0（抑制清单为空）".into()];
            }
            let mut lines = vec![format!("forgotten sources: {}", sources.len())];
            for source in sources {
                let path = source
                    .get("source_path")
                    .and_then(Value::as_str)
                    .map(sanitize)
                    .unwrap_or_else(|| MISSING.into());
                let provider = source
                    .get("provider_id")
                    .and_then(Value::as_str)
                    .map(sanitize)
                    .unwrap_or_else(|| MISSING.into());
                lines.push(format!("  {path}  provider={provider}"));
            }
            lines.push("撤销：forget --readmit <上面的源路径>（下次 sync 会重新索引它）".into());
            return lines;
        }
        Some("readmit") => {
            let restored = data
                .get("readmitted")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            return vec![format!("readmitted: {restored}")];
        }
        _ => {}
    }

    let executed = data
        .get("executed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let count = |key: &str| data.get(key).and_then(Value::as_u64).unwrap_or(0);
    let wires = |key: &str| -> Vec<String> {
        data.get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(sanitize)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut lines = Vec::new();
    lines.push(if executed {
        "已从索引中删除（不可撤销）：".into()
    } else {
        "将从索引中删除（dry-run，尚未执行）：".into()
    });
    if let Some(scope) = data.get("scope") {
        lines.push(format!("  scope: {}", sanitize(&scope.to_string())));
    }
    lines.push(format!(
        "  sessions: {}  messages: {}  tool_activities: {}  sources: {}",
        count("sessions_removed"),
        count("messages"),
        count("activities"),
        count("sources"),
    ));
    for wire in wires("sessions") {
        lines.push(format!("  - {wire}"));
    }
    let collateral = wires("collateral_sessions");
    if !collateral.is_empty() {
        lines.push(format!(
            "  连带删除（与目标会话共用同一源文件，共 {}）：",
            collateral.len()
        ));
        for wire in collateral {
            lines.push(format!("  * {wire}"));
        }
    }
    let surviving = wires("surviving_sessions");
    if !surviving.is_empty() {
        lines.push(format!(
            "  保留（仍被其它源声明，不会消失，共 {}）：",
            surviving.len()
        ));
        for wire in surviving {
            lines.push(format!("  = {wire}"));
        }
    }
    if let Some(generation) = data.get("generation").and_then(Value::as_u64) {
        lines.push(format!("  generation: {generation}"));
    }
    lines
}

/// 截断与续页提示行。`data.truncation` 为权威；元数据缺失但 outcome 已声明/// partial 时仍以 `?` 占位暴露截断事实。续页行要求 has_more 且携带令牌。
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
            "More results: append the --cursor line below to your command to page (a cursor is valid for about 15 minutes)".into(),
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

/// Session Resume 表格的一行（human 渲染专用；值由 main.rs 装配，`date_ymd`
/// 已格式化为本地 `YYYY-MM-DD`）。
pub struct SessionResumeTableRow {
    pub date_ymd: String,
    pub provider: String,
    pub title: Option<String>,
    pub working_directory: Option<String>,
    /// Canonical `ses_v1_…`——catalog 身份，`context`/`show`/`get-session-resume`
    /// 唯一接受的形状（M2P-1：此列过去填的是 provider 原生 id，喂给 context 会被拒）。
    /// provider 原生 id 不进本表：需要它的场景（resume）由 `resume <canonical>` 与
    /// `get-session-resume <canonical>` 从同一个 canonical id 解析出来。
    pub session_id: String,
}

/// 表格目标总宽：日期/Provider/Session ID 按内容占满后，标题与工作目录在
/// 剩余预算内按 35%/65% 分配；预算不足时保留最小列宽，整体超宽交给终端换行。
const TABLE_TARGET_COLS: usize = 100;
/// 标题列最小显示宽度。
const TITLE_MIN_COLS: usize = 12;
/// 工作目录列最小显示宽度。
const CWD_MIN_COLS: usize = 16;
/// 列间隔 ` | ` 的显示宽度。
const COL_GAP_COLS: usize = 3;
/// 缺失值占位符。
const MISSING: &str = "—";

/// 清洗后的单行单元格（缺失值已统一替换为 `—`）。
struct PreparedResumeRow {
    date: String,
    provider: String,
    title: String,
    working_directory: String,
    session_id: String,
}

/// 渲染一组会话 Resume 元数据为横向表格（ADR-0009）：
/// `日期 | Provider | 会话标题 | 工作目录 | Session ID`。
///
/// - Session ID 列是 canonical `ses_v1_…`——可直接复制给 `context`/`show`/
///   `get-session-resume`（M2P-1）。
/// - Provider 与 Session ID 永不截断；标题超长尾部省略（`…`）、工作目录超长
///   中间折叠（如 `C:/…/agent-session-grep`）；缺失值统一渲染 `—`；
///   newline/tab 清洗为空格；CJK 按显示宽度 2 对齐。
/// - 宽度不足时不切换纵向版式：行保持完整、字段值不截断，超宽由终端换行。
pub fn render_session_resume_table(rows: &[SessionResumeTableRow]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cells: Vec<PreparedResumeRow> = rows.iter().map(prepare_resume_row).collect();
    let date_width = display_width("日期")
        .max(10)
        .max(widest(&cells, |cell| &cell.date));
    let provider_width = display_width("Provider").max(widest(&cells, |cell| &cell.provider));
    let session_width = display_width("Session ID").max(widest(&cells, |cell| &cell.session_id));
    let fixed_cols = date_width + provider_width + session_width + COL_GAP_COLS * 4;
    let remaining = TABLE_TARGET_COLS.saturating_sub(fixed_cols);
    let title_width = TITLE_MIN_COLS.max(remaining * 35 / 100);
    let cwd_width = CWD_MIN_COLS.max(remaining * 65 / 100);

    let mut lines = vec![format!(
        "{} | {} | {} | {} | Session ID",
        pad_to_width("日期", date_width),
        pad_to_width("Provider", provider_width),
        pad_to_width("会话标题", title_width),
        pad_to_width("工作目录", cwd_width),
    )];
    for cell in &cells {
        lines.push(format!(
            "{} | {} | {} | {} | {}",
            pad_to_width(&cell.date, date_width),
            pad_to_width(&cell.provider, provider_width),
            pad_to_width(
                &truncate_tail_ellipsis(&cell.title, title_width),
                title_width
            ),
            pad_to_width(
                &collapse_middle(&cell.working_directory, cwd_width),
                cwd_width
            ),
            cell.session_id,
        ));
    }
    lines.join("\n")
}

/// 各单元格的最大显示宽度。
fn widest(cells: &[PreparedResumeRow], key: impl Fn(&PreparedResumeRow) -> &str) -> usize {
    cells.iter().map(key).map(display_width).max().unwrap_or(0)
}

/// 清洗单元格并统一缺失值：控制字符替换为空格，空白（含空串）渲染 `—`。
fn prepare_resume_row(row: &SessionResumeTableRow) -> PreparedResumeRow {
    PreparedResumeRow {
        date: cell_text(&row.date_ymd),
        provider: cell_text(&row.provider),
        title: row
            .title
            .as_deref()
            .map(cell_text)
            .unwrap_or_else(|| MISSING.into()),
        working_directory: row
            .working_directory
            .as_deref()
            .map(cell_text)
            .unwrap_or_else(|| MISSING.into()),
        session_id: cell_text(&row.session_id),
    }
}

/// 单元格文本：控制字符清洗为空格；空白视为缺失，统一渲染 `—`。
fn cell_text(raw: &str) -> String {
    let text = sanitize(raw);
    if text.trim().is_empty() {
        MISSING.into()
    } else {
        text
    }
}

/// 按显示宽度右填充空格至 `width` 列；已超宽时原样返回（不截断）。
fn pad_to_width(text: &str, width: usize) -> String {
    let used = display_width(text);
    if used >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - used))
    }
}

/// 尾部省略：超出 `max_width` 时按显示宽度截前缀并接 `…`（占 1 列）。
fn truncate_tail_ellipsis(text: &str, max_width: usize) -> String {
    let text = sanitize(text);
    if display_width(&text) <= max_width {
        return text;
    }
    let prefix = take_display_prefix(&text, max_width.saturating_sub(1));
    format!("{prefix}…")
}

/// 工作目录中间折叠：保留首段与尾段、中间接 `…`（如 `C:/…/agent-session-grep`）；
/// 单段路径或预算过小退化为字符级中间折叠。
fn collapse_middle(text: &str, max_width: usize) -> String {
    let text = sanitize(text);
    if display_width(&text) <= max_width {
        return text;
    }
    let parts: Vec<&str> = text.split(&['/', '\\'][..]).collect();
    if parts.len() <= 1 || max_width <= 3 {
        return char_fold_middle(&text, max_width);
    }
    let head_budget = (max_width - 3) / 2;
    let tail_budget = max_width - 3 - head_budget;
    let mut head: Vec<&str> = Vec::new();
    let mut head_used = 0usize;
    for (index, part) in parts.iter().enumerate() {
        let cost = display_width(part) + usize::from(index > 0);
        if index > 0 && head_used + cost > head_budget {
            break;
        }
        head_used += cost;
        head.push(part);
    }
    let mut tail: Vec<&str> = Vec::new();
    let mut tail_used = 0usize;
    for (offset, part) in parts.iter().rev().enumerate() {
        let cost = display_width(part) + 1;
        if offset > 0 && tail_used + cost > tail_budget {
            break;
        }
        tail_used += cost;
        tail.push(part);
    }
    let mut tail: Vec<&str> = tail.into_iter().rev().collect();
    if head.len() + tail.len() > parts.len() {
        tail.truncate(parts.len() - head.len());
    }
    let sep = text
        .chars()
        .find(|c| matches!(c, '/' | '\\'))
        .unwrap_or('/');
    let head_text = head.join(&sep.to_string());
    if tail.is_empty() {
        return format!("{head_text}{sep}…");
    }
    let tail_text = tail.join(&sep.to_string());
    format!("{head_text}{sep}…{sep}{tail_text}")
}

/// 字符级中间折叠：前缀 + `…` + 后缀（单段路径或极小预算的退路）。
fn char_fold_middle(text: &str, max_width: usize) -> String {
    let prefix_width = max_width.saturating_sub(1) / 2;
    let suffix_width = max_width.saturating_sub(1) - prefix_width;
    let prefix = take_display_prefix(text, prefix_width);
    let suffix = take_display_suffix(text, suffix_width);
    format!("{prefix}…{suffix}")
}

/// 按显示宽度取前缀，字符边界安全。
fn take_display_prefix(text: &str, max_width: usize) -> String {
    let mut prefix = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        let width = char_display_width(c);
        if used + width > max_width {
            break;
        }
        used += width;
        prefix.push(c);
    }
    prefix
}

/// 按显示宽度取后缀（从尾部累计，再恢复原顺序）。
fn take_display_suffix(text: &str, max_width: usize) -> String {
    let mut suffix: Vec<char> = Vec::new();
    let mut used = 0usize;
    for c in text.chars().rev() {
        let width = char_display_width(c);
        if used + width > max_width {
            break;
        }
        used += width;
        suffix.push(c);
    }
    suffix.into_iter().rev().collect()
}

/// 字符串显示宽度：CJK（统一表意文字、假名、谚文、全角形式等）按 2 列计。
fn display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

/// 单字符显示宽度：CJK 及其兼容形式按 2 列计，其余按 1 列计。
fn char_display_width(c: char) -> usize {
    let code = c as u32;
    if (0x1100..=0x115F).contains(&code) // 谚文字母
        || (0x2E80..=0x303E).contains(&code) // CJK 部首与标点
        || (0x3041..=0x33FF).contains(&code) // 假名、CJK 兼容
        || (0x3400..=0x4DBF).contains(&code) // CJK 扩展 A
        || (0x4E00..=0x9FFF).contains(&code) // CJK 统一表意文字
        || (0xA000..=0xA4CF).contains(&code) // 彝文
        || (0xAC00..=0xD7A3).contains(&code) // 谚文音节
        || (0xF900..=0xFAFF).contains(&code) // CJK 兼容表意文字
        || (0xFE30..=0xFE4F).contains(&code) // CJK 兼容形式
        || (0xFF00..=0xFF60).contains(&code) // 全角形式
        || (0xFFE0..=0xFFE6).contains(&code)
    // 全角符号
    {
        2
    } else {
        1
    }
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
    fn config_paths_says_the_config_file_is_never_read() {
        // M3-6：四个平台分支都打印 `config.toml` 路径，但依赖图里没有 TOML
        // 解析器、全仓库零处读它。不声明就是在宣传一个不存在的功能 ——
        // 用户会去写那个文件，然后困惑为什么没生效。
        let data = json!({
            "config": "/placeholder/config/agentsessions/config.toml",
            "data": "/placeholder/data/agentsessions",
            "cache": "/placeholder/cache/agentsessions",
            "logs": "/placeholder/data/agentsessions/logs",
            "config_is_read": false,
        });
        let text =
            render_success("config.paths", Outcome::Success, &data, &Page::default()).join("\n");
        // 路径本身仍要报告：它是保留位置，不是虚构的。
        assert!(text.contains("config.toml"), "{text}");
        assert!(
            text.contains("reserved path") && text.contains("reads no configuration file"),
            "必须说明配置文件当前不被读取: {text}"
        );
        assert!(
            text.contains("has no effect"),
            "必须说明写那个文件不会生效: {text}"
        );
        // 机器面的布尔字段不该作为一行裸 kv 混进人类版式的路径列表。
        assert!(
            !text.contains("config_is_read"),
            "human 版式不应暴露机器面字段名: {text}"
        );
    }

    #[test]
    fn providers_renders_maturity_target_and_all_capabilities() {
        let data = json!({
            "providers": [{
                "provider_id": "claude-code",
                "variant_id": "claude-code/jsonl-v1",
                "maturity": "experimental",
                "maturity_target": "certified",
                "discover": "native",
                "probe": "native",
                "parse": "native",
                "search": "native",
                "context": "native",
                "resume": "derived",
                "handoff": "unsupported",
                "tool_activity": "partial",
                "source_span": "native",
                "incremental": "native"
            }, {
                "provider_id": "zcode",
                "variant_id": "",
                "maturity": "unsupported",
                "maturity_target": null,
                "discover": "unknown",
                "probe": "unknown",
                "parse": "unknown",
                "search": "unknown",
                "context": "unknown",
                "resume": "unknown",
                "handoff": "unknown",
                "tool_activity": "unknown",
                "source_span": "unknown",
                "incremental": "unknown"
            }]
        });
        let lines = render_success("providers", Outcome::Success, &data, &Page::default());
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "providers: 2");
        assert!(lines[1].contains("maturity=experimental"), "{lines:?}");
        assert!(lines[1].contains("target=certified"), "{lines:?}");
        assert!(lines[2].contains("tool_activity=partial"), "{lines:?}");
        assert!(lines[3].contains("target=—"), "{lines:?}");
        assert!(lines[3].contains("variant=—"), "{lines:?}");
        assert!(lines[4].contains("incremental=unknown"), "{lines:?}");
    }

    #[test]
    fn resume_renders_dry_run_preview() {
        let data = json!({
            "session_id": "ses_v1_aaa",
            "provider_id": "claude-code",
            "available": true,
            "command": "(cd /home/u/proj && claude --resume abc-123)",
            "working_directory": "/home/u/proj",
            "permission_mode": null,
            "permission_mode_verified": false,
            "unavailable_reason": null,
            "executed": false,
        });
        let lines = render_success("resume", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "session ses_v1_aaa  (provider claude-code)",
                "command: (cd /home/u/proj && claude --resume abc-123)",
                "working directory: /home/u/proj",
                "permission mode: not verified (nothing is passed; yolo/full-auto is never added)",
                "dry-run: nothing ran. Add --yes to really resume.",
            ]
        );
    }

    #[test]
    fn resume_renders_first_run_forced_preview() {
        let data = json!({
            "session_id": "ses_v1_ddd",
            "provider_id": "codex",
            "available": true,
            "command": "codex resume xyz",
            "working_directory": null,
            "permission_mode": null,
            "permission_mode_verified": false,
            "unavailable_reason": null,
            "executed": false,
            "first_run_preview": true,
        });
        let lines = render_success("resume", Outcome::Success, &data, &Page::default());
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("first use: preview was forced and nothing ran")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("--yes was ignored")),
            "{lines:?}"
        );
    }

    #[test]
    fn resume_renders_unavailable_without_command_line() {
        let data = json!({
            "session_id": "ses_v1_bbb",
            "provider_id": null,
            "available": false,
            "command": null,
            "working_directory": null,
            "permission_mode": null,
            "unavailable_reason": "no resume metadata claims",
            "executed": false,
        });
        let lines = render_success("resume", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "session ses_v1_bbb  (provider —)",
                "not resumable: no resume metadata claims",
                "Note: the history is still searchable; only in-place resume is unavailable.",
            ]
        );
    }

    #[test]
    fn resume_renders_executed_state() {
        let data = json!({
            "session_id": "ses_v1_ccc",
            "provider_id": "codex",
            "available": true,
            "command": "codex resume xyz",
            "working_directory": null,
            "permission_mode": "--dangerously-bypass-approvals-and-sandbox",
            "unavailable_reason": null,
            "executed": true,
        });
        let lines = render_success("resume", Outcome::Success, &data, &Page::default());
        assert!(
            lines
                .iter()
                .any(|l| l == "executed: the provider process started and exited.")
        );
        assert!(
            lines
                .iter()
                .any(|l| l == "permission mode: --dangerously-bypass-approvals-and-sandbox")
        );
    }

    #[test]
    fn handoff_renders_sections_and_schema() {
        let data = json!({
            "pack_id": "pack_v1_abcd1234",
            "schema_version": "1.0",
            "catalog_generation": 3,
            "created_at": "2026-08-16T01:00:00Z",
            "confidence": { "overall": "high" },
            "matched_sessions": [
                { "session_id": "ses_v1_aaa", "occurrences": 2 }
            ],
            "evidence": [
                { "message_id": "msg_v1_aaa", "text": "the real evidence text" }
            ],
            "inference": [],
            "truncation": { "truncated": false, "reason": "none", "dropped_count": 0 },
        });
        let lines = render_success("handoff", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "handoff pack pack_v1_abcd1234 (schema 1.0; generation 3)",
                "created_at: 2026-08-16T01:00:00Z",
                "confidence: high",
                "matched_sessions: 1",
                "  ses_v1_aaa  (2 occurrence(s))",
                "evidence: 1",
                "  [msg_v1_aaa] the real evidence text",
                "inference: none (deterministic)",
            ]
        );
    }

    #[test]
    fn handoff_renders_truncation_when_incomplete() {
        let data = json!({
            "pack_id": "pack_v1_x",
            "schema_version": "1.0",
            "catalog_generation": 1,
            "created_at": "2026-08-16T01:00:00Z",
            "confidence": { "overall": "low" },
            "matched_sessions": [],
            "evidence": [],
            "inference": [],
            "truncation": { "truncated": true, "reason": "max_evidence", "dropped_count": 9 },
        });
        let lines = render_success("handoff", Outcome::Success, &data, &Page::default());
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("matched_sessions: none"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l == "truncated: reason=max_evidence dropped=9")
        );
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
            [
                "no hits",
                "Hint: try a shorter query, or fewer words (a single word often works).",
            ]
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
                "More results: append the --cursor line below to your command to page (a cursor is valid for about 15 minutes)",
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
        // 旧字段名兼容：渲染器对 snippet/text 两者都认（ADR-0008 后摘要由
        // text 承载，snippet 回退读取）。
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
            [
                "no hits",
                "Hint: try a shorter query, or fewer words (a single word often works).",
            ]
        );
        // 有令牌但 has_more=false：同样不给续页行。
        let page = Page {
            next_cursor: Some("tok".into()),
            has_more: false,
        };
        assert_eq!(
            render_success("search", Outcome::Success, &data, &page),
            [
                "no hits",
                "Hint: try a shorter query, or fewer words (a single word often works).",
            ]
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
                "Hint: try a shorter query, or fewer words (a single word often works).",
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
                "Run `context <session>` to expand the full context of this session.",
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
    fn sync_summary_branches_on_committed_not_emitted() {
        // M2P-6：源文件有跳过行时 emitted > 0 而 committed == 0。旧实现按 emitted
        // 分支，于是什么都没提交的重跑照样打印"新增 N 条消息"——而真实 transcript
        // 带跳过行正是常态。结论行必须以 committed 为准。
        let noop_rerun = json!({
            "sources": 1,
            "emitted": 5,
            "committed": 0,
            "unchanged": 5,
            "skipped": 2,
            "generation": 7,
        });
        let lines = render_success("sync", Outcome::Success, &noop_rerun, &Page::default());
        assert!(
            lines.contains(&"总结：没有新增消息（源文件未变化）。".to_string()),
            "no-op 重跑必须报告无新增: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("新增 5 条")),
            "不得按 emitted 谎报新增: {lines:?}"
        );
        // committed 缺失时不编造结论行。
        let no_committed = json!({ "sources": 1, "emitted": 5, "unchanged": 0, "generation": 1 });
        let lines = render_success("sync", Outcome::Success, &no_committed, &Page::default());
        assert!(
            !lines.iter().any(|line| line.starts_with("总结：")),
            "committed 缺失时不应有结论行: {lines:?}"
        );
    }

    #[test]
    fn sync_discover_reports_every_provider_with_its_root_state() {
        // M2P-6：CLI 一直算出 per-provider 报告，渲染器从不读它。四种 root_state
        // 必须各有可辨认的说法，且"不支持自动发现"要说出替代动作。
        let data = json!({
            "sources": 1,
            "emitted": 2,
            "committed": 2,
            "unchanged": 0,
            "skipped": 0,
            "generation": 3,
            "discovery": {
                "complete": false,
                "providers": [
                    { "id": "claude-code", "found": 12, "removed": 1, "complete": true,
                      "root_state": "scanned" },
                    { "id": "codex", "found": 0, "removed": 0, "complete": false,
                      "root_state": "missing" },
                    { "id": "aider", "found": 0, "removed": 0, "complete": false,
                      "root_state": "unsupported" },
                    { "id": "opencode", "found": 3, "removed": 0, "complete": false,
                      "root_state": "scanned" },
                ]
            }
        });
        let lines = render_success("sync", Outcome::Success, &data, &Page::default());
        let text = lines.join("\n");
        assert!(text.contains("discovery: 扫描了 4 个 provider"), "{text}");
        assert!(text.contains("扫描不完整"), "整体不完整必须说出来: {text}");
        // 已扫描：报计数，removed 非零时一并报出。
        assert!(
            text.contains("claude-code: 12 source(s) found, 1 removed"),
            "{text}"
        );
        // 根不存在 vs 不支持自动发现必须可区分——**在展开时**。
        // 两者都无发现、无 removed,所以默认折叠(M3-17);展开后的措辞由
        // 下面的 `discovery_expands_folded_providers_on_request` 断言。
        assert!(
            !text.contains("codex: data root not present"),
            "无发现的 provider 默认应折叠: {text}"
        );
        assert!(
            !text.contains("aider: no discovery root"),
            "无发现的 provider 默认应折叠: {text}"
        );
        // 折叠必须留下计数与展开办法,不能静默消失。
        assert!(
            text.contains("另有 2 个 provider 未检测到数据"),
            "折叠必须报计数: {text}"
        );
        assert!(text.contains("codex") && text.contains("aider"), "{text}");
        assert!(text.contains("--all"), "折叠必须给出展开办法: {text}");
        // per-provider 的部分扫描单独标注。
        assert!(
            text.contains("opencode: 3 source(s) found (scan incomplete)"),
            "{text}"
        );
        // 非 discover 的普通 sync 不得凭空出现 discovery 段。
        let plain = json!({ "sources": 1, "emitted": 1, "committed": 1, "generation": 1 });
        let lines = render_success("sync", Outcome::Success, &plain, &Page::default());
        assert!(
            !lines.iter().any(|line| line.starts_with("discovery:")),
            "{lines:?}"
        );
    }

    #[test]
    fn discovery_expands_folded_providers_on_request() {
        // `--all` 把折叠掉的行原样展开（措辞与折叠前一致），并且不再出现折叠汇总。
        let providers = json!([
            { "id": "claude-code", "found": 1, "removed": 0, "complete": true,
              "root_state": "scanned" },
            { "id": "codex", "found": 0, "removed": 0, "complete": false,
              "root_state": "missing" },
            { "id": "aider", "found": 0, "removed": 0, "complete": false,
              "root_state": "unsupported" },
        ]);
        let expanded = json!({
            "sources": 1, "committed": 1, "generation": 1,
            "discovery": { "complete": false, "providers": providers, "expand_all": true }
        });
        let text = render_success("sync", Outcome::Success, &expanded, &Page::default()).join("\n");
        assert!(text.contains("codex: data root not present"), "{text}");
        assert!(
            text.contains("aider: no discovery root — pass transcripts explicitly"),
            "展开后必须给出手动路径: {text}"
        );
        assert!(!text.contains("另有"), "--all 下不应再出现折叠汇总: {text}");
    }

    #[test]
    fn discovery_never_folds_a_provider_with_findings_or_trouble() {
        // 折叠的边界:有发现、有 removed、或 home 算不出来的行必须留下。
        //
        // ⚠️ 每行的 `complete: false` **不是**留下的理由:实测里
        // `unsupported`/`missing` 全部带 `complete: false`(含义是"本轮不能
        // 推导 tombstone"而非"扫描出错")。第一版把它当异常,结果一行都没折叠。
        let data = json!({
            "sources": 1,
            "committed": 1,
            "generation": 1,
            "discovery": {
                "complete": false,
                "providers": [
                    // 有发现 → 留下。
                    { "id": "claude-code", "found": 2, "removed": 0, "complete": true,
                      "root_state": "scanned" },
                    // root 不存在但有 removed（源消失了）→ 留下:影响 tombstone。
                    { "id": "codex", "found": 0, "removed": 3, "complete": false,
                      "root_state": "missing" },
                    // home 算不出来 → 留下:这是真故障,且影响所有 provider。
                    { "id": "openclaw", "found": 0, "removed": 0, "complete": false,
                      "root_state": "home_unresolved" },
                    // 无发现、无 removed → 折叠。
                    { "id": "pi", "found": 0, "removed": 0, "complete": false,
                      "root_state": "unsupported" },
                ]
            }
        });
        let text = render_success("sync", Outcome::Success, &data, &Page::default()).join("\n");
        assert!(text.contains("claude-code: 2 source(s) found"), "{text}");
        assert!(
            text.contains("codex: data root not present"),
            "removed 非零必须留下: {text}"
        );
        assert!(
            text.contains("openclaw: cannot resolve your home directory"),
            "home 故障绝不折叠: {text}"
        );
        assert!(
            text.contains("另有 1 个 provider 未检测到数据"),
            "只应折叠 pi: {text}"
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
                "committed: 10（本次实际入库的消息条数）",
                "unchanged: 7（未变化的已有消息条数，不是文件数）",
                "skipped: 0（因格式无法入库的消息条数）",
                "generation: 4（当前入库代次）",
                "总结：新增 10 条消息。",
            ]
        );
        let data = json!({
            "tool": "agent-session-grep",
            "version": "0.1.0",
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
                "version: 0.1.0",
            ]
        );
    }

    /// 引导式 doctor（M4-6）：事实行版式不变，`checks` 单独成段，未通过的检查
    /// 各带一行缩进的下一步。`checks` 绝不落进 kv 兜底（否则是一行巨大 JSON）。
    #[test]
    fn doctor_renders_checks_as_a_section_with_next_steps() {
        let data = json!({
            "db": "ok",
            "schema": 13,
            "checks": [
                { "name": "provider_roots", "status": "ok", "detail": "2/6 roots exist" },
                {
                    "name": "catalog_entities",
                    "status": "warn",
                    "detail": "0 entities",
                    "next_step": "Run `asg sync --discover` to index your history.",
                },
            ],
        });
        let lines = render_success("doctor", Outcome::Success, &data, &Page::default());
        assert_eq!(
            lines,
            [
                "db: ok",
                "schema: 13",
                "",
                "CHECKS:",
                "  ok   provider_roots  2/6 roots exist",
                "  warn catalog_entities  0 entities",
                "       -> Run `asg sync --discover` to index your history.",
            ]
        );
        // 通过的检查不留空的下一步行。
        assert_eq!(lines.iter().filter(|line| line.contains("->")).count(), 1);
    }

    /// 没有 `checks` 的响应（旧库、机器面）版式与本改动前逐字节相同。
    #[test]
    fn doctor_without_checks_renders_exactly_as_before() {
        let data = json!({ "db": "not-checked", "schema": null });
        assert_eq!(
            render_success("doctor", Outcome::Success, &data, &Page::default()),
            ["db: not-checked", "schema: null"]
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

    #[test]
    fn resume_table_renders_five_columns_in_contract_order() {
        let rows = [resume_row(
            "2026-08-14",
            "claude-code",
            Some("配置数据库连接"),
            Some("C:/dev/agent-session-grep"),
            "ses_v1_abc",
        )];
        let output = render_session_resume_table(&rows);
        // 列宽：日期 10、Provider 11（内容全宽）、Session ID 10；
        // 剩余 100 - 10 - 11 - 10 - 12 = 57 → 标题 19、工作目录 37。
        let header = format!(
            "日期{} | Provider{} | 会话标题{} | 工作目录{} | Session ID",
            " ".repeat(6),
            " ".repeat(3),
            " ".repeat(11),
            " ".repeat(29),
        );
        let body = format!(
            "2026-08-14 | claude-code | 配置数据库连接{} | C:/dev/agent-session-grep{} | ses_v1_abc",
            " ".repeat(5),
            " ".repeat(12),
        );
        assert_eq!(output, format!("{header}\n{body}"));
    }

    #[test]
    fn resume_table_never_truncates_provider_or_session_id() {
        let provider = "hyperbolic-parallel-provider-v9";
        let session_id = format!("ses_v1_{}", "abcdef0123456789".repeat(4));
        let rows = [resume_row(
            "2026-08-14",
            provider,
            Some("短标题"),
            Some("C:/a"),
            &session_id,
        )];
        let output = render_session_resume_table(&rows);
        let body = output.lines().nth(1).unwrap();
        // Session ID 是末列且不加填充：整行必须以完整 ID 收尾。
        assert!(
            body.ends_with(&session_id),
            "session id truncated: {body:?}"
        );
        // Provider 按内容全宽：完整出现且紧跟列分隔符。
        assert!(
            body.contains(&format!("{provider} |")),
            "provider truncated: {body:?}"
        );
    }

    #[test]
    fn resume_table_renders_dash_for_missing_values() {
        let rows = [
            resume_row("", "codex", None, None, "ses_v1_xyz"),
            resume_row("", "", None, None, "ses_v1_uvw"),
        ];
        let output = render_session_resume_table(&rows);
        // 列宽：日期 10、Provider 8、Session ID 10；剩余 60 → 标题 21、工作目录 39。
        let row1 = format!(
            "—{} | codex{} | —{} | —{} | ses_v1_xyz",
            " ".repeat(9),
            " ".repeat(3),
            " ".repeat(20),
            " ".repeat(38),
        );
        let row2 = format!(
            "—{} | —{} | —{} | —{} | ses_v1_uvw",
            " ".repeat(9),
            " ".repeat(7),
            " ".repeat(20),
            " ".repeat(38),
        );
        assert_eq!(output.lines().nth(1), Some(row1.as_str()));
        assert_eq!(output.lines().nth(2), Some(row2.as_str()));
    }

    #[test]
    fn collapse_middle_keeps_head_and_tail_segments() {
        // 30 列预算：首段 C:/Users、尾段 agent-session-grep，中间折叠。
        assert_eq!(
            collapse_middle("C:/Users/someone/dev/agent-session-grep", 30),
            "C:/Users/…/agent-session-grep"
        );
        // 反斜杠路径同样折叠，分隔符保持原样。
        assert_eq!(
            collapse_middle(r"C:\Users\someone\dev\agent-session-grep", 30),
            r"C:\Users\…\agent-session-grep"
        );
        // 宽度足够时原样返回。
        assert_eq!(
            collapse_middle("C:/dev/agent-session-grep", 40),
            "C:/dev/agent-session-grep"
        );
        // 单段路径退化为字符级中间折叠。
        assert_eq!(
            collapse_middle("abcdefghijklmnopqrstuvwxyz", 10),
            "abcd…vwxyz"
        );
    }

    #[test]
    fn display_width_counts_cjk_as_two_columns() {
        assert_eq!(display_width("会话标题"), 8);
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("2026-08-14"), 10);
        // 12 列预算：按显示宽度截前缀（10 列）再补 `…`（1 列）。
        assert_eq!(
            truncate_tail_ellipsis("这是一段非常非常长的会话标题", 12),
            "这是一段非…"
        );
        // 未超宽不截断。
        assert_eq!(truncate_tail_ellipsis("short", 12), "short");
    }

    #[test]
    fn resume_table_truncates_cjk_title_by_display_width() {
        // 标题列 19：20 个 CJK 字（40 列）→ 按显示宽度保留 9 字 + `…`。
        let rows = [resume_row(
            "2026-08-14",
            "claude-code",
            Some(&"标".repeat(20)),
            Some("C:/a"),
            "ses_v1_abc",
        )];
        let output = render_session_resume_table(&rows);
        let body = output.lines().nth(1).unwrap();
        let expected_title = format!("{}…", "标".repeat(9));
        assert!(
            body.contains(&format!("{expected_title} |")),
            "title not truncated by display width: {body:?}"
        );
    }

    #[test]
    fn resume_table_sanitizes_newlines_and_tabs() {
        let rows = [resume_row(
            "2026-08-14",
            "claude-code",
            Some("第一行\n第二行\ttab"),
            Some("C:/a\r\nb"),
            "ses_v1_abc",
        )];
        let output = render_session_resume_table(&rows);
        assert!(!output.contains('\t'));
        assert!(!output.contains('\r'));
        assert!(!output.ends_with('\n'));
        let body = output.lines().nth(1).unwrap();
        assert!(
            body.contains("第一行 第二行 tab"),
            "control chars not cleaned: {body:?}"
        );
        assert!(
            body.contains("C:/a  b"),
            "cwd control chars not cleaned: {body:?}"
        );
    }

    #[test]
    fn resume_table_empty_input_renders_empty_string() {
        assert_eq!(render_session_resume_table(&[]), "");
    }

    fn resume_row(
        date: &str,
        provider: &str,
        title: Option<&str>,
        cwd: Option<&str>,
        session: &str,
    ) -> SessionResumeTableRow {
        SessionResumeTableRow {
            date_ymd: date.to_string(),
            provider: provider.to_string(),
            title: title.map(str::to_string),
            working_directory: cwd.map(str::to_string),
            session_id: session.to_string(),
        }
    }
}
