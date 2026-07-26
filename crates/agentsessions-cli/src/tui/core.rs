//! TUI 纯核心（Elm-style，task design 07-27-tui-preview §2）：
//! `Model` + `Msg` + `Effect` + [`update`] reducer 与 view-model 纯函数。
//!
//! 约束：本文件不 import ratatui / crossterm / store / App——键盘输入用自带的
//! [`KeyInput`]，数据加载结果用平数据 [`SearchPage`] / [`ContextView`]，副作用只以
//! [`Effect`] 描述、由 glue（mod.rs）执行。业务规则零复制：分页令牌、分支策略、
//! 命中→会话解析全部通过 Effect 交回 Application ADT，本层只持有 UI 状态。

use agentsessions_domain::ContextPolicy;

/// 三屏状态机（PRD R2）：Search（输入）→ Results(命中列表) → Context（消息链）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Search,
    Results,
    Context,
}

/// 键盘输入的自有枚举：core 保持 crossterm-free，glue 负责 KeyEvent → KeyInput 映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyInput {
    Char(char),
    Enter,
    Esc,
    Up,
    Down,
    PgUp,
    PgDn,
    Backspace,
    CtrlC,
}

/// 一页检索结果的平数据投影（glue 从 `AppResponse::Search` 构造）。
///
/// `next_cursor`/`has_more` 只来自 App 响应——core 不构造、不解析令牌。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SearchPage {
    /// 命中 `(wire id, score)`，App 钉住排序原样透传。
    pub hits: Vec<(String, f32)>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub generation: u64,
    pub truncated: bool,
    pub truncation_reason: Option<String>,
}

/// Context 屏单条消息的展示事实：角色 + 文本 + 证据精度（wire 字符串，如 `byte`）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContextMessage {
    pub role: String,
    pub text: String,
    /// 证据精度标记；缺失/legacy 行如实为 `unknown`，绝不臆造。
    pub precision: String,
}

/// 一次会话上下文装配的平数据投影（glue 从 `AppResponse::Context` 构造）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContextView {
    pub session_id: String,
    pub lines: Vec<ContextMessage>,
    pub truncated: bool,
    pub truncation_reason: Option<String>,
    pub warnings: Vec<String>,
    pub generation: u64,
}

/// UI 状态全集。reducer 之外无人可变更；glue 只读它渲染。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Model {
    pub screen: Screen,
    /// Search 屏的编辑中输入。
    pub input: String,
    /// 最近一次提交的查询串——续页 Effect 绑定它，而非编辑中的 `input`。
    pub query: String,
    /// 已累积的命中（`n` 追加下一页，提交新查询时清空）。
    pub hits: Vec<(String, f32)>,
    pub selected: usize,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    /// 最近一次翻页的追加事实（如 `+5`）；首页加载为 `None`。
    pub page_note: Option<String>,
    pub context: Option<ContextView>,
    pub scroll: usize,
    pub policy: ContextPolicy,
    /// 最近一次错误或提示（如 `no hits`）；渲染进状态行，不弹窗、不退出。
    pub status: Option<String>,
    /// 最近一次加载的截断事实（PARTIAL 渲染依据），来自 App 响应。
    pub truncated: bool,
    pub truncation_reason: Option<String>,
    pub warnings: Vec<String>,
    pub quit: bool,
    pub generation: u64,
}

impl Default for Model {
    fn default() -> Self {
        Model {
            screen: Screen::Search,
            input: String::new(),
            query: String::new(),
            hits: Vec::new(),
            selected: 0,
            next_cursor: None,
            has_more: false,
            page_note: None,
            context: None,
            scroll: 0,
            policy: ContextPolicy::Mainline,
            status: None,
            truncated: false,
            truncation_reason: None,
            warnings: Vec::new(),
            quit: false,
            generation: 0,
        }
    }
}

/// reducer 的输入事件：按键，或 glue 执行 Effect 后的回填结果。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Msg {
    Key(KeyInput),
    SearchLoaded(SearchPage),
    ContextLoaded(ContextView),
    /// Effect 执行失败的状态行文本（`error [<code>]: <msg>` 或解析类提示）。
    EffectFailed(String),
}

/// 待 glue 执行的副作用。字段是构造 `AppRequest` 所需的全部事实——
/// core 不接触 App 类型，也绝不在此之外发起数据访问。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Effect {
    /// 检索：`cursor: None` 为新查询首页，`Some` 为 App 发行的续读令牌原样回传。
    Search {
        query: String,
        cursor: Option<String>,
    },
    /// 命中→会话解析 + 上下文装配（Show 取 payload 的 `session` 字段后转 Context）。
    ResolveAndLoadContext {
        hit_id: String,
        policy: ContextPolicy,
    },
    /// 按既知会话 id 重新装配上下文（`f` 切换策略后的 re-fetch）。
    LoadContext {
        session_id: String,
        policy: ContextPolicy,
    },
}

/// 状态转移唯一入口：`(Model, Msg) -> (Model, Option<Effect>)`。纯函数、可单测。
pub(crate) fn update(model: Model, msg: Msg) -> (Model, Option<Effect>) {
    match msg {
        Msg::Key(key) => handle_key(model, key),
        Msg::SearchLoaded(page) => search_loaded(model, page),
        Msg::ContextLoaded(view) => context_loaded(model, view),
        Msg::EffectFailed(text) => effect_failed(model, text),
    }
}

/// 键位表（task design §2，冻结）：全局 Ctrl+C 退出；各屏见 match 各臂。
fn handle_key(mut model: Model, key: KeyInput) -> (Model, Option<Effect>) {
    if key == KeyInput::CtrlC {
        model.quit = true;
        return (model, None);
    }
    match model.screen {
        Screen::Search => match key {
            KeyInput::Char(c) => {
                model.input.push(c);
                (model, None)
            }
            KeyInput::Backspace => {
                model.input.pop();
                (model, None)
            }
            // 空白查询不提交（与 App 的 empty-query 校验对齐，省一次必败请求）。
            KeyInput::Enter => {
                if model.input.trim().is_empty() {
                    return (model, None);
                }
                model.query = model.input.clone();
                model.hits.clear();
                model.selected = 0;
                model.next_cursor = None;
                model.has_more = false;
                model.page_note = None;
                model.status = None;
                model.truncated = false;
                model.truncation_reason = None;
                model.warnings.clear();
                let effect = Effect::Search {
                    query: model.query.clone(),
                    cursor: None,
                };
                (model, Some(effect))
            }
            KeyInput::Esc => {
                if model.input.is_empty() {
                    model.quit = true;
                } else {
                    model.input.clear();
                }
                (model, None)
            }
            _ => (model, None),
        },
        Screen::Results => match key {
            KeyInput::Up => {
                model.selected = model.selected.saturating_sub(1);
                (model, None)
            }
            KeyInput::Down => {
                model.selected = (model.selected + 1).min(model.hits.len().saturating_sub(1));
                (model, None)
            }
            KeyInput::Enter => match model.hits.get(model.selected) {
                Some((id, _)) => {
                    let effect = Effect::ResolveAndLoadContext {
                        hit_id: id.clone(),
                        policy: model.policy,
                    };
                    (model, Some(effect))
                }
                None => (model, None),
            },
            // 翻页 = 把 App 发行的令牌原样回传；无令牌即无下一页，no-op。
            KeyInput::Char('n') => match model.next_cursor.clone() {
                Some(cursor) if model.has_more => {
                    let effect = Effect::Search {
                        query: model.query.clone(),
                        cursor: Some(cursor),
                    };
                    (model, Some(effect))
                }
                _ => (model, None),
            },
            KeyInput::Char('q') => {
                model.quit = true;
                (model, None)
            }
            KeyInput::Esc => {
                model.screen = Screen::Search;
                (model, None)
            }
            _ => (model, None),
        },
        Screen::Context => match key {
            KeyInput::Up => {
                model.scroll = model.scroll.saturating_sub(1);
                (model, None)
            }
            KeyInput::Down => {
                model.scroll = (model.scroll + 1).min(max_scroll(&model));
                (model, None)
            }
            KeyInput::PgUp => {
                model.scroll = model.scroll.saturating_sub(10);
                (model, None)
            }
            KeyInput::PgDn => {
                model.scroll = (model.scroll + 10).min(max_scroll(&model));
                (model, None)
            }
            // 策略切换即重取：分支选择规则在 domain/App，本层只翻开关。
            KeyInput::Char('f') => {
                model.policy = match model.policy {
                    ContextPolicy::Mainline => ContextPolicy::Full,
                    ContextPolicy::Full => ContextPolicy::Mainline,
                };
                match &model.context {
                    Some(view) => {
                        let effect = Effect::LoadContext {
                            session_id: view.session_id.clone(),
                            policy: model.policy,
                        };
                        (model, Some(effect))
                    }
                    None => (model, None),
                }
            }
            KeyInput::Char('q') => {
                model.quit = true;
                (model, None)
            }
            KeyInput::Esc => {
                model.screen = Screen::Results;
                (model, None)
            }
            _ => (model, None),
        },
    }
}

/// 命中加载：追加语义（design §0.7）。首页从空表开始（提交时已清空），
/// 追加页把选中行跳到本页首行；空结果如实报 `no hits`，界面保持可用。
fn search_loaded(mut model: Model, page: SearchPage) -> (Model, Option<Effect>) {
    let prev = model.hits.len();
    let added = page.hits.len();
    model.hits.extend(page.hits);
    let len = model.hits.len();
    model.selected = if prev == 0 || len == 0 {
        0
    } else {
        prev.min(len - 1)
    };
    model.next_cursor = page.next_cursor;
    model.has_more = page.has_more;
    model.generation = page.generation;
    model.truncated = page.truncated;
    model.truncation_reason = page.truncation_reason;
    model.warnings.clear();
    model.page_note = if prev > 0 {
        Some(format!("+{added}"))
    } else {
        None
    };
    model.status = if len == 0 {
        Some("no hits".to_string())
    } else {
        None
    };
    model.screen = Screen::Results;
    (model, None)
}

/// 上下文加载：截断/警告事实提升到 Model（状态行渲染源），滚动复位。
fn context_loaded(mut model: Model, view: ContextView) -> (Model, Option<Effect>) {
    model.generation = view.generation;
    model.truncated = view.truncated;
    model.truncation_reason = view.truncation_reason.clone();
    model.warnings = view.warnings.clone();
    model.context = Some(view);
    model.scroll = 0;
    model.status = None;
    model.screen = Screen::Context;
    (model, None)
}

/// Effect 失败只进状态行：UI 继续运行、不换屏、不崩（PRD R4）。
fn effect_failed(mut model: Model, text: String) -> (Model, Option<Effect>) {
    model.status = Some(text);
    (model, None)
}

/// Context 屏滚动上界：最后一行仍可见（无内容时为 0）。
fn max_scroll(model: &Model) -> usize {
    model
        .context
        .as_ref()
        .map(|view| view.lines.len().saturating_sub(1))
        .unwrap_or(0)
}

/// 命中列表行：选中行前缀 `> `，其余两空格对齐。
pub(crate) fn hit_lines(model: &Model) -> Vec<String> {
    model
        .hits
        .iter()
        .enumerate()
        .map(|(i, (id, score))| {
            let prefix = if i == model.selected { "> " } else { "  " };
            format!("{prefix}{id}  score {score:.3}")
        })
        .collect()
}

/// Context 行：`role: 文本首行 [precision]`；unknown 精度如实渲染 `[unknown]`。
pub(crate) fn context_lines(model: &Model) -> Vec<String> {
    match &model.context {
        None => Vec::new(),
        Some(view) => view
            .lines
            .iter()
            .map(|message| {
                let first = message.text.lines().next().unwrap_or("");
                format!("{}: {} [{}]", message.role, first, message.precision)
            })
            .collect(),
    }
}

/// 状态行：generation + 截断（`PARTIAL: <预算旋钮>`）+ 首条警告 + 最近错误/提示。
/// 诚实渲染是硬要求（PRD R3）：截断与降级绝不吞掉。
pub(crate) fn status_line(model: &Model) -> String {
    let mut parts = vec![format!("gen {}", model.generation)];
    if model.truncated {
        let reason = model.truncation_reason.as_deref().unwrap_or("unspecified");
        parts.push(format!("PARTIAL: {reason}"));
    }
    if let Some(warning) = model.warnings.first() {
        parts.push(format!("warning: {warning}"));
    }
    if let Some(status) = &model.status {
        parts.push(status.clone());
    }
    parts.join(" | ")
}

/// 标题行：当前屏 + 键位提示 + 诚实的分页/策略事实。
pub(crate) fn title_line(model: &Model) -> String {
    match model.screen {
        Screen::Search => "Search - type query, Enter: run, Esc: clear/quit".to_string(),
        Screen::Results => {
            let more = if model.has_more { "  n: next page" } else { "" };
            let note = model
                .page_note
                .as_deref()
                .map(|n| format!("  [{n}]"))
                .unwrap_or_default();
            format!(
                "Results - {} hits{more}{note}  Enter: open, Esc: back, q: quit",
                model.hits.len()
            )
        }
        Screen::Context => {
            let policy = match model.policy {
                ContextPolicy::Mainline => "mainline",
                ContextPolicy::Full => "full",
            };
            let session = model
                .context
                .as_ref()
                .map(|view| view.session_id.as_str())
                .unwrap_or("-");
            format!("Context {session} - policy {policy}  f: toggle, Esc: back, q: quit")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(model: Model, input: KeyInput) -> (Model, Option<Effect>) {
        update(model, Msg::Key(input))
    }

    fn typed(model: Model, text: &str) -> Model {
        text.chars().fold(model, |m, c| key(m, KeyInput::Char(c)).0)
    }

    /// 已输入 "rust" 并提交（hits 已清空、Effect 已发出）的 Search 屏模型。
    fn submitted(query: &str) -> Model {
        let (model, effect) = key(typed(Model::default(), query), KeyInput::Enter);
        assert!(effect.is_some(), "non-empty submit must emit an effect");
        model
    }

    fn page(hits: &[(&str, f32)], cursor: Option<&str>) -> SearchPage {
        SearchPage {
            hits: hits
                .iter()
                .map(|(id, score)| ((*id).to_string(), *score))
                .collect(),
            next_cursor: cursor.map(str::to_string),
            has_more: cursor.is_some(),
            generation: 7,
            truncated: false,
            truncation_reason: None,
        }
    }

    /// 提交 "rust" 后加载一页命中的 Results 屏模型。
    fn results(hits: &[(&str, f32)], cursor: Option<&str>) -> Model {
        let loaded = Msg::SearchLoaded(page(hits, cursor));
        update(submitted("rust"), loaded).0
    }

    fn view(session: &str, lines: usize) -> ContextView {
        ContextView {
            session_id: session.to_string(),
            lines: (0..lines)
                .map(|i| ContextMessage {
                    role: "user".to_string(),
                    text: format!("m{i}"),
                    precision: "byte".to_string(),
                })
                .collect(),
            truncated: false,
            truncation_reason: None,
            warnings: Vec::new(),
            generation: 7,
        }
    }

    fn in_context(session: &str, lines: usize) -> Model {
        let loaded = Msg::ContextLoaded(view(session, lines));
        update(results(&[("msg_v1_a", 2.0)], None), loaded).0
    }

    // ---- reducer：Search 屏 ----

    #[test]
    fn typing_and_backspace_edit_input() {
        let model = typed(Model::default(), "abc");
        assert_eq!(model.input, "abc");
        let (model, effect) = key(model, KeyInput::Backspace);
        assert_eq!(model.input, "ab");
        assert!(effect.is_none());
    }

    #[test]
    fn enter_on_empty_input_is_noop() {
        let (model, effect) = key(Model::default(), KeyInput::Enter);
        assert!(effect.is_none());
        assert_eq!(model.screen, Screen::Search);
        // 纯空白同样不提交（App 会拒绝空查询，这里不发必败请求）。
        let (model, effect) = key(typed(Model::default(), "   "), KeyInput::Enter);
        assert!(effect.is_none());
        assert_eq!(model.input, "   ");
    }

    #[test]
    fn enter_submits_search_and_resets_hits() {
        let mut model = typed(Model::default(), "rust");
        model.hits = vec![("msg_v1_old".to_string(), 1.0)];
        model.next_cursor = Some("stale".to_string());
        model.has_more = true;
        let (model, effect) = key(model, KeyInput::Enter);
        assert_eq!(
            effect,
            Some(Effect::Search {
                query: "rust".to_string(),
                cursor: None,
            })
        );
        assert!(model.hits.is_empty());
        assert_eq!(model.query, "rust");
        assert_eq!(model.selected, 0);
        assert!(model.next_cursor.is_none());
        assert!(!model.has_more);
    }

    #[test]
    fn esc_on_search_clears_then_quits() {
        let (model, _) = key(typed(Model::default(), "rust"), KeyInput::Esc);
        assert_eq!(model.input, "");
        assert!(!model.quit);
        let (model, _) = key(model, KeyInput::Esc);
        assert!(model.quit);
    }

    // ---- reducer：SearchLoaded / 翻页 ----

    #[test]
    fn search_loaded_populates_and_moves_to_results() {
        let (model, effect) = update(
            submitted("rust"),
            Msg::SearchLoaded(page(&[("msg_v1_a", 2.0), ("msg_v1_b", 1.0)], Some("tok1"))),
        );
        assert!(effect.is_none());
        assert_eq!(model.screen, Screen::Results);
        assert_eq!(model.hits.len(), 2);
        assert_eq!(model.selected, 0);
        assert_eq!(model.next_cursor.as_deref(), Some("tok1"));
        assert!(model.has_more);
        assert_eq!(model.generation, 7);
        assert!(model.status.is_none());
    }

    #[test]
    fn search_loaded_empty_page_reports_no_hits_and_stays_functional() {
        let loaded = Msg::SearchLoaded(page(&[], None));
        let (model, _) = update(submitted("nope"), loaded);
        assert_eq!(model.screen, Screen::Results);
        assert!(model.hits.is_empty());
        assert_eq!(model.status.as_deref(), Some("no hits"));
        // 空列表上导航/打开都是 no-op，Esc 仍可回到 Search。
        let (model, effect) = key(model, KeyInput::Down);
        assert_eq!(model.selected, 0);
        assert!(effect.is_none());
        let (model, effect) = key(model, KeyInput::Enter);
        assert!(effect.is_none());
        let (model, _) = key(model, KeyInput::Esc);
        assert_eq!(model.screen, Screen::Search);
    }

    #[test]
    fn next_page_emits_search_with_stored_cursor() {
        let model = results(&[("msg_v1_a", 2.0)], Some("tok1"));
        let (_, effect) = key(model, KeyInput::Char('n'));
        assert_eq!(
            effect,
            Some(Effect::Search {
                query: "rust".to_string(),
                cursor: Some("tok1".to_string()),
            })
        );
    }

    #[test]
    fn search_loaded_appends_and_selects_first_new_row() {
        let model = results(&[("msg_v1_a", 2.0), ("msg_v1_b", 1.5)], Some("tok1"));
        let (model, _) = key(model, KeyInput::Char('n'));
        let loaded = Msg::SearchLoaded(page(&[("msg_v1_c", 1.0)], None));
        let (model, _) = update(model, loaded);
        let ids: Vec<&str> = model.hits.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["msg_v1_a", "msg_v1_b", "msg_v1_c"]);
        assert_eq!(model.selected, 2, "selection jumps to first new row");
        assert!(!model.has_more);
        assert!(model.next_cursor.is_none());
        assert_eq!(model.page_note.as_deref(), Some("+1"));
    }

    #[test]
    fn next_page_without_more_is_noop() {
        let model = results(&[("msg_v1_a", 2.0)], None);
        let (model, effect) = key(model, KeyInput::Char('n'));
        assert!(effect.is_none());
        assert_eq!(model.hits.len(), 1);
    }

    // ---- reducer：Results 导航 ----

    #[test]
    fn selection_saturates_at_list_edges() {
        let model = results(&[("msg_v1_a", 2.0), ("msg_v1_b", 1.0)], None);
        let (model, _) = key(model, KeyInput::Up);
        assert_eq!(model.selected, 0, "Up saturates at the top");
        let (model, _) = key(model, KeyInput::Down);
        assert_eq!(model.selected, 1);
        let (model, _) = key(model, KeyInput::Down);
        assert_eq!(model.selected, 1, "Down saturates at the bottom");
        // 空列表：两个方向都停在 0。
        let empty = results(&[], None);
        let (empty, _) = key(empty, KeyInput::Down);
        assert_eq!(empty.selected, 0);
        let (empty, _) = key(empty, KeyInput::Up);
        assert_eq!(empty.selected, 0);
    }

    #[test]
    fn enter_on_hit_resolves_selected_id() {
        let model = results(&[("msg_v1_a", 2.0), ("msg_v1_b", 1.0)], None);
        let (model, _) = key(model, KeyInput::Down);
        let (_, effect) = key(model, KeyInput::Enter);
        assert_eq!(
            effect,
            Some(Effect::ResolveAndLoadContext {
                hit_id: "msg_v1_b".to_string(),
                policy: ContextPolicy::Mainline,
            })
        );
    }

    // ---- reducer：Context 屏 ----

    #[test]
    fn context_loaded_switches_screen_with_scroll_reset() {
        let mut model = results(&[("msg_v1_a", 2.0)], None);
        model.scroll = 9;
        let (model, effect) = update(model, Msg::ContextLoaded(view("ses_v1_s", 3)));
        assert!(effect.is_none());
        assert_eq!(model.screen, Screen::Context);
        assert_eq!(model.scroll, 0);
        assert!(model.context.is_some());
    }

    #[test]
    fn policy_toggle_refetches_context() {
        let (model, effect) = key(in_context("ses_v1_s", 3), KeyInput::Char('f'));
        assert_eq!(model.policy, ContextPolicy::Full);
        assert_eq!(
            effect,
            Some(Effect::LoadContext {
                session_id: "ses_v1_s".to_string(),
                policy: ContextPolicy::Full,
            })
        );
        let (model, effect) = key(model, KeyInput::Char('f'));
        assert_eq!(model.policy, ContextPolicy::Mainline);
        assert_eq!(
            effect,
            Some(Effect::LoadContext {
                session_id: "ses_v1_s".to_string(),
                policy: ContextPolicy::Mainline,
            })
        );
    }

    #[test]
    fn context_scroll_saturates() {
        let model = in_context("ses_v1_s", 5);
        let (model, _) = key(model, KeyInput::Up);
        assert_eq!(model.scroll, 0, "Up saturates at the top");
        let (model, _) = key(model, KeyInput::PgDn);
        assert_eq!(model.scroll, 4, "PgDn saturates at the last line");
        let (model, _) = key(model, KeyInput::Down);
        assert_eq!(model.scroll, 4, "Down saturates at the last line");
        let (model, _) = key(model, KeyInput::PgUp);
        assert_eq!(model.scroll, 0);
    }

    // ---- reducer：退出与屏间转移 ----

    #[test]
    fn esc_chain_context_results_search_quit() {
        let model = in_context("ses_v1_s", 1);
        let (model, _) = key(model, KeyInput::Esc);
        assert_eq!(model.screen, Screen::Results);
        let (model, _) = key(model, KeyInput::Esc);
        assert_eq!(model.screen, Screen::Search);
        // 提交过的输入仍在：先清空，再退出。
        let (model, _) = key(model, KeyInput::Esc);
        assert_eq!(model.input, "");
        assert!(!model.quit);
        let (model, _) = key(model, KeyInput::Esc);
        assert!(model.quit);
    }

    #[test]
    fn q_quits_on_results_and_context_but_types_on_search() {
        let results_model = results(&[("msg_v1_a", 1.0)], None);
        let (model, _) = key(results_model, KeyInput::Char('q'));
        assert!(model.quit);
        let (model, _) = key(in_context("ses_v1_s", 1), KeyInput::Char('q'));
        assert!(model.quit);
        let (model, _) = key(Model::default(), KeyInput::Char('q'));
        assert!(!model.quit);
        assert_eq!(model.input, "q", "q must type into the search input");
    }

    #[test]
    fn ctrl_c_quits_everywhere() {
        for model in [
            Model::default(),
            results(&[("msg_v1_a", 1.0)], None),
            in_context("ses_v1_s", 1),
        ] {
            let (model, effect) = key(model, KeyInput::CtrlC);
            assert!(model.quit);
            assert!(effect.is_none());
        }
    }

    #[test]
    fn effect_failed_sets_status_and_keeps_screen() {
        let model = results(&[("msg_v1_a", 1.0)], None);
        let failure = Msg::EffectFailed("error [not_found]: gone".to_string());
        let (model, effect) = update(model, failure);
        assert!(effect.is_none());
        assert_eq!(model.screen, Screen::Results);
        assert_eq!(model.status.as_deref(), Some("error [not_found]: gone"));
    }

    // ---- view-model ----

    #[test]
    fn hit_lines_prefix_selected_row() {
        let mut model = results(&[("msg_v1_a", 2.0), ("msg_v1_b", 1.0)], None);
        model.selected = 1;
        let lines = hit_lines(&model);
        assert!(lines[0].starts_with("  msg_v1_a"), "{lines:?}");
        assert!(lines[1].starts_with("> msg_v1_b"), "{lines:?}");
    }

    #[test]
    fn context_lines_render_precision_markers() {
        let context = ContextView {
            session_id: "ses_v1_s".to_string(),
            lines: vec![
                ContextMessage {
                    role: "user".to_string(),
                    text: "hello\nworld".to_string(),
                    precision: "byte".to_string(),
                },
                ContextMessage {
                    role: "assistant".to_string(),
                    text: "hi".to_string(),
                    precision: "unknown".to_string(),
                },
            ],
            truncated: false,
            truncation_reason: None,
            warnings: Vec::new(),
            generation: 7,
        };
        let model = Model {
            context: Some(context),
            ..Model::default()
        };
        let lines = context_lines(&model);
        // 只取文本首行；unknown 精度如实渲染 [unknown]。
        assert_eq!(lines[0], "user: hello [byte]");
        assert_eq!(lines[1], "assistant: hi [unknown]");
    }

    #[test]
    fn status_line_reports_partial_truncation() {
        let model = Model {
            generation: 7,
            truncated: true,
            truncation_reason: Some("max_items".to_string()),
            ..Model::default()
        };
        let status = status_line(&model);
        assert!(status.contains("gen 7"), "{status}");
        assert!(status.contains("PARTIAL: max_items"), "{status}");
    }

    #[test]
    fn status_line_renders_error_and_warning() {
        let model = Model {
            status: Some("error [cursor_expired]: cursor expired".to_string()),
            ..Model::default()
        };
        assert!(status_line(&model).contains("error [cursor_expired]"));
        let model = Model {
            warnings: vec!["2 of 3 evidence spans have unknown precision".to_string()],
            ..Model::default()
        };
        assert!(status_line(&model).contains("warning: 2 of 3"));
    }
}
