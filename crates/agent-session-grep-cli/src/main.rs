//! agent-session-grep CLI：组合根（composition root）。
//!
//! 本 crate 是唯一把抽象端口与具体 adapter 绑定的地方——它 `new` 出 [`SqliteStore`]
//! 并注入 [`App`]，其余各层对具体后端一无所知；分层依赖保持
//! domain ← ports ← application ← adapters。
//! 首个垂直切片只暴露两个子命令，用于端到端打通 discovery→catalog→search 骨架：
//!
//! ```text
//! agent-session-grep --db <path> sync --discover           # 建立索引
//! agent-session-grep --db <path> search <query>           # 全文检索
//! agent-session-grep --db <path> get <wire-id>            # 按 id 取回 payload
//! ```
//!
//! `index <id-fact> <text>` 是切片期留下的直写入口，绕过 provider 解析，需
//! `--force-dev` 且不在用户 help 里公布。
//!
//! 参数解析刻意手写、不引第三方 CLI 框架——切片阶段只需最小可用面。
//! 输出走 Robot-JSON 雏形（每行一个 JSON 对象），为后续 CONTRACT 对齐留口。

mod hooks;
mod human;
mod mcp;
mod protocol;
mod redaction;
mod serve;
mod tui;

use agent_session_grep_adapters_sqlite::{
    SourceActivity, SourceBatch, SqliteStore, capture, open_snapshot_source, verify_snapshot,
};
use agent_session_grep_application::{
    App, AppError, AppRequest, AppResponse, ContextLevel, ResponseBudget, SourceRejection,
    StagedBatch, Truncation, evidence::Precision, handoff_pack::HandoffInput,
    parse_relative_search_instant, parse_search_instant, select_and_stage_source, source_rejection,
};
use agent_session_grep_domain::{
    ContextPolicy, DomainError, EvidenceSpan, IdKind, MessageEdge, MessagePlacement,
    MessageRelation, SessionIdentityNamespace, Stability, StableId,
};
use agent_session_grep_ports::{
    Confidence, ParseReport, ProviderAdapter, ProviderSessionObservation, ReadOnlySource,
    RedactionStatus, ResumeClaimsStore, RetrievalMode, SearchFacets, SearchFilters, SearchInstant,
    SearchProvider, SidechainFacet, SourceFormatFamily, SourceResumeClaim,
    capability::{ProviderCapability, ProviderCapabilityMatrix, ProviderMaturity},
    source_format_family_for,
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
use protocol::{CanonicalCode, ProtocolError};
use std::collections::{BTreeMap, BTreeSet};

/// Provider parse diagnostics exposed through the existing success-envelope
/// `warnings` channel are bounded at the CLI boundary. This keeps a badly
/// damaged source from producing an unbounded response while preserving the
/// actionable line/session detail required by sync diagnostics.
const DIAGNOSTIC_WARNING_LIMIT: usize = 16;
const DIAGNOSTIC_WARNING_CHARS: usize = 512;

/// CLI 顶层错误：所有失败都归一到 [`ProtocolError`]，exit code 由 Error Catalog 决定。
///
/// `Usage` 保留为薄封装，仅表示参数校验失败（映射 `invalid_request` → exit 2），
/// 使用法错误与业务错误遵循 CLI/Robot/MCP contract 的同一 envelope/退出码映射。
#[derive(Debug)]
struct CliError(ProtocolError);

impl CliError {
    /// 参数或请求校验失败（exit 2）。
    fn usage(msg: impl Into<String>) -> Self {
        CliError(ProtocolError::new(CanonicalCode::InvalidRequest, msg))
    }
}

impl From<DomainError> for CliError {
    fn from(e: DomainError) -> Self {
        CliError(e.into())
    }
}

impl From<AppError> for CliError {
    fn from(e: AppError) -> Self {
        CliError(e.into())
    }
}

impl From<ProtocolError> for CliError {
    fn from(e: ProtocolError) -> Self {
        CliError(e)
    }
}

fn main() {
    // command 名先解析出来供错误 envelope 使用；失败时也要标注是哪个命令。
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = command_name(&args);
    let mode = match protocol::parse_output_mode(&args) {
        Ok(mode) => mode,
        Err(message) => {
            let err = ProtocolError::new(CanonicalCode::InvalidRequest, message);
            protocol::write_stdout_line(&protocol::error_envelope(&command, &err, None));
            std::process::exit(err.code.exit_code());
        }
    };
    // --request-id：robot 调用方的关联 id。非法值是用法错误（exit 2），
    // 不静默替换为生成 id——那会让调用方以为关联成功。
    let request_id = match extract_request_id(&args) {
        Ok(id) => id,
        Err(message) => {
            let err = ProtocolError::new(CanonicalCode::InvalidRequest, message);
            match mode {
                protocol::OutputMode::Human => {
                    render_human_error(&err);
                }
                protocol::OutputMode::Json | protocol::OutputMode::Jsonl => {
                    protocol::write_stdout_line(&protocol::error_envelope(&command, &err, None));
                }
            }
            std::process::exit(err.code.exit_code());
        }
    };
    match run(
        &args,
        mode,
        request_id.as_deref(),
        extract_offline_flag(&args),
    ) {
        Ok(protocol::Outcome::Success) => {}
        // 部分成功（预算截断）按 contract §5 exit 10——结果可用但不完整，不伪装 success。
        Ok(protocol::Outcome::Partial) => std::process::exit(10),
        Err(CliError(err)) => {
            // 错误 envelope 只写 stdout 一个对象；进程级诊断（人类模式）走 stderr。
            match mode {
                protocol::OutputMode::Human => {
                    render_human_error(&err);
                }
                protocol::OutputMode::Json | protocol::OutputMode::Jsonl => {
                    protocol::write_stdout_line(&protocol::error_envelope(
                        &command,
                        &err,
                        request_id.as_deref(),
                    ));
                }
            }
            std::process::exit(err.code.exit_code());
        }
    }
}

/// 人类模式的错误渲染：报错行（保留稳定 code 前缀）+ 一行白话"下一步"指引
/// （error catalog 的 `operator_action` 面向新手落地）。robot/json 模式保持
/// 稳定 envelope，不受影响。
///
/// 消息自己已经给出可执行的下一步时不再追加通用指引：`operator_action` 是逐
/// code 的**通用**兜底，遇到更具体的场景就会说错话。库不存在时 `not_found`
/// 的通用指引是"确认实体 ID 是否正确（运行 list 可浏览可用实体）"——既答错了
/// 问题（缺的是库不是 id），又推荐了一条同样需要库的命令。
fn render_human_error(err: &ProtocolError) {
    eprintln!("error [{}]: {}", err.code.as_str(), err.message);
    if !message_states_next_step(&err.message) {
        eprintln!("下一步：{}", err.code.operator_action());
    }
}

/// 错误消息是否已自带可执行的下一步命令。判据是消息里出现了本二进制的调用
/// 形态——只有构造方明确写了完整命令时才成立，不做模糊猜测。
fn message_states_next_step(message: &str) -> bool {
    message.contains("asg ") || message.contains("agent-session-grep ")
}

/// Convert provider parse diagnostics into bounded public warnings. Diagnostics
/// are source-derived and may include line numbers or bounded session IDs, but
/// never source paths; each warning is independently clamped before rendering.
fn diagnostic_warnings<'a>(
    diagnostics: impl IntoIterator<Item = &'a str>,
    total: usize,
) -> Vec<String> {
    let detail_limit = if total > DIAGNOSTIC_WARNING_LIMIT {
        DIAGNOSTIC_WARNING_LIMIT.saturating_sub(1)
    } else {
        DIAGNOSTIC_WARNING_LIMIT
    };
    let mut warnings: Vec<String> = diagnostics
        .into_iter()
        .take(detail_limit)
        .map(|diagnostic| {
            let mut bounded: String = diagnostic.chars().take(DIAGNOSTIC_WARNING_CHARS).collect();
            if diagnostic.chars().count() > DIAGNOSTIC_WARNING_CHARS {
                bounded.push('…');
            }
            bounded
        })
        .collect();
    if total > detail_limit {
        warnings.push(format!(
            "{} additional provider diagnostics omitted",
            total - detail_limit
        ));
    }
    warnings
}

/// 从参数抽出 `--offline`（裸 flag，无取值）：只在全局 flag 前缀位置识别。
/// 缺省 false；命令名之后的同名 token 是位置参数，不当 flag 解析。重复出现
/// 与其它裸 flag（`--discover`/`--include-system`）一致，按在场一次处理。
fn extract_offline_flag(args: &[String]) -> bool {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !a.starts_with('-') {
            return false; // 已到命令名：之后的 token 不当 flag 解析
        }
        if a == "--offline" {
            return true;
        }
        // 其它带值 flag 跳过其取值，避免把取值误当命令名。
        match a.as_str() {
            "--db" | "--output" | "--request-id" | "--cursor" | "--max-items" | "--max-bytes"
            | "--max-messages" | "--policy" | "--level" | "--provider" | "--since" | "--until"
            | "--session" | "--around" => {
                it.next();
            }
            _ => {}
        }
    }
    false
}

/// 从参数抽出 `--request-id`：缺 flag → None；有 flag 则值必须满足 envelope
/// 约束（`^[A-Za-z0-9._:-]+$`，1..=128），否则是用法错误。
///
/// 与 [`protocol::parse_output_mode`] 同一规则：flag 只在前缀位置（第一个位置
/// 参数之前）识别——命令名/查询文本恰等于 `--request-id` 时按查询走，不得误判。
/// 取值为已知 flag 名（`--request-id --robot` 会把 flag 当取值，且 `--robot`
/// 恰好能通过 id 字符集校验）与重复 `--request-id`（不再静默 first-wins）
/// 都是用法错误（R8.1/R8.2）。
fn extract_request_id(args: &[String]) -> Result<Option<String>, String> {
    let mut it = args.iter();
    let mut seen: Option<String> = None;
    while let Some(a) = it.next() {
        if !a.starts_with('-') {
            return Ok(seen); // 已到命令名：之后的 token 不当 flag 解析
        }
        if a == "--request-id" {
            let value = it.next().ok_or("--request-id requires a value")?;
            if is_known_flag_name(value) {
                return Err(format!(
                    "--request-id requires a value, got {value:?} (a flag name)"
                ));
            }
            if !protocol::valid_request_id(value) {
                return Err(format!(
                    "--request-id must match ^[A-Za-z0-9._:-]+$ (1..=128 chars), got {value:?}"
                ));
            }
            if seen.is_some() {
                return Err("duplicate --request-id".into());
            }
            seen = Some(value.clone());
        }
        // 其它带值 flag 跳过其取值，避免把取值误当位置参数提前终止扫描。
        match a.as_str() {
            "--db" | "--output" | "--cursor" | "--max-items" | "--max-bytes" | "--max-messages"
            | "--max-evidence" | "--max-tokens" | "--policy" | "--level" | "--provider"
            | "--since" | "--until" | "--session" | "--around" | "--tool-kind" | "--tool-name" => {
                it.next();
            }
            _ => {}
        }
    }
    Ok(seen)
}

/// 从参数里解出子命令名（用于错误 envelope 的 `command` 字段）。
///
/// 跳过已知 flag（带值的连同其取值）；第一个既不是已知 flag、也不是已知 flag
/// 取值的 token 即命令名。未知的 `-` 开头 token 不是 flag——它是命令名笔误
/// （如 `--bogus`），错误 envelope 的 `command` 必须指向它而不是后面的真命令
/// （R8.4：`--bogus --robot status` 的失败者是 `--bogus`，不是 `status`）。
fn command_name(args: &[String]) -> String {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" | "--output" | "--cursor" | "--max-items" | "--max-bytes" | "--max-messages"
            | "--max-evidence" | "--max-tokens" | "--policy" | "--level" | "--request-id"
            | "--provider" | "--since" | "--until" | "--session" | "--around" | "--tool-kind"
            | "--tool-name" => {
                it.next(); // 消费其取值
            }
            "--robot" | "--no-color" | "--help" | "-h" | "--version" | "-V" | "--discover"
            | "--offline" => {}
            s => return s.to_string(),
        }
    }
    "unknown".into()
}

/// help/version 拦截结果（ADR-0006）：在 `--db` 解析与存储打开之前识别。
#[derive(Debug, Clone, PartialEq, Eq)]
enum HelpRequest {
    /// 顶层 `--help` / `-h`（无子命令）。
    TopLevelHelp,
    /// 顶层 `--version` / `-V`。
    TopLevelVersion,
    /// `<cmd> --help` / `-h`（含 `index rebuild --help`）。
    SubcommandHelp(String),
}

/// 提前拦截 help/version（ADR-0006）：只在全局 flag 前缀位置之后识别。
///
/// - 全为前缀 flag（无命令名）时，`--help`/`-h` → 顶层帮助，`--version`/`-V`
///   → 版本；两者同时出现时帮助优先（与旧行为一致）。
/// - 命令名紧跟 `--help`/`-h` → 该子命令帮助（`index rebuild --help` 也覆盖）；
///   `search foo --help` 里更靠后的 `--help` 不在此位——不得拦截，交给命令层
///   按位置参数处理（search 只接受一个查询词，多余 token 是 usage error）。
/// - 未知命令（不在 [`known_subcommand`]）后的 `--help` 不拦截——交给 dispatch
///   报 unknown subcommand，而不是给出误导性帮助。
fn intercept_help_or_version(args: &[String]) -> Option<HelpRequest> {
    let mut it = args.iter();
    let mut prefix_help = false;
    let mut prefix_version = false;
    while let Some(token) = it.next() {
        if !token.starts_with('-') {
            // 第一个裸 token 即命令名：紧随其后的 --help/-h 是子命令帮助。
            let cmd = token.as_str();
            match it.next().map(String::as_str) {
                Some("--help") | Some("-h") if known_subcommand(cmd) => {
                    return Some(HelpRequest::SubcommandHelp(cmd.to_string()));
                }
                // index rebuild|embeddings --help：rebuild/embeddings 是 index 的
                // 子词，帮助旗标跟在它们后面。
                Some("rebuild") | Some("embeddings") if cmd == "index" => {
                    if matches!(it.next().map(String::as_str), Some("--help") | Some("-h")) {
                        return Some(HelpRequest::SubcommandHelp("index".into()));
                    }
                }
                // model import|status --help：import/status 是 model 的子词，
                // 帮助旗标跟在它们后面（与 index rebuild --help 同规则）。
                Some("import") | Some("status") if cmd == "model" => {
                    if matches!(it.next().map(String::as_str), Some("--help") | Some("-h")) {
                        return Some(HelpRequest::SubcommandHelp("model".into()));
                    }
                }
                _ => {}
            }
            // 命令已出现且帮助旗标不在紧跟位：不拦截，按正常命令/查询走。
            return None;
        }
        match token.as_str() {
            "--help" | "-h" => prefix_help = true,
            "--version" | "-V" => prefix_version = true,
            // 裸 flag（无取值）不改变拦截判定：--robot/--no-color/--offline 等同理。
            "--robot" | "--no-color" | "--discover" | "--offline" => {}
            // 带值 flag 跳过其取值，避免把取值误当命令名。
            "--db" | "--output" | "--request-id" | "--cursor" | "--max-items" | "--max-bytes"
            | "--max-messages" | "--max-evidence" | "--max-tokens" | "--policy" | "--level"
            | "--provider" | "--since" | "--until" | "--session" | "--around" | "--tool-kind"
            | "--tool-name" => {
                it.next();
            }
            _ => {}
        }
    }
    if prefix_help {
        Some(HelpRequest::TopLevelHelp)
    } else if prefix_version {
        Some(HelpRequest::TopLevelVersion)
    } else {
        None
    }
}

fn run(
    args: &[String],
    mode: protocol::OutputMode,
    request_id: Option<&str>,
    offline: bool,
) -> Result<protocol::Outcome, CliError> {
    let started = std::time::Instant::now();
    // help/version 提前拦截（ADR-0006）：在解析 --db、打开存储、special-command
    // 分发（doctor/config/mcp/tui）之前处理——`<cmd> --help` 不要求 --db，任何
    // help 路径都不创建数据库、不抢 writer lease、无文件副作用。
    // 语义：--help/-h/--version 紧跟命令名（对 index 可跟在 rebuild 后）才是
    // 子命令帮助；命令名之后更靠后的同名 token 不得拦截——由命令层按位置参数
    // 处理（search 只接受一个查询词，`search foo --help` 因此是多余参数
    // usage error，`search --help` 则始终是帮助拦截）。查询文本恰等于 flag 名
    // 的检索（`search --robot` / `search --output`）只发生在 help 旗标不在
    // 命令名紧跟位时。
    if let Some(intercept) = intercept_help_or_version(args) {
        match intercept {
            HelpRequest::TopLevelHelp => emit_help("help", &help_text(), mode, request_id),
            HelpRequest::TopLevelVersion => emit_version(
                "version",
                &format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
                mode,
                request_id,
            ),
            HelpRequest::SubcommandHelp(cmd) => {
                emit_help(&cmd, subcommand_help_text(&cmd), mode, request_id);
            }
        }
        return Ok(protocol::Outcome::Success);
    }
    if command_name(args) == "doctor" {
        return doctor(args, mode, request_id, offline);
    }
    if command_name(args) == "config" {
        let positionals = bare_positionals(args);
        if positionals == ["config".to_string(), "paths".to_string()] {
            return config_paths(mode, request_id);
        }
        if positionals.first().map(String::as_str) == Some("config") && positionals.len() > 2 {
            return Err(CliError::usage(
                "config paths takes no additional arguments",
            ));
        }
        return Err(CliError::usage(
            "config paths is the only supported config command",
        ));
    }

    if command_name(args) == "providers" {
        if bare_positionals(args).len() > 1 {
            return Err(CliError::usage("providers takes no positional arguments"));
        }
        return providers(mode, request_id);
    }

    // model import/status: offline-only model cache management. Does not open
    // the catalog DB. Import requires a semantic-candle build and refuses with
    // capability_not_supported otherwise (honest, never stages silently);
    // status works in every build and reports the default bundle state.
    if command_name(args) == "model" {
        return model_command(args, mode, request_id, offline);
    }

    let (db_flag, rest) = parse_db_flag(args)?;
    // 未知子命令在打开存储之前就拒绝：`asg handof q` 必须报 did-you-mean，
    // 而不是先因为缺库报 not_found（错的那层）。列表由 KNOWN_SUBCOMMANDS 派生。
    match rest.first() {
        None => return Err(missing_subcommand_error()),
        Some(cmd) if !known_subcommand(cmd) => {
            return Err(unknown_subcommand_error(cmd));
        }
        Some(_) => {}
    }
    let (db, db_origin) = resolve_store_path(db_flag)?;
    // 写入子命令抢 data-root writer lease；读路径不抢，允许多读者并发。
    let writes = rest
        .first()
        .map(|c| matches!(c.as_str(), "index" | "ingest" | "sync"))
        .unwrap_or(false);
    let store = if writes {
        // 写路径负责把 data root 建出来：默认库落在平台数据目录下，而那个目录
        // 在首次写入前并不存在（旧行为是让 SQLite 报 catalog_error 并倒过来
        // 怪用户的路径写法）。
        create_store_parent_dir(&db)?;
        SqliteStore::open_for_write(&db)
    } else {
        // 读路径绝不建库（M2P-2）：`--db <typo>` 或未同步过的默认库过去会得到
        // 一个"干净的零命中"外加一个新建的空库文件——最容易的静默失败。
        require_existing_store(&db, db_origin)?;
        SqliteStore::open(&db)
    }
    .map_err(ProtocolError::from)?;
    // mcp：stdio JSON-RPC 服务接管整个 stdout（MCP framing 即协议），不走
    // dispatch/emit_result；--output/--robot/--request-id 对其无意义（design §0.6）。
    if rest.first().map(String::as_str) == Some("mcp") {
        if rest.len() > 1 {
            return Err(CliError::usage("mcp takes no positional arguments"));
        }
        return mcp::serve(&store);
    }
    // tui：交互式只读浏览（Preview）。同 mcp 一样接管终端，不走 dispatch/
    // emit_result；输出模式 flag 对其无意义（design §0.6）。
    // `tui --snapshot-json <query>` 是无终端的 headless 结构投影，供 release
    // 一致性 harness 复用同一 Application 搜索路径做跨入口比对。
    if rest.first().map(String::as_str) == Some("tui") {
        let mut tui_args = rest[1..].to_vec();
        let snapshot_query = extract_flag(&mut tui_args, "--snapshot-json")?;
        if let Some(query) = snapshot_query {
            if !tui_args.is_empty() {
                return Err(CliError::usage(
                    "tui --snapshot-json <query> takes no additional arguments",
                ));
            }
            let snapshot = tui::snapshot_search(&store, query)?;
            protocol::write_stdout_line(&snapshot.to_string());
            return Ok(protocol::Outcome::Success);
        }
        if !tui_args.is_empty() {
            return Err(CliError::usage("tui takes no positional arguments"));
        }
        return tui::run(&store);
    }
    // serve：loopback HTTP + 嵌入式 Web UI。同样接管（长期运行），不走
    // dispatch/emit_result；--output/--robot 无意义。`--port <n>` 可选（默认 0 = 随机端口）。
    if rest.first().map(String::as_str) == Some("serve") {
        let mut args = rest[1..].to_vec();
        let port_value = extract_flag(&mut args, "--port")?;
        let lan_requested = take_bool_flag(&mut args, "--lan");
        if lan_requested {
            return Err(CliError::usage(
                "serve --lan: capability_not_supported; this release is loopback-only",
            ));
        }
        if !args.is_empty() {
            return Err(CliError::usage("serve takes no positional arguments"));
        }
        let port = port_value
            .as_deref()
            .map(|v| {
                v.parse::<u16>()
                    .map_err(|_| CliError::usage("--port 需要 0-65535 的整数"))
            })
            .transpose()?
            .unwrap_or(0);
        let session = serve::ServeSession::bind_loopback(port)
            .map_err(|e| CliError::usage(format!("serve: bind failed: {e}")))?;
        return serve::run(&session, &db, offline, &store);
    }
    // catalog 与 index 是同一个 SqliteStore；App 泛型接受同一实例的两次移动，
    // 故这里克隆一个连接语义上的第二把手不可行——改为让 App 持有单一 store。
    let (command, outcome, mut data, page, warnings) =
        dispatch(&store, &db, db_origin, &rest, mode, request_id, offline)?;
    if mode == protocol::OutputMode::Human {
        attach_empty_catalog_hint(&store, command, &mut data, &db, db_origin)?;
    }
    let duration_ms = started.elapsed().as_millis() as u64;
    // 生效检索模式：search 的 data 已含 `retrieval_mode` 字段（render 投影）；
    // 其他命令恒为 lexical。
    let retrieval_mode = data
        .get("retrieval_mode")
        .and_then(serde_json::Value::as_str)
        .map(|s| match s {
            "semantic" => RetrievalMode::Semantic,
            "hybrid" => RetrievalMode::Hybrid,
            "lexical_fallback" => RetrievalMode::LexicalFallback,
            _ => RetrievalMode::Lexical,
        })
        .unwrap_or(RetrievalMode::Lexical);
    emit_result(
        command,
        mode,
        outcome,
        data,
        duration_ms,
        &page,
        &warnings,
        request_id,
        retrieval_mode,
    );
    Ok(outcome)
}

/// 成功结果的统一出口（contract §6 truth table）：
/// Human → 渲染器文本行走 stdout、warnings 走 stderr（无 envelope）；
/// Json/Jsonl → 单个 success envelope。所有 stdout 写入都经 pipe-safe 通道。
#[allow(clippy::too_many_arguments)]
fn emit_result(
    command: &str,
    mode: protocol::OutputMode,
    outcome: protocol::Outcome,
    data: serde_json::Value,
    duration_ms: u64,
    page: &protocol::Page,
    warnings: &[String],
    request_id: Option<&str>,
    retrieval_mode: RetrievalMode,
) {
    match mode {
        protocol::OutputMode::Human => {
            for warning in warnings {
                eprintln!("warning: {warning}");
            }
            for line in human::render_success(command, outcome, &data, page) {
                protocol::write_stdout_line(&line);
            }
        }
        protocol::OutputMode::Json | protocol::OutputMode::Jsonl => {
            // ADR-0009: machine/cross-boundary output is redacted by default.
            // Human CLI output stays unredacted per ADR-0004 (handled above).
            let (redacted_data, mut redaction) = redaction::redact_value(data);
            let redacted_warnings: Vec<String> = warnings
                .iter()
                .map(|w| {
                    let (r, _) = redaction::redact_text(w);
                    r
                })
                .collect();
            // If any warning was redacted, merge into the count.
            let warning_redactions = warnings
                .iter()
                .zip(redacted_warnings.iter())
                .filter(|(a, b)| a != b)
                .count() as u64;
            if warning_redactions > 0 {
                redaction.redacted_count += warning_redactions;
                redaction.status = agent_session_grep_ports::RedactionState::Applied;
            }
            protocol::write_stdout_line(&protocol::success_envelope(
                command,
                outcome,
                redacted_data,
                duration_ms,
                page,
                &redacted_warnings,
                request_id,
                retrieval_mode,
                &redaction,
            ));
        }
    }
}

#[derive(serde::Serialize)]
struct ProviderCapabilityView<'a> {
    #[serde(flatten)]
    capability: &'a ProviderCapability,
    maturity_target: Option<ProviderMaturity>,
}

/// Read-only public projection of the provider capability matrix. The current
/// matrix supplies every fact; only `maturity_target` is computed, through the
/// matrix-owned roadmap function rather than an entry-point-local table.
fn provider_matrix_data() -> serde_json::Value {
    let matrix = ProviderCapabilityMatrix::current();
    let providers = matrix
        .providers
        .iter()
        .map(|capability| ProviderCapabilityView {
            capability,
            maturity_target: ProviderMaturity::target_for(&capability.provider_id),
        })
        .collect::<Vec<_>>();
    // `semantic` 是对 frozen v1.1 envelope 的加法键（Robot 消费方一次命令即可
    // 同时发现能力矩阵与语义检索事实）；`data` 对 providers 无 schema 约束。
    serde_json::json!({
        "providers": providers,
        "semantic": semantic_capability_data(),
    })
}

/// 语义检索构建事实（`providers` 的 `semantic` 键，`model status` 同一诚实模式）。
///
/// - `feature`：编译进 semantic-candle 时为 "semantic-candle"，默认构建为 null。
/// - `default_model`：恒为诚实默认向量化器 id（bigram-hash，非语义模型）。
/// - `runtime`：编译进 semantic-candle 时为本地 Candle E5 运行时的稳定标识
///   "candle-e5-local"，默认构建为 null。这是构建事实，不是安装事实：E5
///   bundle 是否已通过校验缓存在本地，由 `model status` 报告。
fn semantic_capability_data() -> serde_json::Value {
    use agent_session_grep_application::embedding::BIGRAM_HASH_MODEL_ID;
    #[cfg(feature = "semantic-candle")]
    {
        serde_json::json!({
            "feature": "semantic-candle",
            "default_model": BIGRAM_HASH_MODEL_ID,
            "runtime": "candle-e5-local",
        })
    }
    #[cfg(not(feature = "semantic-candle"))]
    {
        serde_json::json!({
            "feature": null,
            "default_model": BIGRAM_HASH_MODEL_ID,
            "runtime": null,
        })
    }
}

/// `doctor` 的构建事实：semantic-candle 编译进二进制时报告 true，默认构建
/// 报告 null（与 `model status` 的 `feature: null` 诚实模式一致）。
fn semantic_feature_flag() -> serde_json::Value {
    #[cfg(feature = "semantic-candle")]
    {
        serde_json::json!(true)
    }
    #[cfg(not(feature = "semantic-candle"))]
    {
        serde_json::json!(null)
    }
}

fn providers(
    mode: protocol::OutputMode,
    request_id: Option<&str>,
) -> Result<protocol::Outcome, CliError> {
    emit_result(
        "providers",
        mode,
        protocol::Outcome::Success,
        provider_matrix_data(),
        0,
        &protocol::Page::default(),
        &[],
        request_id,
        RetrievalMode::Lexical,
    );
    Ok(protocol::Outcome::Success)
}

fn config_paths(
    mode: protocol::OutputMode,
    request_id: Option<&str>,
) -> Result<protocol::Outcome, CliError> {
    let paths = platform_paths()?;
    emit_result(
        "config.paths",
        mode,
        protocol::Outcome::Success,
        paths,
        0,
        &protocol::Page::default(),
        &[],
        request_id,
        RetrievalMode::Lexical,
    );
    Ok(protocol::Outcome::Success)
}

/// `model import --dir <bundle>` / `model status`: offline model-cache management.
///
/// Never opens a network connection. Import verifies SHA-256 of every declared
/// file, then atomically publishes under `{cache}/models/{model_id}/`. Status
/// reports whether the default E5 bundle is present and verified.
fn model_command(
    args: &[String],
    mode: protocol::OutputMode,
    request_id: Option<&str>,
    offline: bool,
) -> Result<protocol::Outcome, CliError> {
    // model never needs the network; offline is reported honestly but never rejects.
    let _ = offline;
    let positionals = bare_positionals(args);
    let sub = positionals.get(1).map(String::as_str).unwrap_or("");
    match sub {
        "import" => {
            let mut rest = args.to_vec();
            // Strip the command name tokens so extract_flag sees only flags.
            // Flags may appear before or after `model import`.
            let dir = extract_flag(&mut rest, "--dir")?
                .ok_or_else(|| CliError::usage("model import requires --dir <bundle-directory>"))?;
            let paths = platform_paths()?;
            let cache = paths
                .get("cache")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CliError::usage("cannot resolve platform cache path"))?;
            #[cfg(feature = "semantic-candle")]
            {
                let published = agent_session_grep_application::candle_embedding::import_bundle(
                    std::path::Path::new(&dir),
                    std::path::Path::new(cache),
                )
                .map_err(|e| CliError(ProtocolError::from(e)))?;
                let manifest =
                    agent_session_grep_application::candle_embedding::read_and_verify_bundle(
                        &published,
                    )
                    .map_err(|e| CliError(ProtocolError::from(e)))?;
                emit_result(
                    "model.import",
                    mode,
                    protocol::Outcome::Success,
                    serde_json::json!({
                        "imported": true,
                        "path": published.to_string_lossy(),
                        "model_id": manifest.model_id,
                        "dimension": manifest.dimension,
                        "license": manifest.license,
                        "files": manifest.files.len(),
                    }),
                    0,
                    &protocol::Page::default(),
                    &[],
                    request_id,
                    RetrievalMode::Lexical,
                );
                Ok(protocol::Outcome::Success)
            }
            #[cfg(not(feature = "semantic-candle"))]
            {
                // Default build: import is refused honestly (capability_not_supported)
                // rather than staged silently — operators rebuild with the feature.
                let _ = dir;
                let _ = cache;
                Err(CliError(ProtocolError::new(
                    CanonicalCode::CapabilityNotSupported,
                    "model import requires a binary built with --features semantic-candle \
                     (default build stays lexical-only; rebuild with the feature to import)",
                )))
            }
        }
        "status" => {
            let paths = platform_paths()?;
            let cache = paths.get("cache").and_then(|v| v.as_str()).unwrap_or("");
            #[cfg(feature = "semantic-candle")]
            {
                let dir = agent_session_grep_application::candle_embedding::default_model_dir(
                    std::path::Path::new(cache),
                );
                let (present, verified, detail) =
                    match agent_session_grep_application::candle_embedding::read_and_verify_bundle(
                        &dir,
                    ) {
                        Ok(m) => (
                            true,
                            true,
                            serde_json::json!({
                                "model_id": m.model_id,
                                "dimension": m.dimension,
                                "license": m.license,
                                "files": m.files.len(),
                                "path": dir.to_string_lossy(),
                            }),
                        ),
                        Err(e) => (
                            dir.exists(),
                            false,
                            serde_json::json!({
                                "path": dir.to_string_lossy(),
                                "error": e.to_string(),
                            }),
                        ),
                    };
                emit_result(
                    "model.status",
                    mode,
                    protocol::Outcome::Success,
                    serde_json::json!({
                        "feature": "semantic-candle",
                        "present": present,
                        "verified": verified,
                        "detail": detail,
                    }),
                    0,
                    &protocol::Page::default(),
                    &[],
                    request_id,
                    RetrievalMode::Lexical,
                );
                Ok(protocol::Outcome::Success)
            }
            #[cfg(not(feature = "semantic-candle"))]
            {
                let _ = cache;
                emit_result(
                    "model.status",
                    mode,
                    protocol::Outcome::Success,
                    serde_json::json!({
                        "feature": null,
                        "present": false,
                        "verified": false,
                        "detail": {
                            "note": "default build has no semantic-candle feature; \
                                     lexical/bigram-hash remains the only vector backend"
                        },
                    }),
                    0,
                    &protocol::Page::default(),
                    &[],
                    request_id,
                    RetrievalMode::Lexical,
                );
                Ok(protocol::Outcome::Success)
            }
        }
        "" => Err(CliError::usage(
            "model requires a subcommand: import | status",
        )),
        other => Err(CliError::usage(format!(
            "unknown model subcommand `{other}` (expected import|status)"
        ))),
    }
}

fn platform_paths() -> Result<serde_json::Value, CliError> {
    platform_paths_impl()
}

/// MCP 侧可直接调用的平台路径解析（无需 CliError 转协议）。
/// 仅在 semantic-candle 构建下被 mcp.rs 使用。
#[cfg(feature = "semantic-candle")]
pub(crate) fn platform_paths_for_mcp() -> Result<serde_json::Value, CliError> {
    platform_paths_impl()
}

#[cfg(windows)]
// 数据兼容性：目录名 `AgentSessions` 刻意保留旧名——data-root 布局（config/data/
// cache/logs）与既有安装共享，改名会破坏已存在 data root 的路径查找。
fn platform_paths_impl() -> Result<serde_json::Value, CliError> {
    let roaming = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::usage("APPDATA environment variable is not set"))?;
    let local = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::usage("LOCALAPPDATA environment variable is not set"))?;
    Ok(serde_json::json!({
        "config": roaming.join("AgentSessions").join("config.toml").to_string_lossy(),
        "data":   local.join("AgentSessions").join("data").to_string_lossy(),
        "cache":  local.join("AgentSessions").join("cache").to_string_lossy(),
        "logs":   local.join("AgentSessions").join("logs").to_string_lossy(),
    }))
}

#[cfg(target_os = "macos")]
// 数据兼容性：目录名 `AgentSessions` 刻意保留旧名（见 windows 分支注释）。
fn platform_paths_impl() -> Result<serde_json::Value, CliError> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::usage("HOME environment variable is not set"))?;
    let lib = home.join("Library");
    let support = lib.join("Application Support").join("AgentSessions");
    Ok(serde_json::json!({
        "config": support.join("config.toml").to_string_lossy(),
        "data":   support.join("data").to_string_lossy(),
        "cache":  lib.join("Caches").join("AgentSessions").to_string_lossy(),
        "logs":   lib.join("Logs").join("AgentSessions").to_string_lossy(),
    }))
}

#[cfg(all(unix, not(target_os = "macos")))]
// 数据兼容性：目录名 `agentsessions`（`.config/agentsessions`、
// `.local/share/agentsessions` 等）刻意保留旧名——data-root 布局与既有安装共享。
fn platform_paths_impl() -> Result<serde_json::Value, CliError> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::usage("HOME environment variable is not set"))?;
    let config_base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let data_base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));
    let cache_base = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"));
    Ok(serde_json::json!({
        "config": config_base.join("agentsessions").join("config.toml").to_string_lossy(),
        "data":   data_base.join("agentsessions").to_string_lossy(),
        "cache":  cache_base.join("agentsessions").to_string_lossy(),
        "logs":   data_base.join("agentsessions").join("logs").to_string_lossy(),
    }))
}

#[cfg(not(any(windows, unix)))]
// 数据兼容性：目录名 `.agentsessions` 刻意保留旧名（见 windows 分支注释）。
fn platform_paths_impl() -> Result<serde_json::Value, CliError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::usage("cannot resolve home directory"))?;
    let base = home.join(".agentsessions");
    Ok(serde_json::json!({
        "config": base.join("config.toml").to_string_lossy(),
        "data":   base.join("data").to_string_lossy(),
        "cache":  base.join("cache").to_string_lossy(),
        "logs":   base.join("logs").to_string_lossy(),
    }))
}

/// 顶层帮助文本。与 --version 一样走协议出口：裸 println! 会在下游提前关管道时
/// panic（exit 101 + stderr 污染），违反 CONTRACT §6 的 EPIPE 静默 exit 0。
fn help_text() -> String {
    format!(
        "{name} {version}
AI coding-agent history search engine（本地 AI 编程会话历史搜索）。

快速上手（新手从这里开始）:
    agent-session-grep sync --discover              扫描各 provider 数据根建立索引
    agent-session-grep search 关键词                 搜索历史会话
    agent-session-grep show <命中ID>                 看一条命中的正文
    agent-session-grep context <会话ID>              展开一个会话的上下文
    agent-session-grep config paths                 查看数据默认放哪里
    agent-session-grep providers                    查看 Provider 成熟度与能力
数据流：search 返回命中消息 → show <msg_id> 看正文 → context <ses_id> 看整个会话。

USAGE:
    agent-session-grep [--db <path>] <COMMAND> [ARGS]
    agent-session-grep doctor [--db <path>]
    agent-session-grep --help | --version

COMMANDS:
    ingest <file>          解析原始 .jsonl 文件并入库（只读源）
    sync <file>...          原子扫描多个 .jsonl 文件；无变化时不生成新 generation
    sync --discover          自动发现各 provider 数据根下的源并同步（只读源）
    index rebuild          从权威 catalog 全量重投影 FTS 索引（维护命令）
    index compact          回收 freelist（VACUUM；显式维护命令，持排他锁）
    index embeddings       从权威 catalog 构建语义向量索引（semantic/hybrid 检索前置）
    index purge-activities 修剪孤儿工具活动行（无 catalog 消息的活动/悬空 claim；维护命令）
    search <query>         全文检索，按相关性降序返回命中（支持分页/预算/过滤 flag）
    handoff <query>        检索并为查询生成 handoff pack（原文证据 + 建议命令；dry-run）
    get-message <msg-id>   返回命中消息及其同会话主线邻居（--session/--around）
    get-session-resume <ses-id> 返回只读 Resume Metadata（Provider Session ID / Original Working Directory）
    resume <ses-id>        预览恢复命令（默认 dry-run；--yes 才实际执行）
    hook <event>           Claude Code Hook 集成（默认关闭；--enable 才注入历史）
    get <wire-id>          按实体 id 取回原始 payload
    show <wire-id>         按实体 id 取回并归一化展示（role/text 结构）
    list [limit]           稳定排序列出 catalog 实体（默认 20；支持分页/预算 flag）
    context <ses-id>       装配会话上下文：分支消息链 + 证据区间
    status                 报告 catalog 实体总数
    mcp                    启动 stdio MCP 服务（JSON-RPC 2.0；stdout 只输出 MCP frame）
    tui                    交互式只读浏览（Preview；需要交互式终端）
    serve                  启动 loopback HTTP 服务 + 嵌入式 Web UI（--port <n>；仅 loopback）
    doctor                 环境自检（可选 --db 校验存储可打开）
    providers              报告 Provider 成熟度、路线目标与逐字段能力
    config paths           报告当前平台的 config/data/cache/logs 路径
    model import|status    本地 embedding 模型缓存（永不联网；import 需 semantic-candle 构建）

PAGINATION / BUDGET (search, list):
    --cursor <token>       上一页 envelope `page.next_cursor` 的续读令牌
    --max-items <n>        页大小上限（同时作为响应条目预算）
    --max-bytes <n>        响应字节预算（最低 4096）

FILTER (search):
    --provider claude|claude-code|codex  限定 provider（可重复，多个取值按 OR 合并）
    --since <time>         起始时间（含）；RFC3339/ISO-8601 绝对值或 1h/1d/1w 相对量
    --until <time>         结束时间（不含）；语法同 --since
    --include-system       默认排除 system/developer 角色消息；加此旗标恢复
    --group-by-session     按会话归并：每会话保留最高分命中并附 occurrences 计数

FACETS (search，结构化过滤；默认不过滤，输出与旧版一致):
    --main-only            只看主线消息（排除 sidechain）
    --subagent-only        只看 subagent（sidechain）消息；与 --main-only 互斥
    --include-sidechain    显式包含 sidechain（默认值；不与上述两者并用）
    --tool-kind <kind>     只保留做过 file|command|web|query|unknown 工具调用的消息
    --tool-name <name>     只保留用过该工具（逐字相等）的消息

GET MESSAGE:
    --session <ses-id>     共享消息的所属会话；有歧义时必须指定
    --around <n>           主线两侧各返回 n 条邻居（默认 0，仅锚点）
    --max-items <n>        返回消息条数预算
    --max-bytes <n>        响应字节预算（最低 4096）

CONTEXT:
    --policy mainline|full 分支策略（默认 mainline：排除 sidechain 沿 parent 链）
    --level raw|talks|sessions 结构层级（默认 raw：纯消息链；talks 按用户消息分组；sessions 结构概览）
    --max-messages <n>     消息条数预算
    --max-bytes <n>        响应字节预算

GLOBAL（全局 flag 放在命令名之前；子命令 flag 如 --max-items 放在命令名之后）:
    --db <path>            SQLite store path. Optional: defaults to <data dir>/asg.db
                           (override with $ASG_DB; `config paths` prints the data dir).
                           Read commands never create a store; write commands do.
    --output human|json|jsonl  输出模式（默认 human：人类可读文本；json/jsonl 为协议 envelope）
    --robot                等价 --output json，无颜色/进度（stdout 只输出协议）
    --request-id <id>      robot 调用方关联 id，原样回显于每个 frame（A-Za-z0-9._:- 计 1-128 字符）
    --offline              拒绝任何需要联网的显式操作（fail-closed；当前所有命令本地执行，本 flag 是稳定显式模式，doctor/hook 会如实上报）
    -h, --help             打印本帮助
    -V, --version          打印版本

术语速记:
    generation       第 N 次入库（数据每更新一次 +1）
    cursor           翻页令牌（结果多于一页时用来取下一页）
    wire-id          实体 ID（msg_v1_ 消息 / ses_v1_ 会话 / doc_v1_ 文档）
    score            相关度分数（越高越相关，按分数降序排列）

EXIT CODES:
    0 成功；10 部分成功（预算截断，结果可用但不完整）；其余见 error catalog",
        name = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// 帮助/版本统一出口（ADR-0006）：human 逐行打印文本到 stdout；json/jsonl/robot
/// 输出单个 success envelope（jsonl 即单帧），`--request-id` 原样回显。所有
/// help 路径都发生在 --db 解析与存储打开之前，无任何文件副作用。
fn emit_help(command: &str, text: &str, mode: protocol::OutputMode, request_id: Option<&str>) {
    emit_help_payload(
        command,
        serde_json::json!({ "help_text": text }),
        text,
        mode,
        request_id,
    );
}

/// 版本统一出口：与 [`emit_help`] 同构，版本串放进 `data.version`。
fn emit_version(command: &str, text: &str, mode: protocol::OutputMode, request_id: Option<&str>) {
    emit_help_payload(
        command,
        serde_json::json!({ "version": text }),
        text,
        mode,
        request_id,
    );
}

/// [`emit_help`] / [`emit_version`] 的公共实现。stdout 写入统一走
/// [`protocol::write_stdout_line`]（EPIPE 静默 exit 0，CONTRACT §6）。
fn emit_help_payload(
    command: &str,
    data: serde_json::Value,
    text: &str,
    mode: protocol::OutputMode,
    request_id: Option<&str>,
) {
    match mode {
        protocol::OutputMode::Human => {
            for line in text.lines() {
                protocol::write_stdout_line(line);
            }
        }
        protocol::OutputMode::Json | protocol::OutputMode::Jsonl => {
            protocol::write_stdout_line(&help_envelope(command, data, request_id));
        }
    }
}

/// 机器模式下 help/version 的 envelope（json/jsonl 同形：单个 success envelope，
/// jsonl 即单帧；`data` 携带帮助文本/版本串；`--request-id` 原样回显）。
fn help_envelope(command: &str, data: serde_json::Value, request_id: Option<&str>) -> String {
    protocol::success_envelope(
        command,
        protocol::Outcome::Success,
        data,
        0,
        &protocol::Page::default(),
        &[],
        request_id,
        RetrievalMode::default(),
        &RedactionStatus::default(),
    )
}

/// 全部子命令名——`known_subcommand` 与 unknown-subcommand 报错共用的唯一真源。
///
/// 过去这两处是两份手写列表，报错那份漏了 7 个命令（`asg handof q` 会声称
/// `handoff` 不是可用命令）。任何新子命令只加这一行即可，二者不可能再漂移。
const KNOWN_SUBCOMMANDS: &[&str] = &[
    "ingest",
    "sync",
    "index",
    "search",
    "handoff",
    "get-message",
    "get-session-resume",
    "resume",
    "hook",
    "get",
    "show",
    "list",
    "context",
    "status",
    "mcp",
    "tui",
    "serve",
    "doctor",
    "providers",
    "config",
    "model",
];

/// 是否为已知子命令（用于子命令 `--help` 拦截与 unknown-subcommand 报错提示）。
fn known_subcommand(cmd: &str) -> bool {
    KNOWN_SUBCOMMANDS.contains(&cmd)
}

/// 与输入最接近的子命令名（did-you-mean）。
///
/// 用 Levenshtein 距离，阈值随输入长度放宽（短名字的一次编辑就可能是另一个真命令，
/// 所以 ≤4 字符只容 1 次编辑）。无足够接近的候选返回 `None`——绝不瞎猜。
fn closest_subcommand(input: &str) -> Option<&'static str> {
    let lowered = input.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return None;
    }
    let budget = if lowered.chars().count() <= 4 { 1 } else { 2 };
    KNOWN_SUBCOMMANDS
        .iter()
        .map(|candidate| (*candidate, edit_distance(&lowered, candidate)))
        .filter(|(_, distance)| *distance <= budget)
        .min_by_key(|(candidate, distance)| (*distance, candidate.len()))
        .map(|(candidate, _)| candidate)
}

/// Levenshtein 距离（滚动一行 DP，无新依赖）。
fn edit_distance(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0usize; right_chars.len() + 1];
    for (i, left_char) in left.chars().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right_chars.iter().enumerate() {
            let substitution = previous[j] + usize::from(left_char != *right_char);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right_chars.len()]
}

/// unknown-subcommand 报错：列表由 [`KNOWN_SUBCOMMANDS`] 派生，并在有足够接近的
/// 候选时给 did-you-mean。
fn unknown_subcommand_error(cmd: &str) -> CliError {
    let mut message = format!("unknown subcommand: {cmd}\n");
    if let Some(suggestion) = closest_subcommand(cmd) {
        message.push_str(&format!("Did you mean `{suggestion}`?\n"));
    }
    message.push_str(&format!(
        "Available commands: {}\nRun `--help` for full usage.",
        KNOWN_SUBCOMMANDS.join(", ")
    ));
    CliError::usage(message)
}

/// 无子命令时的报错：同样把完整命令表说出来。
fn missing_subcommand_error() -> CliError {
    CliError::usage(format!(
        "missing subcommand.\nAvailable commands: {}\nRun `--help` for full usage.",
        KNOWN_SUBCOMMANDS.join(", ")
    ))
}

/// 子命令级帮助文本：渲染该命令的签名、flag 与一个真实示例。由 help/version
/// 提前拦截阶段（ADR-0006）在 `<cmd> --help|-h`（含 `index rebuild --help`）时
/// 触发——顶层 --help 只给全局概览，子命令帮助给单命令的用法。
fn subcommand_help_text(cmd: &str) -> &'static str {
    match cmd {
        "search" => {
            "search <query>：全文检索历史会话，按相关性降序返回命中。\n\
                     示例：agent-session-grep --db <path> search 配置备份\n\
                     flag（放子命令后）：--max-items <n> 页大小、--cursor <token> 翻页、--max-bytes <n> 预算；\n\
                     过滤：--provider claude|claude-code|codex（可重复，OR）、--since/--until <RFC3339 或 1h|1d|1w>（半开区间 [since, until)）；\n\
                     检索模式：--mode lexical|semantic|hybrid（默认 lexical）。semantic/hybrid 需先跑 `index embeddings`；\n\
                     向量索引未就绪时结果标注 retrieval_mode=lexical_fallback 并给出 warning，绝不静默降级；\n\
                     --include-system（默认排除 system/developer 角色消息）、--group-by-session（按会话归并并附 occurrences）；\n\
                     结构化过滤：--main-only 只看主线（排除 sidechain）、--subagent-only 只看 subagent 消息、\n\
                     --tool-kind file|command|web|query|unknown 只保留做过该种工具调用的消息、\n\
                     --tool-name <名字> 只保留用过该工具（逐字相等）的消息（--main-only 与 --subagent-only 互斥）"
        }
        "get-message" => {
            "get-message <msg-id>：返回一个消息及其同会话主线邻居。\n\
                          示例：agent-session-grep --db <path> get-message msg_v1_... --session ses_v1_... --around 2\n\
                          flag：--session <ses-id>、--around <n>、--max-items <n>、--max-bytes <n>"
        }
        "handoff" => {
            "handoff <query>：为查询生成 handoff pack（handoff-pack/v1）。\n\
                     示例：agent-session-grep --db <path> handoff 配置备份\n\
                     检索命中后组装：原文证据（evidence）与推断（inference）严格分栏；\n\
                     deterministic 默认（无 LLM 调用）；dry-run——只输出 pack 与建议命令，不注入任何 agent。\n\
                     flag：--max-evidence <n> 证据条数上限（默认 20）、--max-tokens <n> token 预算（默认 8000）、\n\
                     --max-bytes <n> 序列化字节预算（默认 2000000）；截断时 exit 10"
        }
        "get" => {
            "get <wire-id>：按实体 ID 取回原始 payload。\n\
                  示例：agent-session-grep --db <path> get msg_v1_..."
        }
        "get-session-resume" => {
            "get-session-resume <ses-id>：返回一个会话的只读 Resume Metadata。\n\
                  示例：agent-session-grep --db <path> get-session-resume ses_v1_...\n\
                  固定字段（缺失为 null）：provider_session_id、original_working_directory；\n\
                  不会生成/执行任何恢复命令，也不暴露 transcript 路径。"
        }
        "resume" => {
            "resume <ses-id>：预览（默认）或执行会话的原地恢复。\n\
                  示例：agent-session-grep --db <path> resume ses_v1_...\n\
                  默认 dry-run——打印将执行的完整命令（provider/cwd/session id）并退出；\n\
                  确认无误后加 --yes 才在原工作目录实际启动 provider 进程。\n\
                  首次使用 resume 强制只预览一次（持久标记），--yes 从第二次起才生效；\n\
                  未核验恢复命令的 provider 报 available:false，绝不编造命令。"
        }
        "hook" => {
            "hook <session-start|user-prompt-submit>：Claude Code Hook 集成（默认关闭）。\n\
                  示例：echo '{\"prompt\":\"数据库迁移\"}' | agent-session-grep --db <path> hook user-prompt-submit --enable\n\
                  从 stdin 读 hook payload，检索历史并按 hookSpecificOutput 契约输出；\n\
                  不加 --enable 时输出空 context（不注入任何历史）；\n\
                  flag：--enable 启用注入、--max-tokens <n> 预算（默认 2000）；\n\
                  --provider claude|claude-code|codex（可重复，OR 限定 provider）、--decay-days <n>（只注入最近 N 天）；\n\
                  注入文本经跨边界脱敏（ADR-0009）；全局 --offline 时如实上报 offline 字段。"
        }
        "show" => {
            "show <wire-id>：按实体 ID 取回并展示（role/text/时间戳）。\n\
                   示例：agent-session-grep --db <path> show msg_v1_...\n\
                   从 search 命中或 show 输出里的 session 字段，可继续用 context 展开会话。"
        }
        "list" => {
            "list [limit]：按稳定序列出实体（默认 20）。\n\
                   示例：agent-session-grep --db <path> list 50\n\
                   flag（放子命令后）：--cursor <token> 翻页"
        }
        "context" => {
            "context <ses-id>：装配一个会话的完整上下文（消息链 + 证据区间）。\n\
                      示例：agent-session-grep --db <path> context ses_v1_...\n\
                      flag：--policy mainline|full、--level raw|talks|sessions、--max-messages <n>、--max-bytes <n>"
        }
        "status" => {
            "status：报告当前库的实体总数与 generation。\n\
                     示例：agent-session-grep --db <path> status"
        }
        "sync" => {
            "sync <file>...：原子扫描一个或多个 transcript 文件入库；无变化不写库。\n\
                   sync --from-file <清单>：从清单文件读路径（每行一个，空行与 # 注释忽略）。\n\
                   \u{20}  源很多时用它——几千个路径展开后会超出命令行长度上限。\n\
                   sync --discover：自动发现各 provider 数据根（~/.claude/projects、~/.codex/sessions 等）下的源并同步；\n\
                   \u{20}  是不是源由各 provider 自己的 probe 判定，不按扩展名（SQLite/JSON/Markdown transcript 同样能被发现）。\n\
                   sync --provider <id> <file>...：点名 provider 消歧。少数 provider 的磁盘格式彼此同形\n\
                   \u{20}  （如 pi 与 openclaw），probe 只能判定同分而拒绝归属；此时用它指明。\n\
                   示例：agent-session-grep --db <path> --robot sync 会话.jsonl\n\
                   示例：agent-session-grep --db <path> sync --from-file sources.txt\n\
                   示例：agent-session-grep --db <path> sync --discover\n\
                   示例：agent-session-grep --db <path> sync --provider pi 会话.jsonl\n\
                   约束：单个 transcript 文件应只包含一个会话；检测到多个 sessionId 时仍归属首个会话，并在 warnings 报告。\n\
                   提示：不接受目录；--discover 会递归扫描 provider 数据根，无法归属的文件跳过并计数。"
        }
        "ingest" => {
            "ingest <file>：解析单个 .jsonl 文件入库。\n\
                     示例：agent-session-grep --db <path> ingest 会话.jsonl\n\
                     约束：单个 transcript 文件应只包含一个会话；检测到多个 sessionId 时仍归属首个会话，并输出诊断 warning。"
        }
        "index" => {
            "index rebuild：从权威 catalog 重建全文（FTS）索引；\n\
                    index embeddings：从权威 catalog 构建语义向量索引（semantic/hybrid 检索前置，需 semantic-candle 构建的二进制）。\n\
                    index purge-activities：修剪孤儿工具活动行（维护命令）。\n\
                    index compact：回收 freelist（VACUUM 整库重写）。实测 10 万条库回收约 17%。\n\
                   \u{20}  刻意不塞进 sync：它持排他锁、需要约等于库大小的临时空间，何时付这个代价由你决定。\n\
                   建立索引请用 sync --discover，不要用 index 直写。\n\
                   示例：agent-session-grep --db <path> --robot index rebuild"
        }
        "doctor" => {
            "doctor [--db <path>]：环境自检；带 --db 时校验存储可打开、报 schema。\n\
                     示例：agent-session-grep --db <path> doctor"
        }
        "mcp" => {
            "mcp：启动 stdio MCP 服务（供 Claude Code 等 AI 宿主调用）。\n\
                  示例：agent-session-grep --db <path> mcp"
        }
        "tui" => {
            "tui：交互式只读浏览（Preview）。需要交互式终端。\n\
                   tui --snapshot-json <query>：headless 结构投影（供 release 一致性 harness 跨入口比对）。\n\
                   示例：agent-session-grep --db <path> tui"
        }
        "serve" => {
            "serve：启动 loopback HTTP 服务 + 嵌入式 Web UI（仅 127.0.0.1）。\n\
                    每次启动生成随机 bearer token；浏览器打开终端打印的 URL（含 token）即可访问。\n\
                    本 release 仅 loopback：--lan 为 capability_not_supported（绝不暴露局域网）。\n\
                    flag：--port <n>（可选，默认 0 = 随机端口）。\n\
                    示例：agent-session-grep --db <path> serve"
        }
        "providers" => {
            "providers：报告当前 Provider 能力矩阵（成熟度事实、路线目标与逐字段能力）。\n\
                         示例：agent-session-grep --robot providers\n\
                         数据来自唯一能力矩阵；deferred provider 的 maturity_target 为 null。"
        }
        "config" => {
            "config paths：报告当前平台的 config/data/cache/logs 路径。\n\
                     示例：agent-session-grep config paths"
        }
        "model" => {
            "model import|status：本地 embedding 模型缓存管理（永不联网）。\n\
                     model import --dir <bundle>  校验 SHA-256 后原子发布到 cache/models/...\n\
                     model status                 报告默认 E5 bundle 是否已导入且校验通过\n\
                     需要 `--features semantic-candle` 构建的二进制才能 import；默认构建仅 status。\n\
                     示例：agent-session-grep model status"
        }
        _ => "运行 agent-session-grep --help 查看完整命令列表。",
    }
}

/// doctor：最小环境自检。报告版本；若给了 --db，尝试打开存储并报告 schema。
/// `offline` 作为诊断字段原样上报（design D5）：`--offline` 是稳定显式模式，
/// 当前没有任何命令需要联网，doctor 如实反映调用方声明的 offline 意图。
fn doctor(
    args: &[String],
    mode: protocol::OutputMode,
    request_id: Option<&str>,
    offline: bool,
) -> Result<protocol::Outcome, CliError> {
    let started = std::time::Instant::now();
    // 多余位置参数是用法错误，不静默忽略（与其它子命令一致）。
    if bare_positionals(args).len() > 1 {
        return Err(CliError::usage("doctor takes no positional arguments"));
    }
    // 与 parse_db_flag 共用同一 --db 取值守卫：`doctor --db --robot` 不得把
    // --robot 当路径（会造出同名文件），重复 --db 是用法错误（R8.1/R8.2）。
    // doctor 的 --db 允许在命令名之后（`doctor [--db <path>]`），故整串扫描。
    let db_opt = extract_db_flag_anywhere(args)?;
    let data = match db_opt {
        None => serde_json::json!({
            "tool": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "db": "not-checked",
            "schema": null,
            "offline": offline,
            // 构建事实：semantic-candle 是否编译进本二进制（默认构建为 null）。
            "semantic_feature": semantic_feature_flag(),
            // schema v12 事实：tool_activities/tool_activity_membership 表随
            // 本二进制管理的每个 catalog 落库，未指定 --db 也成立。
            "tool_activity_storage": true,
            // 新手会误以为 db: not-checked 是自检失败（10 角色体验测试缺陷）。
            // 加一行白话提示，说明如何真正校验。
            "hint": "未指定数据库：以上仅检查了环境。运行 doctor --db <path> 可校验数据库与 schema。",
        }),
        Some(path) => {
            let store = SqliteStore::open(&path).map_err(ProtocolError::from)?;
            let schema = store.schema_version().map_err(ProtocolError::from)?;
            // generation 与待收敛 intent 数是 durable outbox 中断恢复与一致性的只读证据。
            let generation = store.active_generation().map_err(ProtocolError::from)?;
            let interrupted = store
                .interrupted_batch_count()
                .map_err(ProtocolError::from)?;
            // 工具活动保留策略证据（v12）：孤儿投影行计数。>0 时用
            // `index purge-activities` 确定性修剪（catalog/FTS 不受影响）。
            let (orphaned_tool_activities, orphaned_activity_memberships) = store
                .orphaned_activity_counts()
                .map_err(ProtocolError::from)?;
            serde_json::json!({
                "tool": env!("CARGO_PKG_NAME"),
                "version": env!("CARGO_PKG_VERSION"),
                "db": "ok",
                "schema": schema,
                "offline": offline,
                "semantic_feature": semantic_feature_flag(),
                // 打开的库已被迁移到本二进制的 schema v12，工具活动存储存在。
                "tool_activity_storage": true,
                "generation": generation,
                "interrupted_batches": interrupted,
                "orphaned_tool_activities": orphaned_tool_activities,
                "orphaned_activity_memberships": orphaned_activity_memberships,
            })
        }
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    emit_result(
        "doctor",
        mode,
        protocol::Outcome::Success,
        data,
        duration_ms,
        &protocol::Page::default(),
        &[],
        request_id,
        RetrievalMode::Lexical,
    );
    Ok(protocol::Outcome::Success)
}

/// 已知 flag 名全集（前缀位置可出现的旗标）。取值守卫用它拒绝 `--db --robot`
/// 这类把 flag 当取值的写法——否则 `parse_db_flag` 会造出名为 `--robot` 的文件。
fn is_known_flag_name(token: &str) -> bool {
    matches!(
        token,
        "--db"
            | "--output"
            | "--request-id"
            | "--robot"
            | "--no-color"
            | "--help"
            | "-h"
            | "--version"
            | "-V"
            | "--cursor"
            | "--max-items"
            | "--max-bytes"
            | "--max-messages"
            | "--max-evidence"
            | "--max-tokens"
            | "--policy"
            | "--level"
            | "--provider"
            | "--since"
            | "--until"
            | "--session"
            | "--around"
            | "--snapshot-json"
            | "--discover"
            | "--offline"
            | "--main-only"
            | "--subagent-only"
            | "--include-sidechain"
            | "--tool-kind"
            | "--tool-name"
    )
}

/// 从前缀位置抽出 `--db <path>`；缺省返回 None。带值 flag 的取值跳过。
///
/// 取值缺失、取值是已知 flag 名（`--db --robot` 会造出名为 `--robot` 的文件）、
/// 重复出现（`--db a --db b` 不再静默 last-wins）都是用法错误（R8.1/R8.2）。
/// doctor 与 [`parse_db_flag`] 共用同一守卫，避免 doctor 的 --db 绕过校验。
fn extract_db_flag(args: &[String]) -> Result<Option<String>, CliError> {
    extract_db_flag_impl(args, true)
}

/// doctor 专用变体：`--db` 允许跟在命令名之后（`doctor [--db <path>]`），
/// 扫描全部 token 而非只扫前缀；取值守卫与 [`extract_db_flag`] 一致。
fn extract_db_flag_anywhere(args: &[String]) -> Result<Option<String>, CliError> {
    extract_db_flag_impl(args, false)
}

fn extract_db_flag_impl(args: &[String], prefix_only: bool) -> Result<Option<String>, CliError> {
    let mut db = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !a.starts_with('-') {
            if prefix_only {
                break; // 已到命令名：之后的 token 不当全局 flag 解析
            }
            continue; // doctor 的扫描：非 flag token 直接跳过
        }
        if a == "--db" {
            let value = it
                .next()
                .ok_or_else(|| CliError::usage("--db requires a path"))?;
            if is_known_flag_name(value) {
                return Err(CliError::usage(format!(
                    "--db requires a path (got flag {value}); --db <path> must precede other flags"
                )));
            }
            if value.is_empty() {
                return Err(CliError::usage(
                    "--db requires a non-empty path (an empty path silently uses SQLite's private temporary database)",
                ));
            }
            if db.is_some() {
                return Err(CliError::usage("duplicate --db flag"));
            }
            db = Some(value.clone());
        }
        match a.as_str() {
            "--output" | "--request-id" | "--cursor" | "--max-items" | "--max-bytes"
            | "--max-messages" | "--max-evidence" | "--max-tokens" | "--policy" | "--level"
            | "--provider" | "--since" | "--until" | "--session" | "--around" | "--tool-kind"
            | "--tool-name" => {
                it.next();
            }
            _ => {}
        }
    }
    Ok(db)
}

/// 从 `--db <path>` 抽出数据库路径，返回其余参数。
///
/// flag 只在前缀位置（第一个裸参数即命令名之前）识别；命令名之后的 token
/// 原样进 `rest`，由 dispatch 的 `extract_flag` 挑出命令级 flag——这样查询文本
/// 恰等于 `--help`/`--robot`/`--output` 等 flag 名时不会被吞掉。
///
/// `--db` 的取值守卫（缺值、取值为已知 flag、重复）统一在 [`extract_db_flag`]
/// 完成（R8.1/R8.2）。
///
/// `--db` 缺席不是错误：store path 由 [`resolve_store_path`] 按
/// `--db` > `$ASG_DB` > 平台数据目录默认值解析（D8 零配置）。
fn parse_db_flag(args: &[String]) -> Result<(Option<String>, Vec<String>), CliError> {
    let db = extract_db_flag(args)?;
    let mut rest = Vec::new();
    let mut seen_command = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if seen_command {
            rest.push(a.to_string());
            continue;
        }
        match a.as_str() {
            "--db" | "--output" | "--request-id" => {
                it.next(); // 消费其取值（--db 取值已由 extract_db_flag 校验）
            }
            "--robot" | "--no-color" | "--help" | "-h" | "--version" | "-V" | "--discover"
            | "--offline" => {}
            other => {
                seen_command = true;
                rest.push(other.to_string());
            }
        }
    }
    Ok((db, rest))
}

/// store path 的来源，决定给用户的提示里是否需要带 `--db`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DbOrigin {
    /// 显式 `--db <path>`。
    Flag,
    /// `$ASG_DB` 环境变量。
    Env,
    /// 平台数据目录下的默认库（零配置路径）。
    Default,
}

/// 默认库文件名（落在 `config paths` 报告的 data 目录下）。
const DEFAULT_DB_FILE_NAME: &str = "asg.db";

/// 覆盖默认 store path 的环境变量名。
const DB_ENV_VAR: &str = "ASG_DB";

/// 解析本次调用要用的 store path（D8：零配置优先，显式覆盖优先级最高）。
///
/// 优先级 `--db <path>` > `$ASG_DB` > `<平台 data 目录>/asg.db`——与
/// ripgrep/gh/atuin 的习惯一致：装完即可用，路径只在需要时才显式给。
/// 默认位置恰是 `config paths` 报告的 data 目录，因此那条命令不再"撒谎"。
fn resolve_store_path(flag: Option<String>) -> Result<(String, DbOrigin), CliError> {
    if let Some(path) = flag {
        return Ok((path, DbOrigin::Flag));
    }
    // 环境变量为空串时视为未设置：空路径会让 SQLite 静默启用私有临时库。
    if let Some(value) = std::env::var_os(DB_ENV_VAR) {
        let path = value.to_string_lossy().into_owned();
        if !path.trim().is_empty() {
            return Ok((path, DbOrigin::Env));
        }
    }
    let paths = platform_paths()?;
    let data = paths
        .get("data")
        .and_then(|value| value.as_str())
        .ok_or_else(|| CliError::usage("cannot resolve the platform data directory"))?;
    let path = std::path::Path::new(data).join(DEFAULT_DB_FILE_NAME);
    Ok((path.to_string_lossy().into_owned(), DbOrigin::Default))
}

/// 命令行前缀：默认/环境变量解析出的库无需 `--db`，显式 `--db` 的提示必须带上
/// 同一路径，否则复制粘贴出来的命令会指向另一个库。
fn db_flag_prefix(db: &str, origin: DbOrigin) -> String {
    match origin {
        DbOrigin::Flag => format!("--db {db} "),
        DbOrigin::Env | DbOrigin::Default => String::new(),
    }
}

/// 空库/缺库时该跑的那条命令（提示里逐字给出，用户可直接复制）。
fn sync_discover_command(db: &str, origin: DbOrigin) -> String {
    format!("asg {}sync --discover", db_flag_prefix(db, origin))
}

/// 写路径的 data root 兜底创建。默认库位于平台数据目录，首次写入前该目录不存在。
fn create_store_parent_dir(db: &str) -> Result<(), CliError> {
    let Some(parent) = std::path::Path::new(db).parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() || parent.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|error| {
        CliError(ProtocolError::new(
            CanonicalCode::CatalogError,
            format!(
                "cannot create the store directory {}: {error}",
                parent.display()
            ),
        ))
    })
}

/// 读路径的存在性前置检查（M2P-2）：库不存在时报 `not_found` 并给出路径本身
/// 与该跑的完整命令，而不是静默新建一个空库然后报"零命中"。
fn require_existing_store(db: &str, origin: DbOrigin) -> Result<(), CliError> {
    if std::path::Path::new(db).exists() {
        return Ok(());
    }
    let command = sync_discover_command(db, origin);
    let where_from = match origin {
        DbOrigin::Flag => " (from --db)",
        DbOrigin::Env => " (from $ASG_DB)",
        DbOrigin::Default => " (default store path)",
    };
    Err(CliError(ProtocolError::new(
        CanonicalCode::NotFound,
        format!(
            "no store at {db}{where_from}.\n\
             Build one with: {command}\n\
             Reads never create a store, so a mistyped --db fails here instead of \
             returning an empty result."
        ),
    )))
}

/// 提取纯位置参数（跳过已知带值 flag 及其取值、已知裸 flag）——供 doctor/config
/// 这类绕过 parse_db_flag 的命令做多余参数校验。
///
/// 未知的 `-` 开头 token 不是已知 flag，按多余位置参数计入（R8.3）：`doctor
/// --bogus` 不能静默丢弃 `--bogus` 后假装成功（exit 0 + db:not-checked）。
fn bare_positionals(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" | "--output" | "--request-id" | "--cursor" | "--max-items" | "--max-bytes"
            | "--max-messages" | "--max-evidence" | "--max-tokens" | "--policy" | "--level"
            | "--provider" | "--since" | "--until" | "--session" | "--around" | "--tool-kind"
            | "--tool-name" => {
                it.next(); // 消费其取值
            }
            "--robot" | "--no-color" | "--help" | "-h" | "--version" | "-V" | "--discover"
            | "--offline" => {}
            s => out.push(s.to_string()),
        }
    }
    out
}

/// `--offline` fail-closed 网关（design D5）：未来需要联网的能力（模型下载、
/// 外部 Embedding API、telemetry）在尝试连接前必须先经过本网关。offline 下
/// 以 `capability_not_supported` 拒绝，绝不静默降级。当前没有任何能力需要
/// 联网，网关由未来命令调用、测试直接覆盖。
fn offline_capability_gate(offline: bool, capability: &str) -> Result<(), CliError> {
    if offline {
        return Err(CliError(ProtocolError::new(
            CanonicalCode::CapabilityNotSupported,
            format!("capability {capability} requires network; refused under --offline"),
        )));
    }
    Ok(())
}

/// 分发子命令。`store` 同时充当 CatalogStore 与 SearchIndex（同一 SqliteStore）。
/// `db` 用于 resume 首次预览标记的 data-root 定位（与 writer lease 同一根）。
/// `mode`/`request_id` 只喂给需要发协议 frame 的子命令（sync 的 jsonl progress）。
/// `offline` 是 `--offline` 全局 flag 的 fail-closed 网关（design D5）：当前没有任何
/// 子命令需要联网，flag 不改动既有行为；未来需要联网的能力（模型下载、外部
/// Embedding API、telemetry）必须先经过 [`offline_capability_gate`]，在 offline
/// 下以 `capability_not_supported` 拒绝，绝不静默降级。
fn dispatch(
    store: &SqliteStore,
    db: &str,
    db_origin: DbOrigin,
    rest: &[String],
    mode: protocol::OutputMode,
    request_id: Option<&str>,
    offline: bool,
) -> Result<
    (
        &'static str,
        protocol::Outcome,
        serde_json::Value,
        protocol::Page,
        Vec<String>,
    ),
    CliError,
> {
    let cmd = rest.first().ok_or_else(missing_subcommand_error)?.as_str();
    // Design D5 fail-closed gate: 未来需要联网的子命令必须在此登记，并在连接前
    // 经过 [`offline_capability_gate`]。当前没有任何子命令需要联网（模型下载、
    // 外部 Embedding API、telemetry 均未实现），列表为空——offline 是稳定显式
    // 模式，不改动既有命令行为；测试直接覆盖网关语义。
    const NETWORK_REQUIRING_SUBCOMMANDS: &[&str] = &[];
    for network_cmd in NETWORK_REQUIRING_SUBCOMMANDS {
        if cmd == *network_cmd {
            offline_capability_gate(offline, cmd)?;
        }
    }
    match cmd {
        // index 是切片期的写入入口：派生一个 Reconstructed 消息 id，写 catalog + 索引。
        // 它不经 Application（Application 首片只暴露读用例），直接用端口写入。
        // `index rebuild` 是维护子命令：从权威 catalog 全量重投影 FTS 索引。
        "index" => {
            if rest.get(1).map(String::as_str) == Some("rebuild") {
                no_extra_args(rest, 1, "index rebuild")?;
                let reindexed = store.rebuild_index().map_err(ProtocolError::from)?;
                let generation = store.active_generation().map_err(ProtocolError::from)?;
                Ok((
                    "index.rebuild",
                    protocol::Outcome::Success,
                    serde_json::json!({
                        "reindexed": reindexed,
                        "generation": generation,
                    }),
                    protocol::Page::default(),
                    Vec::new(),
                ))
            } else if rest.get(1).map(String::as_str) == Some("embeddings") {
                // 语义向量索引构建（#3）：从权威 catalog 重投影 message_vec。
                // 与 `index rebuild` 同级——向量表是 catalog 的投影，不是权威数据，
                // 可随时整表重建。构建后 semantic/hybrid 检索才会离开
                // lexical_fallback。
                no_extra_args(rest, 1, "index embeddings")?;
                let (data, warnings) = build_embeddings(store)?;
                Ok((
                    "index.embeddings",
                    protocol::Outcome::Success,
                    data,
                    protocol::Page::default(),
                    warnings,
                ))
            } else if rest.get(1).map(String::as_str) == Some("compact") {
                // freelist 回收（M2-9）：`VACUUM` 整库重写，把已分配未使用的页
                // 交还文件系统。实测 10 万条真库回收 17.3%（537.8 → 444.7 MB）。
                //
                // 刻意是显式命令而非 sync 的隐式尾步：VACUUM 持排他锁并需要约等于
                // 库大小的临时空间，塞进 sync 会把几百毫秒的增量同步变成几十秒的
                // 停顿，而多数 sync 之后并无可回收空间。何时付这个代价由用户决定。
                no_extra_args(rest, 1, "index compact")?;
                let (before, after) = store.compact().map_err(ProtocolError::from)?;
                let generation = store.active_generation().map_err(ProtocolError::from)?;
                Ok((
                    "index.compact",
                    protocol::Outcome::Success,
                    serde_json::json!({
                        // 如实报三个数：回收前后与差额。不报"压缩比"——
                        // 回收量取决于删除历史，一个比例会被读成可预期的保证。
                        "bytes_before": before,
                        "bytes_after": after,
                        "bytes_reclaimed": before.saturating_sub(after),
                        "generation": generation,
                    }),
                    protocol::Page::default(),
                    Vec::new(),
                ))
            } else if rest.get(1).map(String::as_str) == Some("purge-activities") {
                // 工具活动保留策略（v12）的维护命令：确定性修剪孤儿活动行
                // （无 catalog 消息的活动、指向不存在活动的 claim）。与
                // `index rebuild` 同一 writer lease/CAS 纪律；无孤儿时不写库。
                // 活动行是 catalog 的投影，修剪绝不触碰 catalog/FTS。
                no_extra_args(rest, 1, "index purge-activities")?;
                let (removed_activities, removed_memberships) = store
                    .purge_orphaned_activities()
                    .map_err(ProtocolError::from)?;
                let generation = store.active_generation().map_err(ProtocolError::from)?;
                Ok((
                    "index.purge-activities",
                    protocol::Outcome::Success,
                    serde_json::json!({
                        "removed_activities": removed_activities,
                        "removed_memberships": removed_memberships,
                        "generation": generation,
                    }),
                    protocol::Page::default(),
                    Vec::new(),
                ))
            } else {
                // 直写入口：绕过 provider 解析,把任意文本塞进权威 catalog。切片
                // 期的开发工具,对真实库是破坏性的——它写进去的东西没有源文件、
                // 没有 provenance,`index rebuild` 会忠实地把它重投影出来。
                //
                // 因此不在用户 help 里公布(见 `help_text`),并要求显式
                // `--force-dev`:能力保留给测试与切片,但不能被顺手用在自己的
                // 真实历史上。
                let mut args = rest.to_vec();
                if !take_bool_flag(&mut args, "--force-dev") {
                    return Err(CliError::usage(
                        "index <id-fact> <text> 是开发直写入口，会绕过 provider 解析\
                         把无来源的内容写进权威 catalog；确实要用请加 --force-dev。\
                         要建立索引请用：sync --discover",
                    ));
                }
                no_extra_args(&args, 2, "index <id-fact> <text> --force-dev")?;
                let fact = arg(&args, 1, "index <id-fact> <text> --force-dev")?;
                let text = arg(&args, 2, "index <id-fact> <text> --force-dev")?;
                Ok((
                    "index",
                    protocol::Outcome::Success,
                    index_one(store, fact, text)?,
                    protocol::Page::default(),
                    Vec::new(),
                ))
            }
        }
        // ingest 打通 ingestion→storage→search 全链路：读取原始 .jsonl →
        // provider.probe 判定 variant → provider.parse 流式产出 Canonical 消息 →
        // 每条写 catalog + FTS 索引。文件严格只读（RFC-0002 §7）。
        "ingest" => {
            no_extra_args(rest, 1, "ingest <file>")?;
            let path = arg(rest, 1, "ingest <file>")?;
            let (data, warnings) = ingest_file(store, path)?;
            Ok((
                "ingest",
                protocol::Outcome::Success,
                data,
                protocol::Page::default(),
                warnings,
            ))
        }
        // sync 对显式列出的多个源执行同一套只读快照 + staging，并在全部成功后
        // 通过一次 durable batch 提交，避免部分 source 已写入、后续 source 失败。
        // jsonl 模式下逐源发 progress frame（contract §4；--robot/Json 禁 progress）。
        // `--discover` 是 opt-in：遍历各 provider 的数据根自动发现源，不传路径。
        "sync" => {
            let mut args = rest.to_vec();
            let discover = take_bool_flag(&mut args, "--discover");
            let from_file = extract_flag(&mut args, "--from-file")?;
            // 显式消歧（M2P-16）：pi 与 openclaw 的磁盘格式同形，任何内容 probe
            // 都区分不了它们。discover 有 root 上下文可用，显式 `sync <file>`
            // 没有——那时用户点名是唯一证据。歧义错误让用户"name the provider
            // explicitly"，这个 flag 就是它承诺的那条出路。
            let sync_provider = match extract_flag(&mut args, "--provider")? {
                Some(id) => Some(validated_sync_provider(&id)?),
                None => None,
            };
            let (data, warnings) = if discover {
                // --discover 不接受额外参数（路径由发现填充）。未知 flag 也不能
                // 静默忽略，否则拼写错误会伪装成成功的空发现。
                if from_file.is_some() {
                    return Err(CliError::usage(
                        "sync --discover 与 --from-file 互斥：前者从 provider 数据根发现路径，后者从清单读路径",
                    ));
                }
                if args.len() > 1 {
                    return Err(CliError::usage(
                        "sync --discover 不接受路径或额外 flag；路径由 provider 数据根自动发现",
                    ));
                }
                if sync_provider.is_some() {
                    return Err(CliError::usage(
                        "sync --discover 与 --provider 互斥：discover 从每个 provider 的数据根\
                         推断归属，点名单个 provider 会与它冲突",
                    ));
                }
                sync_discover(store, SyncProgress::for_mode(mode), request_id)?
            } else if let Some(list_path) = from_file {
                // 5000 个源文件的路径展开后超过 Windows 的命令行长度上限，进程根本
                // 起不来。`--from-file` 收一份每行一个路径的清单（ripgrep/xargs
                // 惯例），把长度限制从命令行搬到文件里。
                //
                // 这条缺口此前由调用方各自绕过：real_data_regression.py 与
                // synthetic_corpus.py 都实现了自己的分批逻辑——同一个 workaround
                // 写两遍，说明缺的是工具能力而不是脚本技巧。
                if args.len() > 1 {
                    return Err(CliError::usage(
                        "sync --from-file <list> 不接受额外路径：路径全部来自清单文件",
                    ));
                }
                let paths = read_source_list(&list_path)?;
                if paths.is_empty() {
                    return Err(CliError::usage(format!(
                        "sync --from-file: {list_path} 里没有任何路径（空行与 # 注释会被忽略）"
                    )));
                }
                sync_files(
                    store,
                    &paths,
                    sync_provider.as_deref(),
                    SyncProgress::for_mode(mode),
                    request_id,
                )?
            } else {
                no_flag_like_positional(
                    &args[1..],
                    "sync <file>... | sync --discover | sync --provider <id> <file>...",
                )?;
                sync_files(
                    store,
                    &args[1..],
                    sync_provider.as_deref(),
                    SyncProgress::for_mode(mode),
                    request_id,
                )?
            };
            Ok((
                "sync",
                protocol::Outcome::Success,
                data,
                protocol::Page::default(),
                warnings,
            ))
        }
        "search" => {
            let mut args = rest.to_vec();
            let cursor = extract_flag(&mut args, "--cursor")?;
            let max_items = extract_flag(&mut args, "--max-items")?;
            let max_bytes = extract_flag(&mut args, "--max-bytes")?;
            let providers = extract_repeated_flag(&mut args, "--provider")?;
            let since = extract_flag(&mut args, "--since")?;
            let until = extract_flag(&mut args, "--until")?;
            let include_system = take_bool_flag(&mut args, "--include-system");
            let group_by_session = take_bool_flag(&mut args, "--group-by-session");
            // #3 检索模式：--mode lexical|semantic|hybrid；默认 lexical。
            let retrieval_mode = match extract_flag(&mut args, "--mode")?.as_deref() {
                None | Some("lexical") => RetrievalMode::Lexical,
                Some("semantic") => RetrievalMode::Semantic,
                Some("hybrid") => RetrievalMode::Hybrid,
                Some(other) => {
                    return Err(CliError::usage(format!(
                        "--mode must be lexical|semantic|hybrid, got {other}"
                    )));
                }
            };
            // 结构化 facet 过滤（additive；默认无过滤，行为与旧版一致）。
            let main_only = take_bool_flag(&mut args, "--main-only");
            let subagent_only = take_bool_flag(&mut args, "--subagent-only");
            let include_sidechain = take_bool_flag(&mut args, "--include-sidechain");
            let tool_kind = extract_flag(&mut args, "--tool-kind")?;
            let tool_name = extract_flag(&mut args, "--tool-name")?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let filters = search_filters_from_flags(
                &providers,
                since.as_deref(),
                until.as_deref(),
                app.now_ms(),
            )?;
            let budget = budget_from_flags(max_items.as_deref(), max_bytes.as_deref(), None)?;
            no_extra_args(&args, 1, "search <query>")?;
            let sidechain = match (main_only, subagent_only) {
                (true, true) => {
                    return Err(CliError::usage(
                        "--main-only and --subagent-only are mutually exclusive",
                    ));
                }
                (true, false) => {
                    if include_sidechain {
                        return Err(CliError::usage(
                            "--include-sidechain conflicts with --main-only",
                        ));
                    }
                    SidechainFacet::MainOnly
                }
                (false, true) => {
                    if include_sidechain {
                        return Err(CliError::usage(
                            "--include-sidechain conflicts with --subagent-only",
                        ));
                    }
                    SidechainFacet::SubagentOnly
                }
                (false, false) => SidechainFacet::Include,
            };
            let kind = match tool_kind.as_deref() {
                None => None,
                Some("file") | Some("command") | Some("web") | Some("query") | Some("unknown") => {
                    Some(tool_kind.unwrap())
                }
                Some(other) => {
                    return Err(CliError::usage(format!(
                        "--tool-kind must be one of file|command|web|query|unknown, got {other}"
                    )));
                }
            };
            let facets = SearchFacets {
                sidechain,
                tool_kind: kind,
                tool_name,
            };
            // 页大小旋钮即 --max-items；未给时保守默认 20（App 内仍与 budget 取小）。
            let limit = if max_items.is_some() {
                budget.max_items
            } else {
                20
            };
            // #3 语义/混合模式：注入 SqliteStore 作为 SemanticIndex，并用同一
            // vectorizer 生成查询向量。`index embeddings` 未跑过时向量表为空，
            // store.is_ready() 为 false，Application 显式降级为 lexical_fallback
            // + warning（禁止静默切换）。
            let query = arg(&args, 1, "search <query>")?.to_string();
            let response = if retrieval_mode == RetrievalMode::Lexical {
                let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
                app.handle(AppRequest::Search {
                    query,
                    filters,
                    facets: facets.clone(),
                    limit,
                    cursor,
                    budget,
                    include_system,
                    group_by_session,
                    mode: retrieval_mode,
                    query_embedding: None,
                })?
            } else {
                // Prefer a verified local Candle E5 bundle when the binary was
                // built with --features semantic-candle and the operator has
                // imported weights; otherwise stay on the honest bigram-hash
                // path (and the store will lexical_fallback if empty).
                use agent_session_grep_application::embedding::{
                    BIGRAM_HASH_MODEL_ID, BigramHashModel,
                };
                use agent_session_grep_ports::EmbeddingModel;

                let (model_id, query_embedding) = {
                    #[cfg(feature = "semantic-candle")]
                    {
                        let cache = platform_paths().ok().and_then(|v| {
                            v.get("cache")
                                .and_then(|c| c.as_str())
                                .map(|s| s.to_string())
                        });
                        if let Some(cache) = cache {
                            let dir =
                                agent_session_grep_application::candle_embedding::default_model_dir(
                                    std::path::Path::new(&cache),
                                );
                            if let Ok(model) =
                                agent_session_grep_application::candle_embedding::CandleE5Model::load_cached(
                                    &dir,
                                )
                            {
                                let emb = model.embed(&query, true).map_err(ProtocolError::from)?;
                                (model.manifest().model_id.clone(), emb)
                            } else {
                                let model = BigramHashModel::new();
                                let emb = model.embed(&query, true).map_err(ProtocolError::from)?;
                                (BIGRAM_HASH_MODEL_ID.to_string(), emb)
                            }
                        } else {
                            let model = BigramHashModel::new();
                            let emb = model.embed(&query, true).map_err(ProtocolError::from)?;
                            (BIGRAM_HASH_MODEL_ID.to_string(), emb)
                        }
                    }
                    #[cfg(not(feature = "semantic-candle"))]
                    {
                        let model = BigramHashModel::new();
                        let emb = model.embed(&query, true).map_err(ProtocolError::from)?;
                        (BIGRAM_HASH_MODEL_ID.to_string(), emb)
                    }
                };
                store.set_semantic_model(&model_id);
                // 每次查询都重设候选查询原文，开启 M2-4 的分层检索。必须逐请求
                // 设置：留着上一次的原文会让它去筛这一次的候选集。
                store.set_semantic_candidate_query(query.as_str());
                let app = App::with_resume_semantic(
                    store_ref(store),
                    store_ref(store),
                    store_ref(store),
                    store_ref(store),
                );
                app.handle(AppRequest::Search {
                    query,
                    filters,
                    facets: facets.clone(),
                    limit,
                    cursor,
                    budget,
                    include_system,
                    group_by_session,
                    mode: retrieval_mode,
                    query_embedding: Some(query_embedding),
                })?
            };
            let (outcome, mut data, page, warnings) = render(response);
            if mode == protocol::OutputMode::Human {
                attach_session_resume_rows(store, &mut data, db, db_origin)?;
            }
            // Robot/机器面如实回显本次应用的 facet（默认值不回显——输出字节不变）。
            if !facets.is_default() {
                let echo = data.as_object_mut().expect("search data is an object");
                echo.insert(
                    "facets".into(),
                    serde_json::json!({
                        "sidechain": facets.sidechain.as_str(),
                        "tool_kind": facets.tool_kind,
                        "tool_name": facets.tool_name,
                    }),
                );
            }
            Ok(("search", outcome, data, page, warnings))
        }
        "handoff" => {
            let mut args = rest.to_vec();
            let max_evidence = extract_flag(&mut args, "--max-evidence")?;
            let max_tokens = extract_flag(&mut args, "--max-tokens")?;
            let max_bytes = extract_flag(&mut args, "--max-bytes")?;
            let providers = extract_repeated_flag(&mut args, "--provider")?;
            let since = extract_flag(&mut args, "--since")?;
            let until = extract_flag(&mut args, "--until")?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let filters = search_filters_from_flags(
                &providers,
                since.as_deref(),
                until.as_deref(),
                app.now_ms(),
            )?;
            no_extra_args(&args, 1, "handoff <query>")?;
            let query = arg(&args, 1, "handoff <query>")?.to_string();
            let search_limit = 50usize;
            // 检索作为装配源：用宽松的 fetch-all 预算（含全文级 snippet），pack
            // 预算由包构建器单一执行（设计 D3——Context 路径的独立 clamp 会双重
            // 应用用户预算）。max_snippet_chars 放大到 schema 上限，让证据尽量
            // 携带原文而非 512 字符截断摘要。
            let response = app.handle(AppRequest::Search {
                query: query.clone(),
                filters: filters.clone(),
                facets: SearchFacets::default(),
                limit: search_limit,
                cursor: None,
                budget: ResponseBudget {
                    max_items: search_limit,
                    max_response_bytes: 64 * 1024 * 1024,
                    max_snippet_chars: 65536,
                    max_messages: search_limit,
                    max_evidence_spans: 512,
                },
                include_system: false,
                group_by_session: false,
                mode: RetrievalMode::Lexical,
                query_embedding: None,
            })?;
            let (hits, generation) = match &response {
                AppResponse::Search {
                    hits, generation, ..
                } => (hits.clone(), *generation),
                _ => return Err(CliError::usage("handoff: unexpected search response")),
            };
            let max_evidence_n = max_evidence
                .as_deref()
                .map(|v| {
                    v.parse::<usize>()
                        .map_err(|_| CliError::usage("--max-evidence 需要正整数"))
                })
                .transpose()?
                .unwrap_or(20);
            let max_tokens_n = max_tokens
                .as_deref()
                .map(|v| {
                    v.parse::<usize>()
                        .map_err(|_| CliError::usage("--max-tokens 需要正整数"))
                })
                .transpose()?
                .unwrap_or(8000);
            let max_bytes_n = max_bytes
                .as_deref()
                .map(|v| {
                    v.parse::<usize>()
                        .map_err(|_| CliError::usage("--max-bytes 需要正整数"))
                })
                .transpose()?
                .unwrap_or(2_000_000);
            // 权威 source locator：批量解析每条命中的 source document + span。
            let source_locations =
                agent_session_grep_application::handoff_pack::resolve_source_locations(
                    store, &hits,
                )
                .map_err(|e| CliError(e.into()))?;
            // Tool activities for the hit messages (schema v12). Empty when none.
            let hit_ids: Vec<_> = hits.iter().map(|h| h.id.clone()).collect();
            let tool_activities = store
                .tool_activities_for_messages(&hit_ids)
                .map_err(|e| CliError(e.into()))?;
            let message_facts: Vec<_> = store
                .message_facts_for(&hit_ids)
                .map_err(|e| CliError(e.into()))?
                .into_iter()
                .map(|(message_id, role, is_sidechain)| {
                    agent_session_grep_application::handoff_pack::MessageFact {
                        message_id,
                        role,
                        is_sidechain,
                    }
                })
                .collect();
            // Provider attribution for exactly the matched sessions: one
            // batched claims read (no N+1). The pack never queries storage.
            let session_ids =
                agent_session_grep_application::handoff_pack::matched_session_ids(&hits);
            let session_providers: Vec<_> = store
                .resume_of(&session_ids)
                .map_err(|e| CliError(e.into()))?
                .into_iter()
                .map(
                    |metadata| agent_session_grep_application::handoff_pack::SessionProvider {
                        session_id: metadata.session_id.as_str().to_string(),
                        provider_id: metadata.provider_id,
                    },
                )
                .collect();
            let pack = agent_session_grep_application::handoff_pack::generate_deterministic(
                HandoffInput {
                    query_terms: std::slice::from_ref(&query),
                    retrieval_mode: agent_session_grep_ports::RetrievalMode::Lexical,
                    filters: agent_session_grep_ports::handoff::HandoffFilters {
                        providers: filters
                            .providers
                            .iter()
                            .map(|p| p.as_str().to_string())
                            .collect(),
                        since: filters
                            .since
                            .map(|s| format!("{}.{:09}Z", s.unix_seconds, s.nanosecond)),
                        until: filters
                            .until
                            .map(|s| format!("{}.{:09}Z", s.unix_seconds, s.nanosecond)),
                    },
                    hits: &hits,
                    source_locations: &source_locations,
                    tool_activities: &tool_activities,
                    message_facts: &message_facts,
                    session_providers: &session_providers,
                    catalog_generation: generation,
                    max_tokens: max_tokens_n as u64,
                    max_bytes: max_bytes_n as u64,
                    max_evidence: max_evidence_n,
                    target: None,
                },
            );
            // 预算截断 → partial（exit 10），绝不伪装 success（contract §5）。
            let outcome = if pack.truncation.truncated {
                protocol::Outcome::Partial
            } else {
                protocol::Outcome::Success
            };
            Ok((
                "handoff",
                outcome,
                serde_json::to_value(&pack)
                    .map_err(|e| CliError::usage(format!("handoff: serialization error: {e}")))?,
                protocol::Page::default(),
                Vec::new(),
            ))
        }
        "get-message" => {
            let mut args = rest.to_vec();
            let session = extract_flag(&mut args, "--session")?;
            let around_value = extract_flag(&mut args, "--around")?;
            let around = around_value
                .as_deref()
                .map(str::parse::<usize>)
                .transpose()
                .map_err(|_| CliError::usage("--around must be a non-negative integer"))?
                .unwrap_or(0);
            let max_items = extract_flag(&mut args, "--max-items")?;
            let max_bytes = extract_flag(&mut args, "--max-bytes")?;
            let budget = budget_from_flags(max_items.as_deref(), max_bytes.as_deref(), None)?;
            no_extra_args(&args, 1, "get-message <message-wire-id>")?;
            let wire = arg(&args, 1, "get-message <message-wire-id>")?;
            let message_id = StableId::from_wire(wire)
                .filter(|id| id.kind() == IdKind::Message)
                .ok_or_else(|| CliError::usage(format!("not a valid message id: {wire}")))?;
            let session_id = session
                .as_deref()
                .map(|wire| {
                    StableId::from_wire(wire)
                        .filter(|id| id.kind() == IdKind::Session)
                        .ok_or_else(|| CliError::usage(format!("not a valid session id: {wire}")))
                })
                .transpose()?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Message {
                message_id,
                session_id,
                around,
                budget,
            })?;
            let (outcome, data, page, warnings) = render(response);
            Ok(("get-message", outcome, data, page, warnings))
        }
        "get" => {
            no_extra_args(rest, 1, "get <wire-id>")?;
            let wire = arg(rest, 1, "get <wire-id>")?;
            let id = StableId::from_wire(wire)
                .ok_or_else(|| CliError::usage(format!("not a valid entity id: {wire}")))?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Get { id: id.clone() })?;
            // 未找到实体：按 error catalog 映射 exit 4，而非当成功渲染 "not found"
            // （10 角色体验测试缺陷：show/get 不存在 ID 返回 exit 0，脚本无法区分）。
            // 消息固定为通用文案，不回显 wire/native ID（R2.1 隐私）。
            if matches!(&response, AppResponse::Get { payload: None }) {
                return Err(CliError(ProtocolError::new(
                    CanonicalCode::NotFound,
                    "entity not found",
                )));
            }
            let (outcome, data, page, warnings) = render(response);
            Ok(("get", outcome, data, page, warnings))
        }
        "show" => {
            no_extra_args(rest, 1, "show <wire-id>")?;
            let wire = arg(rest, 1, "show <wire-id>")?;
            let id = StableId::from_wire(wire)
                .ok_or_else(|| CliError::usage(format!("not a valid entity id: {wire}")))?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Show { id: id.clone() })?;
            if matches!(&response, AppResponse::Show { payload: None }) {
                return Err(CliError(ProtocolError::new(
                    CanonicalCode::NotFound,
                    "entity not found",
                )));
            }
            let (outcome, data, page, warnings) = render(response);
            Ok(("show", outcome, data, page, warnings))
        }
        "resume" => {
            // Resume 执行层（#5 + audit P1-2）：默认 dry-run 预览完整命令；
            // `--yes` 显式 opt-in 才实际 spawn provider 进程。首次使用无论是否
            // `--yes` 都强制只预览一次（持久标记），标记确认后才允许 `--yes`
            // 直接执行。未核验 resume 命令的 provider 恒为不可恢复（null/—），
            // 绝不编造命令。
            let mut args = rest.to_vec();
            let confirmed = take_bool_flag(&mut args, "--yes");
            no_extra_args(&args, 1, "resume <session-id>")?;
            let wire = arg(&args, 1, "resume <session-id>")?;
            let id = StableId::from_wire(wire)
                .filter(|id| id.kind() == IdKind::Session)
                .ok_or_else(|| CliError::usage(format!("not a valid session id: {wire}")))?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::GetSessionResume {
                session_id: id.clone(),
            })?;
            let AppResponse::SessionResume(metadata) = response else {
                return Err(CliError::usage("resume: unexpected response"));
            };
            let preview =
                agent_session_grep_application::resume::build_resume_descriptor(&metadata);
            let mut data = serde_json::json!({
                "session_id": metadata.session_id.as_str(),
                "provider_id": metadata.provider_id,
                "available": preview.available,
                "command": if preview.command_string.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(preview.command_string)
                },
                "working_directory": preview.descriptor.working_directory,
                "permission_mode": preview.descriptor.permission_mode,
                // 恒 false，且是策略而非缺口：我们不加任何权限 flag，也不宣称
                // 核验过 provider 会用什么权限模式（只读 Resume Metadata 里
                // 没有这个事实）。理由与消费口径见
                // `resume::RESUME_PERMISSION_MODE_VERIFIED`。
                "permission_mode_verified":
                    agent_session_grep_application::resume::RESUME_PERMISSION_MODE_VERIFIED,
                "unavailable_reason": preview.unavailable_reason,
                "executed": false,
            });
            // 不可恢复不是错误：历史恒可检索，只是不可恢复（ADR-0009）。
            if !preview.available {
                return Ok((
                    "resume",
                    protocol::Outcome::Success,
                    data,
                    protocol::Page::default(),
                    Vec::new(),
                ));
            }
            let mut warnings = Vec::new();
            // 首次强制预览（PRD Q24）：持久标记缺失时，即使 `--yes` 也只预览
            // 不执行，并落标记；标记写失败时安全侧继续强制预览（fail closed）。
            let marker_root = resume_marker_root(db);
            let first_run =
                !agent_session_grep_application::resume::resume_preview_acknowledged(&marker_root);
            if first_run {
                match agent_session_grep_application::resume::acknowledge_resume_preview(
                    &marker_root,
                ) {
                    Ok(()) => {
                        if confirmed {
                            data["first_run_preview"] = serde_json::json!(true);
                            warnings.push(
                                "首次使用 resume：已强制预览未执行。再次运行 resume --yes <session-id> 确认后才会真正执行。"
                                    .to_string(),
                            );
                        }
                    }
                    Err(error) => {
                        warnings.push(format!(
                            "resume: 首次预览标记写入失败（将继续强制预览）：{error}"
                        ));
                    }
                }
                return Ok((
                    "resume",
                    protocol::Outcome::Success,
                    data,
                    protocol::Page::default(),
                    warnings,
                ));
            }
            if confirmed {
                execute_resume(&preview.descriptor)?;
                data["executed"] = serde_json::json!(true);
            } else {
                warnings.push("dry-run：未执行。确认命令无误后加 --yes 实际恢复会话。".to_string());
            }
            Ok((
                "resume",
                protocol::Outcome::Success,
                data,
                protocol::Page::default(),
                warnings,
            ))
        }
        "hook" => {
            // Claude Code Hook（#8）：默认关闭，用户显式 --enable 才注入历史上下文。
            // payload 从 stdin 读，输出走 Claude Code 的 hookSpecificOutput 契约。
            // 未启用时输出空 context——不报错、不注入任何历史。
            let mut args = rest.to_vec();
            let max_tokens_flag = extract_flag(&mut args, "--max-tokens")?;
            let enabled = take_bool_flag(&mut args, "--enable");
            // #8 hook provider/time filter：--provider 可重复（OR），--decay-days
            // 限定时间窗（0 = 不过滤）。两者都是 opt-in，缺省空/0 = 全部历史。
            let providers = extract_repeated_flag(&mut args, "--provider")?;
            let decay_days = extract_flag(&mut args, "--decay-days")?;
            no_extra_args(&args, 1, "hook <session-start|user-prompt-submit>")?;
            let raw_event = arg(&args, 1, "hook <session-start|user-prompt-submit>")?;
            let event = hooks::HookEvent::parse(raw_event).ok_or_else(|| {
                CliError::usage("hook <event>: event must be session-start|user-prompt-submit")
            })?;
            let max_tokens = max_tokens_flag
                .as_deref()
                .map(|v| {
                    v.parse::<u64>()
                        .map_err(|_| CliError::usage("--max-tokens 需要非负整数"))
                })
                .transpose()?
                .unwrap_or(2000);
            let config = hooks::HookConfig {
                enabled,
                max_tokens,
                providers,
                decay_days: decay_days
                    .as_deref()
                    .map(|v| {
                        v.parse::<u32>()
                            .map_err(|_| CliError::usage("--decay-days 需要非负整数"))
                    })
                    .transpose()?
                    .unwrap_or(0),
                ..Default::default()
            };
            // stdin payload 允许为空（手工调用/探测）：空即无 query，注入空 context。
            let mut raw_payload = String::new();
            use std::io::Read as _;
            std::io::stdin()
                .read_to_string(&mut raw_payload)
                .map_err(|_| CliError::usage("hook: failed to read payload from stdin"))?;
            let payload: serde_json::Value = if raw_payload.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(&raw_payload)
                    .map_err(|_| CliError::usage("hook: payload is not valid JSON"))?
            };
            let query = hooks::query_from_payload(event, &payload);
            let (text, hits_count) = match (config.should_run(), query) {
                (true, Some(query)) => {
                    let app =
                        App::with_resume(store_ref(store), store_ref(store), store_ref(store));
                    let response = app.handle(AppRequest::Search {
                        query: query.clone(),
                        filters: hook_search_filters(&config, app.now_ms())?,
                        facets: SearchFacets::default(),
                        limit: 10,
                        cursor: None,
                        budget: ResponseBudget::default(),
                        include_system: false,
                        group_by_session: true,
                        mode: RetrievalMode::Lexical,
                        query_embedding: None,
                    })?;
                    let AppResponse::Search { hits, .. } = response else {
                        return Err(CliError::usage("hook: unexpected search response"));
                    };
                    let mut text = hooks::format_context_header(&query, hits.len());
                    for hit in &hits {
                        // Hook 注入是跨边界输出：文本经脱敏后才进入其他 agent 上下文。
                        let (body, _) = redaction::redact_text(hit.text.as_deref().unwrap_or(""));
                        text.push_str(&format!("- [{}] {}\n", hit.id.as_str(), body));
                    }
                    let count = hits.len();
                    (text, count)
                }
                _ => (String::new(), 0usize),
            };
            let output = hooks::build_hook_output(&text, config.max_tokens);
            let mut data = serde_json::to_value(&output)
                .map_err(|e| CliError::usage(format!("hook: serialization error: {e}")))?;
            if let Some(object) = data.as_object_mut() {
                object.insert("event".into(), serde_json::json!(event.as_str()));
                object.insert("enabled".into(), serde_json::json!(config.should_run()));
                object.insert("offline".into(), serde_json::json!(offline));
                object.insert("hits".into(), serde_json::json!(hits_count));
            }
            Ok((
                "hook",
                protocol::Outcome::Success,
                data,
                protocol::Page::default(),
                Vec::new(),
            ))
        }
        "get-session-resume" => {
            // 只读 Resume Metadata（ADR-0009）：只返回结构化字段，绝不构造/执行
            // shell 命令、绝不返回 transcript/source path。
            no_extra_args(rest, 1, "get-session-resume <session-id>")?;
            let wire = arg(rest, 1, "get-session-resume <session-id>")?;
            let id = StableId::from_wire(wire)
                .ok_or_else(|| CliError::usage(format!("not a valid entity id: {wire}")))?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::GetSessionResume {
                session_id: id.clone(),
            })?;
            let (outcome, data, page, warnings) = render(response);
            Ok(("get-session-resume", outcome, data, page, warnings))
        }
        "list" => {
            let mut args = rest.to_vec();
            let cursor = extract_flag(&mut args, "--cursor")?;
            let max_items = extract_flag(&mut args, "--max-items")?;
            let max_bytes = extract_flag(&mut args, "--max-bytes")?;
            let budget = budget_from_flags(max_items.as_deref(), max_bytes.as_deref(), None)?;
            no_extra_args(&args, 1, "list [limit]")?;
            let limit = args
                .get(1)
                .map(|s| s.parse::<usize>())
                .transpose()
                .map_err(|_| CliError::usage("list [limit]: limit must be an integer"))?
                .unwrap_or(if max_items.is_some() {
                    budget.max_items
                } else {
                    20
                });
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::List {
                limit,
                cursor,
                budget,
                sessions_only: false,
            })?;
            let (outcome, data, page, warnings) = render(response);
            Ok(("list", outcome, data, page, warnings))
        }
        // context：装配一个会话的分支消息链 + 证据区间（CONTRACT §1-2）。
        "context" => {
            let mut args = rest.to_vec();
            let policy = match extract_flag(&mut args, "--policy")?.as_deref() {
                None | Some("mainline") => ContextPolicy::Mainline,
                Some("full") => ContextPolicy::Full,
                Some(other) => {
                    return Err(CliError::usage(format!(
                        "--policy must be mainline|full, got {other}"
                    )));
                }
            };
            let max_messages = extract_flag(&mut args, "--max-messages")?;
            let max_bytes = extract_flag(&mut args, "--max-bytes")?;
            let level = match extract_flag(&mut args, "--level")?.as_deref() {
                None | Some("raw") => ContextLevel::Raw,
                Some("talks") => ContextLevel::Talks,
                Some("sessions") => ContextLevel::Sessions,
                Some(other) => {
                    return Err(CliError::usage(format!(
                        "--level must be raw|talks|sessions, got {other}"
                    )));
                }
            };
            let budget = budget_from_flags(None, max_bytes.as_deref(), max_messages.as_deref())?;
            no_extra_args(&args, 1, "context <session-wire-id>")?;
            let wire = arg(&args, 1, "context <session-wire-id>")?;
            let session_id = StableId::from_wire(wire)
                .filter(|id| id.kind() == IdKind::Session)
                .ok_or_else(|| CliError::usage(format!("not a valid session id: {wire}")))?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Context {
                session_id,
                policy,
                level,
                budget,
            })?;
            let (outcome, data, page, warnings) = render(response);
            Ok(("context", outcome, data, page, warnings))
        }
        "status" => {
            no_extra_args(rest, 0, "status")?;
            let app = App::with_resume(store_ref(store), store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Status)?;
            let (outcome, data, page, warnings) = render(response);
            Ok(("status", outcome, data, page, warnings))
        }
        other => Err(unknown_subcommand_error(other)),
    }
}

/// 从参数向量中取走一个带值 flag；不在场返回 `None`，在场缺值是用法错误。
fn extract_flag(args: &mut Vec<String>, name: &str) -> Result<Option<String>, CliError> {
    let Some(i) = args.iter().position(|a| a == name) else {
        return Ok(None);
    };
    if i + 1 >= args.len() {
        return Err(CliError::usage(format!("{name} requires a value")));
    }
    let value = args.remove(i + 1);
    args.remove(i);
    Ok(Some(value))
}

/// 可重复带值 flag 的收集变体（`--provider`）：按出现顺序取走全部取值；
/// 在场缺值是用法错误，重复出现是合法累积（provider 维度按 OR 语义）。
fn extract_repeated_flag(args: &mut Vec<String>, name: &str) -> Result<Vec<String>, CliError> {
    let mut values = Vec::new();
    while let Some(i) = args.iter().position(|a| a == name) {
        if i + 1 >= args.len() {
            return Err(CliError::usage(format!("{name} requires a value")));
        }
        values.push(args.remove(i + 1));
        args.remove(i);
    }
    Ok(values)
}

/// 布尔 flag 提取（`--include-system`/`--group-by-session`）：在场移除该 token
/// 并返回 true，缺场返回 false。不消费取值；重复出现视为在场一次。
fn take_bool_flag(args: &mut Vec<String>, name: &str) -> bool {
    if let Some(i) = args.iter().position(|a| a == name) {
        args.remove(i);
        true
    } else {
        false
    }
}

/// CLI 检索过滤参数归一化：provider 别名 → 规范 id；时间值接受 RFC3339/ISO-8601
/// 绝对时间或 `1h|1d|1w` 紧凑相对量（相对量以注入的 application 时钟 `now_ms`
/// 为基准，全程同一时钟源）。取值非法是用法错误（exit 2）。
fn search_filters_from_flags(
    providers: &[String],
    since: Option<&str>,
    until: Option<&str>,
    now_ms: i64,
) -> Result<SearchFilters, CliError> {
    let mut filters = SearchFilters::default();
    for provider in providers {
        filters
            .providers
            .push(canonical_search_provider(provider).ok_or_else(|| {
                let hint = provider_value_hint();
                CliError::usage(format!("unknown provider: {provider} (expected {hint})"))
            })?);
    }
    filters.since = parse_time_flag("--since", since, now_ms)?;
    filters.until = parse_time_flag("--until", until, now_ms)?;
    Ok(filters)
}

/// Accepted `--provider` values, rendered in every provider usage error.
///
/// 从能力矩阵派生而非写死:写死的名单会在加 provider 时静默过期,而这条提示
/// 恰恰是用户唯一能看到的合法取值来源。`claude` 是 `claude-code` 的历史别名,
/// 单独列出。
fn provider_value_hint() -> String {
    let mut ids = SearchProvider::accepted_ids();
    ids.sort();
    format!("claude|{}", ids.join("|"))
}

/// Normalize one provider request value to [`SearchProvider`]; `None` = unknown.
///
/// Accepts the canonical provider ids that every machine surface already
/// publishes ([`SearchProvider::as_str`], `providers` / `list_providers` /
/// `/api/providers` `provider_id`) plus the historical short aliases. The
/// embedded Web UI fills its provider selector from `/api/providers`, so a
/// canonical id must be a first-class value here — otherwise
/// `/api/search?provider=claude-code` fails `invalid_request` while the
/// equivalent CLI alias succeeds, which is exactly the CLI/Web fork the
/// loopback surface forbids. Unknown values stay fail-closed (usage error at
/// the caller), never silently dropped.
fn canonical_search_provider(provider: &str) -> Option<SearchProvider> {
    // `claude` 是 `claude-code` 的历史别名,保留;其余一律按 canonical id 走
    // 矩阵校验(合法性的真源是矩阵,不是这里的名单)。
    let canonical = if provider == "claude" {
        "claude-code"
    } else {
        provider
    };
    SearchProvider::parse(canonical)
}

/// 从 HookConfig 构建检索过滤（#8）：provider 白名单（空 = 全部）+ 时间衰减
/// （`decay_days` > 0 时 `since = now - decay_days`，旧历史整体排除；0 = 不过滤）。
/// provider 值经 [`parse_provider_value`] 归一，与 search `--provider` 同一套
/// canonical id 与别名。
/// 注入文本保持跨边界脱敏（ADR-0009）由调用方 hook 分支负责，本层只出过滤条件。
fn hook_search_filters(config: &hooks::HookConfig, now_ms: i64) -> Result<SearchFilters, CliError> {
    let mut filters = SearchFilters::default();
    for provider in &config.providers {
        filters
            .providers
            .push(canonical_search_provider(provider).ok_or_else(|| {
                let hint = provider_value_hint();
                CliError::usage(format!(
                    "hook --provider: unknown provider {provider} (expected {hint})"
                ))
            })?);
    }
    if config.decay_days > 0 {
        let day_ms = 86_400_000i64;
        let since_ms = now_ms.saturating_sub(i64::from(config.decay_days).saturating_mul(day_ms));
        filters.since = Some(SearchInstant::from_unix_millis(since_ms));
    }
    Ok(filters)
}

/// 解析单个时间 flag：先按绝对 RFC3339/ISO-8601，失败再按紧凑相对量；两者都
/// 不成立即用法错误（错误信息不含原始值回显以外的后端细节）。
fn parse_time_flag(
    name: &str,
    value: Option<&str>,
    now_ms: i64,
) -> Result<Option<agent_session_grep_ports::SearchInstant>, CliError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    if let Some(instant) = parse_search_instant(raw) {
        return Ok(Some(instant));
    }
    if let Some(instant) = parse_relative_search_instant(raw, now_ms) {
        return Ok(Some(instant));
    }
    Err(CliError::usage(format!(
        "{name} must be an RFC3339/ISO-8601 timestamp or a compact duration (1h|1d|1w), got {raw:?}"
    )))
}

/// 用 flag 覆盖默认预算；数值解析失败是用法错误，下限校验由 App 层统一执行。
fn budget_from_flags(
    max_items: Option<&str>,
    max_bytes: Option<&str>,
    max_messages: Option<&str>,
) -> Result<ResponseBudget, CliError> {
    let mut budget = ResponseBudget::default();
    if let Some(v) = max_items {
        budget.max_items = v
            .parse()
            .map_err(|_| CliError::usage("--max-items must be a non-negative integer"))?;
    }
    if let Some(v) = max_bytes {
        budget.max_response_bytes = v
            .parse()
            .map_err(|_| CliError::usage("--max-bytes must be a non-negative integer"))?;
    }
    if let Some(v) = max_messages {
        budget.max_messages = v
            .parse()
            .map_err(|_| CliError::usage("--max-messages must be a non-negative integer"))?;
    }
    Ok(budget)
}

/// 执行 resume：在原 cwd 下 spawn provider 进程并等待其退出（前台接管）。
///
/// 执行前校验 cwd 存在（存在但不可访问也在此暴露）与 provider 二进制在 PATH
/// 上（缺失即结构化错误、提示安装，不再等到 spawn 才报）；cwd 不存在、二进制
/// 缺失、进程非零退出都返回结构化错误，绝不静默。permission_mode 只在用户
/// 显式选择时出现在 descriptor 中（默认 None，不自动带 yolo）。
fn execute_resume(
    descriptor: &agent_session_grep_application::resume::ResumeDescriptor,
) -> Result<(), CliError> {
    if let Some(dir) = &descriptor.working_directory
        && !std::path::Path::new(dir).is_dir()
    {
        return Err(CliError(
            ProtocolError::new(
                CanonicalCode::SourceIo,
                "resume working directory does not exist",
            )
            .with_details(serde_json::json!({ "stage": "cwd_check" })),
        ));
    }
    // provider 二进制 preflight（audit P1-2）：缺失在 spawn 之前就报结构化
    // 错误并提示安装；dry-run 不经过这里（预览不校验二进制）。
    if !provider_binary_on_path(&descriptor.provider_binary) {
        return Err(CliError(
            ProtocolError::new(
                CanonicalCode::ProviderError,
                "provider binary not found on PATH; install the provider CLI or add it to PATH before resuming",
            )
            .with_details(serde_json::json!({
                "stage": "binary_preflight",
                "binary": descriptor.provider_binary,
            })),
        ));
    }
    let mut command = std::process::Command::new(&descriptor.provider_binary);
    command.args(&descriptor.args);
    if let Some(dir) = &descriptor.working_directory {
        command.current_dir(dir);
    }
    let status = command.status().map_err(|e| {
        // 二进制缺失理论上已被 preflight 拦截；此处兜底处理竞态（preflight 后
        // 才被移除）。不回显完整路径，只给可操作原因。
        let reason = if e.kind() == std::io::ErrorKind::NotFound {
            "provider binary not found on PATH"
        } else {
            "failed to start provider process"
        };
        CliError(
            ProtocolError::new(CanonicalCode::ProviderError, reason)
                .with_details(serde_json::json!({ "stage": "spawn" })),
        )
    })?;
    if !status.success() {
        return Err(CliError(
            ProtocolError::new(
                CanonicalCode::ProviderError,
                "provider exited with a non-zero status",
            )
            .with_details(serde_json::json!({
                "stage": "provider_exit",
                "exit_code": status.code(),
            })),
        ));
    }
    Ok(())
}

/// 解析 resume 首次预览标记所在的 data root：与 writer lease 同一根——
/// db 文件所在目录（裸相对文件名按当前工作目录）。
fn resume_marker_root(db: &str) -> std::path::PathBuf {
    let db_path = std::path::Path::new(db);
    match db_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    }
}

/// provider 二进制是否能在 PATH 上解析（`Command::new` 的查找近似）。
///
/// Unix 要求是普通文件且带执行位；Windows 的 CreateProcess 会依次查找
/// `.exe`/`.com`/`.bat`/`.cmd`，这里覆盖 `.exe`/`.cmd`/`.bat`。
fn provider_binary_on_path(binary: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| {
        let candidate = dir.join(binary);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(&candidate)
                .ok()
                .is_some_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        {
            candidate.is_file()
                || ["exe", "cmd", "bat"]
                    .iter()
                    .any(|ext| candidate.with_extension(ext).is_file())
        }
    })
}

/// 从权威 catalog 重建语义向量索引（`index embeddings`，#3）。
///
/// 与 FTS rebuild 同一语义：向量表是 catalog 的投影，整表清空后按 catalog
/// 重投影，失败不留半成品（每条 upsert 独立，重跑幂等）。只对 Message 实体
/// 建向量——session/document 没有检索正文。
///
/// Model selection:
/// - With `--features semantic-candle` and a verified local E5 bundle under the
///   platform cache (`config paths` → cache/models/...), uses the real Candle
///   multilingual-e5-small encoder.
/// - Otherwise falls back to the honest bigram-hash fuzzy-lexical vectorizer
///   and emits the experimental warning (default release path).
fn build_embeddings(store: &SqliteStore) -> Result<(serde_json::Value, Vec<String>), CliError> {
    use agent_session_grep_application::embedding::BigramHashModel;
    use agent_session_grep_ports::{CatalogStore, EmbeddingModel, SemanticIndex};

    enum ActiveModel {
        Bigram(BigramHashModel),
        // Boxed: the loaded BERT model is far larger than BigramHashModel, and
        // the enum only lives for the duration of this rebuild (clippy
        // large_enum_variant). Semantics unchanged.
        #[cfg(feature = "semantic-candle")]
        Candle(Box<agent_session_grep_application::candle_embedding::CandleE5Model>),
    }

    impl ActiveModel {
        fn embed(&self, text: &str, is_query: bool) -> Result<Vec<f32>, ProtocolError> {
            match self {
                Self::Bigram(m) => m.embed(text, is_query).map_err(ProtocolError::from),
                #[cfg(feature = "semantic-candle")]
                Self::Candle(m) => m.embed(text, is_query).map_err(ProtocolError::from),
            }
        }
        fn model_id(&self) -> &str {
            match self {
                Self::Bigram(m) => m.manifest().model_id.as_str(),
                #[cfg(feature = "semantic-candle")]
                Self::Candle(m) => m.manifest().model_id.as_str(),
            }
        }
        fn manifest_json(&self) -> serde_json::Value {
            match self {
                Self::Bigram(m) => {
                    let man = m.manifest();
                    serde_json::json!({
                        "model_id": man.model_id,
                        "dimension": man.dimension,
                        "license": man.license,
                        "file_hash": man.file_hash,
                        "backend": "bigram-hash",
                    })
                }
                #[cfg(feature = "semantic-candle")]
                Self::Candle(m) => {
                    let man = m.manifest();
                    serde_json::json!({
                        "model_id": man.model_id,
                        "dimension": man.dimension,
                        "license": man.license,
                        "file_hash": man.file_hash,
                        "backend": "semantic-candle",
                    })
                }
            }
        }
        fn warning(&self) -> Option<String> {
            match self {
                Self::Bigram(_) => Some(
                    "当前向量化器是 bigram-hash（模糊词法相似，非语义）；semantic/hybrid 因此仅供实验，lexical 仍是默认。"
                        .to_string(),
                ),
                #[cfg(feature = "semantic-candle")]
                Self::Candle(_) => None,
            }
        }
    }

    let model = {
        #[cfg(feature = "semantic-candle")]
        {
            let cache = platform_paths().ok().and_then(|v| {
                v.get("cache")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string())
            });
            if let Some(cache) = cache {
                let dir = agent_session_grep_application::candle_embedding::default_model_dir(
                    std::path::Path::new(&cache),
                );
                match agent_session_grep_application::candle_embedding::CandleE5Model::load_from_dir(
                    &dir,
                ) {
                    Ok(m) => ActiveModel::Candle(Box::new(m)),
                    Err(_) => ActiveModel::Bigram(BigramHashModel::new()),
                }
            } else {
                ActiveModel::Bigram(BigramHashModel::new())
            }
        }
        #[cfg(not(feature = "semantic-candle"))]
        {
            ActiveModel::Bigram(BigramHashModel::new())
        }
    };

    let model_id = model.model_id().to_string();
    store.set_semantic_model(&model_id);
    // 先清除本模型的旧向量：重建语义与 `index rebuild` 一致（整表重投影，
    // 不是增量补齐），否则 catalog 里已删除的实体会留下孤儿向量。
    let cleared = store
        .clear_embeddings(&model_id)
        .map_err(ProtocolError::from)?;

    // catalog 全量扫描：payload 里没有可检索正文的实体跳过（不臆造向量）。
    let entries = store.list(usize::MAX).map_err(ProtocolError::from)?;
    let mut indexed = 0usize;
    let mut skipped = 0usize;
    for entry in &entries {
        if entry.id.kind() != IdKind::Message {
            skipped += 1;
            continue;
        }
        let text = serde_json::from_slice::<serde_json::Value>(&entry.payload)
            .ok()
            .and_then(|value| {
                value
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if text.trim().is_empty() {
            skipped += 1;
            continue;
        }
        let vector = model.embed(&text, false)?;
        store
            .index_embedding(&entry.id, &vector)
            .map_err(ProtocolError::from)?;
        indexed += 1;
    }

    let mut data = model.manifest_json();
    if let Some(obj) = data.as_object_mut() {
        obj.insert("indexed".into(), serde_json::json!(indexed));
        obj.insert("skipped".into(), serde_json::json!(skipped));
        obj.insert("cleared".into(), serde_json::json!(cleared));
    }
    let warnings = model.warning().into_iter().collect();
    Ok((data, warnings))
}

/// 用与 CLI `index`/`get` 一致的派生路径，从一个 fact 造出 message id。
fn message_id(fact: &str) -> StableId {
    StableId::derive(
        IdKind::Message,
        Stability::Reconstructed,
        &[fact.as_bytes()],
    )
}

/// 写入一条：派生 id → durable outbox → 原子提交 catalog + FTS + generation。
fn index_one(store: &SqliteStore, fact: &str, text: &str) -> Result<serde_json::Value, CliError> {
    let id = message_id(fact);
    let entries = [(id.clone(), text.as_bytes().to_vec(), text.to_string())];
    store.commit_batch(&entries).map_err(ProtocolError::from)?;
    let generation = store.active_generation().map_err(ProtocolError::from)?;
    Ok(serde_json::json!({
        "indexed": id.as_str(),
        "generation": generation,
    }))
}

/// 借用 store 作为端口 trait 对象的辅助——App 需要两个泛型参数各持一份。
/// 由于 App<C,S> 按值持有两个后端，而我们只有一个 SqliteStore 实例，
/// 这里用 `&SqliteStore` 满足两个 trait 约束（trait 对 &T 亦实现）。
fn store_ref(store: &SqliteStore) -> &SqliteStore {
    store
}

/// 空 catalog 时给出下一步动作（M2P-4，仅 Human 模式）。
///
/// `search` → `no hits`、`list` → `catalog is empty`、`status` → `entities: 0`
/// 三条过去都没说出全新库上唯一正确的下一步动作。这里在 catalog 真为空时注入
/// `catalog_count` 与一条可直接复制的 `sync --discover` 命令，由 human 渲染器
/// 打印。只在 Human 模式附加，Robot/MCP 协议形状不受影响。
///
/// 只在结果本身为空时才查 catalog 计数——有命中的查询不付这次计数开销。
fn attach_empty_catalog_hint(
    store: &SqliteStore,
    command: &str,
    data: &mut serde_json::Value,
    db: &str,
    origin: DbOrigin,
) -> Result<(), CliError> {
    let looks_empty = match command {
        "search" => data
            .get("hits")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty),
        "list" => data
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty),
        // status 的 catalog_count 已在 data 里，直接读，不再查一次库。
        "status" => {
            data.get("catalog_count")
                .and_then(serde_json::Value::as_u64)
                == Some(0)
        }
        _ => false,
    };
    if !looks_empty {
        return Ok(());
    }
    // search/list 的空结果可能只是查询无命中或游标翻过了尾页；只有 catalog 真为
    // 空时才说"索引是空的"，否则会把"没搜到"误报成"没索引"。
    // 全限定调用：裸 `store.count()` 会解析到 Iterator::count。
    let catalog_count =
        agent_session_grep_ports::CatalogStore::count(store).map_err(ProtocolError::from)?;
    if catalog_count > 0 {
        return Ok(());
    }
    if let Some(object) = data.as_object_mut() {
        object.insert("catalog_count".into(), serde_json::json!(catalog_count));
        object.insert(
            "empty_catalog_next_command".into(),
            serde_json::json!(sync_discover_command(db, origin)),
        );
    }
    Ok(())
}

/// Human search 的会话表格行按 canonical Session 去重后批量解析 Resume
/// Metadata。此投影只在 Human 模式附加，Robot/MCP 协议形状不受影响。
///
/// 日期取该 Session 最近活动 timestamp 的 `YYYY-MM-DD`（批量一次查询，无 N+1）；
/// 标题取当前页中该 Session 的最高相关度命中 `text`——标题跟随搜索排序，
/// 与“相关度优先”不变量一致。两者缺失渲染为 `—`。
///
/// Session ID 列渲染 **canonical `ses_v1_…`**（M2P-1）。此前填的是
/// `provider_session_id`（provider 原生 id），而 `context`/`show`/
/// `get-session-resume` 只认 canonical id——照抄表格里的 id 会得到
/// `not a valid session id` exit 2，等于把 help 承诺的
/// "search → show → context" 数据流在 human 输出上截断。
/// canonical id 才是 catalog 身份（ADR-0009 / `CONTEXT.md` 双 id 规则），
/// provider 原生 id 只在 `resume` / `get-session-resume` 的输出里出现。
fn attach_session_resume_rows(
    store: &SqliteStore,
    data: &mut serde_json::Value,
    db: &str,
    origin: DbOrigin,
) -> Result<(), CliError> {
    let Some(hits) = data.get("hits").and_then(serde_json::Value::as_array) else {
        return Ok(());
    };
    let mut seen = BTreeSet::new();
    let session_ids: Vec<StableId> = hits
        .iter()
        .filter_map(|hit| hit.get("session_id").and_then(serde_json::Value::as_str))
        .filter(|wire| seen.insert((*wire).to_string()))
        .filter_map(StableId::from_wire)
        .filter(|id| id.kind() == IdKind::Session)
        .collect();
    let metadata = store.resume_of(&session_ids).map_err(ProtocolError::from)?;
    let latest_ymd = store
        .latest_activity_ymd_for_sessions(&session_ids)
        .map_err(ProtocolError::from)?;
    let rows: Vec<serde_json::Value> = metadata
        .iter()
        .map(|metadata| {
            let session_wire = metadata.session_id.as_str();
            let date = latest_ymd.get(session_wire).cloned();
            let title = hits
                .iter()
                .filter_map(|hit| {
                    let hit_session = hit.get("session_id").and_then(serde_json::Value::as_str)?;
                    (hit_session == session_wire)
                        .then_some(())
                        .and_then(|_| hit.get("text").and_then(serde_json::Value::as_str))
                })
                .next()
                .map(|text| text.to_string());
            serde_json::json!({
                "date": date,
                "provider": metadata.provider_id,
                "title": title,
                "working_directory": metadata.original_working_directory,
                // canonical id：`context`/`show`/`get-session-resume` 唯一接受的形状。
                // provider 原生 id 不进表格——需要它的 resume 场景由
                // `resume <canonical>` / `get-session-resume <canonical>` 解析。
                "session_id": session_wire,
            })
        })
        .collect();
    // 表格下方的下一步命令（M2P-1）：canonical id 已在 robot envelope 的
    // suggested_next_commands 里拼成完整命令，human 模式此前从不打印它。
    // 显式 `--db` 时补上同一路径，否则复制出来的命令会指向默认库。
    let next_commands = human_next_commands(hits, db, origin);
    if let Some(object) = data.as_object_mut() {
        object.insert("session_resume_rows".into(), serde_json::Value::Array(rows));
        if !next_commands.is_empty() {
            object.insert(
                "suggested_next_commands_human".into(),
                serde_json::json!(next_commands),
            );
        }
    }
    Ok(())
}

/// 取当前页最相关命中的建议命令，并按需插入 `--db <path>`。
///
/// Application 生成的命令以 `agent-session-grep` 开头且不带 `--db`（对默认库正确）。
/// 调用方给了显式 `--db` 时必须把同一路径补进去——否则提示会指向另一个库。
fn human_next_commands(hits: &[serde_json::Value], db: &str, origin: DbOrigin) -> Vec<String> {
    let prefix = db_flag_prefix(db, origin);
    hits.iter()
        .filter_map(|hit| {
            hit.get("suggested_next_commands")
                .and_then(serde_json::Value::as_array)
        })
        .find(|commands| !commands.is_empty())
        .map(|commands| {
            commands
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(|command| insert_db_flag(command, &prefix))
                .collect()
        })
        .unwrap_or_default()
}

/// 在建议命令的二进制名之后插入 `--db <path> `（前缀为空时原样返回）。
fn insert_db_flag(command: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return command.to_string();
    }
    match command.split_once(' ') {
        Some((binary, rest)) => format!("{binary} {prefix}{rest}"),
        None => command.to_string(),
    }
}

/// 组合根持有的 provider adapter 清单。ingest/sync 用它 probe-select，
/// 由 [`select_and_stage_source`] 挑出认领此源的 adapter（见 RFC-0002 §3）。
/// 新增 provider 只需在此登记一行。
fn provider_registry() -> Vec<Box<dyn ProviderAdapter>> {
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

/// 对一个可重复打开的只读 source probe-select 并 stage，同时返回选中 variant。
///
/// 空源保留整源清空/tombstone 语义；其它源的每次 probe/parse 都由 source
/// 重新打开 bounded reader（JSONL 逐行 / 整档格式按 manifest 上限），生产路径
/// 绝不把完整 transcript 变成 Vec（RFC-0002 §7）。
///
/// 返回 `AppError` 而非 `CliError`：调用方需要区分"没有 provider 认领这个文件"
/// （discover 可跳过，见 [`SyncContext::skip_unrecognized_sources`]）与真正的
/// 失败，而 `CliError` 已把分类拍平成协议错误。`?` 仍可直接用于 `CliError` 上下文。
fn stage_with_source(
    source: &dyn agent_session_grep_ports::ReadOnlySource,
) -> Result<(StagedBatch, String), AppError> {
    let registry = provider_registry();
    let refs: Vec<&dyn ProviderAdapter> = registry.iter().map(|a| a.as_ref()).collect();
    stage_with_registry(&refs, source, None)
}

/// [`stage_with_source`] against a caller-owned adapter registry.
///
/// Split out so a multi-source sync builds the 14 adapters once and shares the
/// borrowed slice across parse threads instead of re-boxing the whole registry
/// per file. `ProviderAdapter: Send + Sync`, so `&[&dyn ProviderAdapter]` is
/// safe to hand to every worker.
///
/// `provider_hint` 是 discover 记下的"此源来自哪个 provider 的数据根"。它只在
/// 内容 probe 同分、无法区分时用于消歧（见 `choose_probed_candidate`）。手动
/// `sync <file>` 没有 root 上下文，传 `None`。
fn stage_with_registry(
    registry: &[&dyn ProviderAdapter],
    source: &dyn agent_session_grep_ports::ReadOnlySource,
    provider_hint: Option<&str>,
) -> Result<(StagedBatch, String), AppError> {
    if source.is_empty() {
        return Ok((
            StagedBatch {
                messages: Vec::new(),
                activities: Vec::new(),
                report: ParseReport {
                    committed: 0,
                    skipped: 0,
                    diagnostics: Vec::new(),
                    session_native_id: None,
                    session_observation: ProviderSessionObservation::default(),
                },
                session_native_id: None,
            },
            "empty".into(),
        ));
    }
    select_and_stage_source(registry, source, provider_hint)
}

struct StagedMessageEntity {
    id: StableId,
    role: String,
    text: String,
    timestamp: Option<String>,
    occurrences: Vec<(MessagePlacement, Option<StableId>, Option<String>)>,
}

/// 把一个源的完整 staging 产物转成稳定实体 + contextual relations。
///
/// 三类实体与 placements/edges 随同一 [`SourceBatch`] 单事务提交：
///
/// - **消息**：身份优先用 provider-native id（Claude Code 的 `uuid`，tier `Native`），
///   provider 未给 native id 时使用 provider/variant/document/ordinal 的 path-free
///   `Unstable` fallback。重复 stable id 只保留一个实体，全部 occurrences 仍保留。
/// - **会话**：id 优先取 provider 报告的 native 会话 id（`ses_v1_` Native tier），
///   缺失回退对 document wire id 的 `Reconstructed` 派生。payload 引用 document
///   与按首次 occurrence 排序的去重成员消息 wire id。
/// - **文档**：内容寻址 `Reconstructed` 派生（provider/variant/fingerprint），
///   不含路径——身份不编码位置（RFC-0001）。payload 携 provider/variant/fingerprint/len。
///
/// `ParseReport.skipped > 0` 会使 source relation-incomplete；observed facts 可提交，
/// 但存储层不会推导 tombstone，且会撤销旧 completeness marker。
///
/// 从源路径推导该 provider 安装的 namespace：优先使用路径上最后一个 provider
/// 数据根（`.claude` / `.codex`）的完整路径，使同一安装下的 transcript
/// 共享 namespace、不同安装分离。手动 ingest 的 Source 若不在已知数据根下，
/// 以其共同父目录作为未知安装边界，避免同一 Session 分散在多个文件时被拆开。
fn installation_namespace(path: &str, provider_id: &str) -> String {
    let marker = match provider_id {
        "claude-code" => ".claude",
        "codex" => ".codex",
        _ => "",
    };
    let segments: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if !marker.is_empty()
        && let Some(index) = segments.iter().rposition(|s| *s == marker)
    {
        return format!("{provider_id}:{}", segments[..=index].join("/"));
    }
    let parent = match segments.split_last() {
        Some((_, parent)) if !parent.is_empty() => parent.join("/"),
        _ => ".".to_string(),
    };
    format!("{provider_id}:{parent}")
}

/// 解析当前用户 home 目录下某 provider 的规范化 transcript 数据根。
///
/// 与 [`installation_namespace`] 复用同一组 marker 常量（`.claude` / `.codex`）。
/// home 目录优先取 `HOME`（Unix），回退 `USERPROFILE`（Windows）；两者都缺失返回
/// `None`，调用方应跳过该 provider 的发现（R4：不猜路径）。
///
/// - `claude-code` → `~/.claude/projects`
/// - `codex` → `~/.codex/sessions`
/// - 未知 provider → `None`
fn provider_data_root(provider_id: &str) -> Option<std::path::PathBuf> {
    let sub = provider_root_subpath(provider_id)?;
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)?;
    Some(home.join(sub))
}

/// 某 provider 在 home 下的规范化 transcript 子路径；未注册 root 的 provider 返回
/// `None`。与 [`provider_data_root`] 分开，是为了让 discover 报告能区分
/// "该 provider 不支持自动发现" 与 "home 解析失败"——两者过去都塌成
/// `found: 0, complete: false`，用户无从判断该不该手动传文件（M2P-6）。
fn provider_root_subpath(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "claude-code" => Some(".claude/projects"),
        "codex" => Some(".codex/sessions"),
        "openclaw" => Some(".openclaw/agents"),
        "tencent-codebuddy" => Some(".codebuddy/projects"),
        "antigravity" => Some(".gemini/antigravity-cli/brain"),
        "opencode" => Some(".local/share/opencode"),
        _ => None,
    }
}

fn source_path_identity(path: &str) -> String {
    if !cfg!(windows) {
        return path.to_string();
    }
    let mut normalized = path.replace('\\', "/");
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_uppercase() {
        let drive = (bytes[0].to_ascii_lowercase() as char).to_string();
        normalized.replace_range(..1, &drive);
    }
    normalized
}

/// 发现期 probe 采样上限：每个候选文件最多读这么多前缀字节。
///
/// 只为回答"这看起来是不是某个 provider 的 transcript"，不做权威判定——权威
/// 判定在 sync 阶段由 `stage_with_source` 对完整快照重做。取 64 KiB：足够覆盖
/// 各 JSONL adapter 的取样窗口（前若干条记录），又不会让遍历一棵大目录树变成
/// 整树整读（`capture` 会为指纹整读文件，发现期绝不能用它）。
const DISCOVERY_PROBE_HEAD_BYTES: usize = 64 * 1024;

/// 读取文件前缀用于发现期 probe，并把截断位置对齐到安全边界。
///
/// 截断的尾巴会切坏最后一条记录/多字节字符：先退到最后一个换行符（行式格式
/// 因此只看到完整记录），没有换行符时退到合法 UTF-8 边界（Markdown/JSON 整档
/// adapter 的 probe 会先做 UTF-8 校验）。
fn read_discovery_head(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    file.take(DISCOVERY_PROBE_HEAD_BYTES as u64 + 1)
        .read_to_end(&mut head)
        .ok()?;
    if head.len() <= DISCOVERY_PROBE_HEAD_BYTES {
        return Some(head);
    }
    head.truncate(DISCOVERY_PROBE_HEAD_BYTES);
    if let Some(last_newline) = head.iter().rposition(|byte| *byte == b'\n') {
        head.truncate(last_newline + 1);
        return Some(head);
    }
    while !head.is_empty() && std::str::from_utf8(&head).is_err() {
        head.pop();
    }
    Some(head)
}

/// 发现期判定：这个文件是否值得当作候选源交给 sync。
///
/// 先按签名字节定家族（SQLite magic header vs 文本），只把文件交给同家族的
/// adapter probe——异家族的 probe 要么无意义，要么昂贵（SQLite 的
/// `probe_source` 会整读到 128 MiB 上限）。家族判定不会漏掉真源：每个 SQLite
/// 数据库都以 magic header 开头，而 JSONL / 整档 JSON / Markdown transcript 都是
/// UTF-8 文本、不可能以该 header 开头。
///
/// SQLite 家族只按 magic header 收下：文件是不是真的 opencode/cursor 库，由
/// sync 阶段的权威 probe 判定，判错的代价是一次 skip 计数而非整轮失败。
/// 文本家族要求至少一个同家族 adapter 给出非 Ambiguous 的候选置信度；歧义
/// （多个 variant 同分）依然被收下，好让 sync 如实报"歧义拒绝"而不是静默丢弃。
fn is_discovery_candidate(path: &std::path::Path, adapters: &[&dyn ProviderAdapter]) -> bool {
    let Some(head) = read_discovery_head(path) else {
        return false;
    };
    if head.is_empty() {
        // 空文件保留整源清空/tombstone 语义：曾入库的源被清空必须被看见。
        return true;
    }
    if SourceFormatFamily::of_head(&head) == SourceFormatFamily::Sqlite {
        return true;
    }
    adapters
        .iter()
        .filter(|adapter| {
            source_format_family_for(adapter.provider_id()) == SourceFormatFamily::Text
        })
        .any(|adapter| {
            adapter
                .probe(&head)
                .is_ok_and(|probe| probe.confidence != Confidence::Ambiguous)
        })
}

/// 递归遍历 `root`，收集候选源文件路径（正斜杠归一）。
///
/// 返回 `(paths, complete)`：`complete = false` 表示遍历中途遇到不可读目录
/// （权限错误等），此时返回已收集到的路径并标记不完整——调用方据此对受影响
/// provider 的源设置 `relation_complete = false`，从而不推导 tombstone（R2）。
///
/// 候选判定由 provider 自己的 probe 回答（见 [`is_discovery_candidate`]），不按
/// 扩展名硬编码：`.jsonl` 过滤会让 SQLite（`opencode.db`、`state.vscdb`）、整档
/// JSON（hermes/cline）与 Markdown（aider）这些真实 transcript 永远发现不到，
/// 那些 provider 于是在零配置路径上不可达（M2P-7）。
///
/// 不跟随符号链接（避免循环 / 越出数据根）。
fn discover_provider_sources(
    root: &std::path::Path,
    adapters: &[&dyn ProviderAdapter],
) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    let mut complete = true;
    let root_type = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata.file_type(),
        Err(_) => return (paths, false),
    };
    if root_type.is_symlink() || !root_type.is_dir() {
        return (paths, false);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        for entry_result in entries {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => {
                    complete = false;
                    continue;
                }
            };
            let path = entry.path();
            // 不跟随符号链接：file_type() 对 symlink 返回 symlink 类型而非目标。
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && is_discovery_candidate(&path, adapters) {
                paths.push(source_path_identity(&path.to_string_lossy()));
            }
        }
    }
    paths.sort();
    (paths, complete)
}

/// `sync --discover` 的 per-provider 发现结果。
struct ProviderDiscovery {
    id: String,
    found: usize,
    removed: usize,
    complete: bool,
    root_state: RootState,
}

/// 某 provider 数据根的状态。`found: 0 + complete: false` 过去把三种完全不同的
/// 情况塌成同一个形状，human 输出因此无法告诉用户该不该手动传文件（M2P-6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootState {
    /// 该 provider 未注册 discovery root：只能显式 `sync <file>`。
    Unsupported,
    /// 注册了 root 但当前用户 home 无法解析。
    HomeUnresolved,
    /// root 已注册但磁盘上不存在（provider 未安装或从未跑过）。
    Missing,
    /// root 存在且已扫描（是否完整看 `complete`）。
    Scanned,
}

impl RootState {
    fn as_str(self) -> &'static str {
        match self {
            RootState::Unsupported => "unsupported",
            RootState::HomeUnresolved => "home_unresolved",
            RootState::Missing => "missing",
            RootState::Scanned => "scanned",
        }
    }
}

/// 执行 `sync --discover`：遍历所有已知 provider 的数据根，收集 `.jsonl` 源，
/// diff 已存路径合成空批 tombstone（仅完整扫描时），并把发现的路径与合成批
/// 一起交给 [`sync_files`] 的核心流程。
///
/// 隐私：结果只报计数与 provider id，绝不包含绝对 transcript 路径。
fn sync_discover(
    store: &SqliteStore,
    progress: SyncProgress,
    request_id: Option<&str>,
) -> Result<(serde_json::Value, Vec<String>), CliError> {
    let mut all_paths: Vec<String> = Vec::new();
    let mut providers_out: Vec<ProviderDiscovery> = Vec::new();
    let mut discovered_provider_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut overall_complete = true;
    // 每个 provider 的 (provider_id, discovered_paths, complete)
    let mut per_provider: Vec<(String, Vec<String>, bool)> = Vec::new();
    // 候选判定要问所有 adapter 的 probe（源在哪个 root 下不代表它属于那个
    // provider——用户可能把 transcript 放错，或一个 root 下混着多种格式）。
    let registry = provider_registry();
    let adapter_refs: Vec<&dyn ProviderAdapter> = registry.iter().map(|a| a.as_ref()).collect();
    for adapter in &registry {
        let pid = adapter.provider_id().to_string();
        let Some(root) = provider_data_root(&pid) else {
            // 两种不同原因：该 provider 根本没注册 discovery root（只能手动
            // `sync <file>`），或注册了但 home 解析失败。两者都不能 tombstone。
            let root_state = if provider_root_subpath(&pid).is_none() {
                RootState::Unsupported
            } else {
                RootState::HomeUnresolved
            };
            overall_complete = false;
            per_provider.push((pid.clone(), Vec::new(), false));
            providers_out.push(ProviderDiscovery {
                id: pid,
                found: 0,
                removed: 0,
                complete: false,
                root_state,
            });
            continue;
        };
        if !root.is_dir() {
            // 根目录不存在时无法确认 provider 源是否只是暂时不可见；保守标记
            // partial，绝不因为缺少根目录而 tombstone prior paths。
            overall_complete = false;
            per_provider.push((pid.clone(), Vec::new(), false));
            providers_out.push(ProviderDiscovery {
                id: pid,
                found: 0,
                removed: 0,
                complete: false,
                root_state: RootState::Missing,
            });
            continue;
        }
        let (paths, complete) = discover_provider_sources(&root, &adapter_refs);
        overall_complete = overall_complete && complete;
        per_provider.push((pid.clone(), paths.clone(), complete));
        let found = paths.len();
        for path in &paths {
            discovered_provider_ids.insert(path.clone(), pid.clone());
        }
        all_paths.extend(paths.iter().cloned());
        providers_out.push(ProviderDiscovery {
            id: pid,
            found,
            removed: 0, // diff 后回填
            complete,
            root_state: RootState::Scanned,
        });
    }
    // dedup all_paths preserving order
    let mut unique: Vec<String> = Vec::with_capacity(all_paths.len());
    for p in &all_paths {
        if !unique.iter().any(|e| e == p) {
            unique.push(p.clone());
        }
    }
    let incomplete_providers: BTreeSet<String> = per_provider
        .iter()
        .filter(|(_, _, complete)| !complete)
        .map(|(pid, _, _)| pid.clone())
        .collect();
    let incomplete_paths: BTreeSet<String> = per_provider
        .iter()
        .filter(|(_, _, complete)| !complete)
        .flat_map(|(_, paths, _)| paths.iter().cloned())
        .collect();
    let relation_recovery_paths = store
        .source_paths_requiring_relation_scan(&unique)
        .map_err(ProtocolError::from)?;
    // Prior-path diff per provider：仅完整扫描时合成空批 tombstone。
    let mut synthetic_batches: Vec<SourceBatch> = Vec::new();
    for (i, (pid, paths, complete)) in per_provider.iter().enumerate() {
        if !complete {
            continue;
        }
        let prior = store
            .source_paths_for_provider(pid.as_str())
            .map_err(ProtocolError::from)?;
        let discovered_for_provider: BTreeSet<&str> = paths.iter().map(String::as_str).collect();
        let mut removed = 0usize;
        for prior_path in &prior {
            if !discovered_for_provider.contains(prior_path.as_str()) {
                // 源曾在该 provider 下被 sync，本次完整扫描未出现在磁盘上 → 合成空批。
                synthetic_batches.push(SourceBatch {
                    source_path: prior_path.clone(),
                    entries: Vec::new(),
                    placements: Vec::new(),
                    edges: Vec::new(),
                    activities: Vec::new(),
                    relation_complete: true,
                    len_bytes: None,
                    fingerprint: None,
                    provider_id: Some(pid.clone()),
                    resume_claim: None,
                });
                removed += 1;
            }
        }
        providers_out[i].removed = removed;
    }
    // 把发现的路径交给 sync_files 核心（绕过目录拒绝 guard）。
    // 若既无发现的源也无被删除的源（例如本机未安装任何 provider），返回一个
    // 不推进 generation 的空成功，而非 usage error——discover 空跑是合法状态。
    let (sync_data, warnings) = if unique.is_empty() && synthetic_batches.is_empty() {
        let generation = store.active_generation().map_err(ProtocolError::from)?;
        (
            serde_json::json!({
                "sources": 0,
                "emitted": 0,
                "messages": 0,
                "committed": 0,
                "unchanged": 0,
                "skipped": 0,
                "diagnostics": 0,
                "generation": generation,
            }),
            Vec::new(),
        )
    } else {
        sync_files_inner(
            store,
            &unique,
            &SyncContext {
                synthetic_batches,
                incomplete_providers,
                relation_recovery_paths,
                incomplete_paths,
                discovered_provider_ids,
                skip_unrecognized_sources: true,
            },
            true,
            progress,
            request_id,
        )?
    };
    // 组装 discovery 结果对象（绝不含绝对路径）。
    let providers_json: Vec<serde_json::Value> = providers_out
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "found": p.found,
                "removed": p.removed,
                "complete": p.complete,
                // 追加字段（M2P-6）：human 渲染器据此区分"未注册 root"、"root 不存在"、
                // "已扫描"，从而在不支持自动发现时告诉用户该手动传什么。
                "root_state": p.root_state.as_str(),
            })
        })
        .collect();
    let mut data = sync_data;
    if let Some(obj) = data.as_object_mut() {
        obj.insert(
            "discovery".into(),
            serde_json::json!({
                "complete": overall_complete,
                "providers": providers_json,
            }),
        );
    }
    Ok((data, warnings))
}

fn staged_to_source(
    path: &str,
    staged: &StagedBatch,
    provider_id: &str,
    variant: &str,
    fingerprint: &str,
    source_len: u64,
) -> Result<SourceBatch, CliError> {
    staged_to_source_with_provider(
        path,
        staged,
        provider_id,
        variant,
        fingerprint,
        source_len,
        None,
    )
}

fn staged_to_source_with_provider(
    path: &str,
    staged: &StagedBatch,
    provider_id: &str,
    variant: &str,
    fingerprint: &str,
    source_len: u64,
    discovered_provider_id: Option<&str>,
) -> Result<SourceBatch, CliError> {
    if staged.report.committed != staged.messages.len() {
        return Err(DomainError::InvariantViolation(format!(
            "provider reported {} committed messages but emitted {}",
            staged.report.committed,
            staged.messages.len()
        ))
        .into());
    }

    // 文档实体：内容寻址——同字节重 ingest 得到同一 id（幂等）。
    let document_id = StableId::derive(
        IdKind::Document,
        Stability::Reconstructed,
        &[
            provider_id.as_bytes(),
            variant.as_bytes(),
            fingerprint.as_bytes(),
        ],
    );
    // 会话实体：native id 优先；缺失回退 document 派生（一文档一会话，见 design §9）。
    // Native 路径必须走 namespaced 构造函数（identity 前置）：canonical 身份 =
    // digest(provider, installation, native id)，同 native id 跨 provider/安装
    // 绝不碰撞；wire 仍为 `ses_v1_` + digest，native id 单独经 Resume claim 持久化。
    let install_ns = installation_namespace(path, provider_id);
    let session_id = match staged.report.session_native_id.as_deref() {
        Some(sid) if !sid.trim().is_empty() => StableId::native_session_scoped(
            &SessionIdentityNamespace {
                provider_id,
                installation_namespace: &install_ns,
            },
            sid,
        ),
        _ => StableId::derive(
            IdKind::Session,
            Stability::Reconstructed,
            &[document_id.as_str().as_bytes()],
        ),
    };

    let mut entities = BTreeMap::<String, StagedMessageEntity>::new();
    let mut member_ids = Vec::new();
    let mut seen_members = BTreeSet::new();
    let mut placements = Vec::with_capacity(staged.messages.len());
    let mut edges = Vec::new();
    for message in &staged.messages {
        if let Some((start, end)) = message.span
            && (end < start || end > source_len)
        {
            return Err(DomainError::InvariantViolation(
                "provider emitted a span outside the verified source document".into(),
            )
            .into());
        }
        let id = if message.native_id.trim().is_empty() {
            StableId::derive(
                IdKind::Message,
                Stability::Unstable,
                &[
                    provider_id.as_bytes(),
                    variant.as_bytes(),
                    document_id.as_str().as_bytes(),
                    &message.seq.to_le_bytes(),
                ],
            )
        } else {
            StableId::native(IdKind::Message, &message.native_id)
        };
        let span = message.span.map(|(start, end)| EvidenceSpan { start, end });
        let placement = MessagePlacement::new(
            session_id.clone(),
            document_id.clone(),
            id.clone(),
            message.seq,
            message.is_sidechain,
            span,
        );
        let parent_id = message
            .parent_native_id
            .as_deref()
            .filter(|parent| !parent.trim().is_empty())
            .map(|parent| StableId::native(IdKind::Message, parent));
        if let Some(parent_message_id) = &parent_id {
            edges.push(MessageEdge {
                child_placement_id: placement.id.clone(),
                parent_message_id: parent_message_id.clone(),
                parent_native_id: message.parent_native_id.clone(),
                relation: MessageRelation::Reply,
            });
        }
        if seen_members.insert(id.as_str().to_string()) {
            member_ids.push(id.as_str().to_string());
        }
        match entities.entry(id.as_str().to_string()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(StagedMessageEntity {
                    id: id.clone(),
                    role: message.role.clone(),
                    text: message.text.clone(),
                    timestamp: message.timestamp.clone(),
                    occurrences: vec![(
                        placement.clone(),
                        parent_id,
                        message.parent_native_id.clone(),
                    )],
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let entity = entry.get_mut();
                if entity.id != id
                    || entity.role != message.role
                    || entity.text != message.text
                    || entity.timestamp != message.timestamp
                {
                    return Err(DomainError::InvariantViolation(
                        "message has conflicting stable projections within one source".into(),
                    )
                    .into());
                }
                entity.occurrences.push((
                    placement.clone(),
                    parent_id,
                    message.parent_native_id.clone(),
                ));
            }
        }
        placements.push(placement);
    }

    let mut entries = Vec::with_capacity(entities.len() + 2);
    for entity in entities.into_values() {
        let mut occurrences = entity.occurrences;
        occurrences.sort_by(|left, right| {
            (left.0.source_ordinal, left.0.id.as_str())
                .cmp(&(right.0.source_ordinal, right.0.id.as_str()))
        });
        let parent_facts: Vec<Option<String>> = occurrences
            .iter()
            .map(|(_, parent_id, _)| parent_id.as_ref().map(|parent| parent.as_str().to_string()))
            .collect();
        let parent = match parent_facts.first() {
            Some(first) if parent_facts.iter().all(|fact| fact == first) => first.clone(),
            _ => None,
        };
        let parent_native_facts: Vec<Option<String>> = occurrences
            .iter()
            .map(|(_, _, parent_native_id)| parent_native_id.clone())
            .collect();
        let parent_native_id = match parent_native_facts.first() {
            Some(first) if parent_native_facts.iter().all(|fact| fact == first) => first.clone(),
            _ => None,
        };
        let is_sidechain = match occurrences.first() {
            Some((first, _, _))
                if occurrences
                    .iter()
                    .all(|(placement, _, _)| placement.is_sidechain == first.is_sidechain) =>
            {
                serde_json::Value::Bool(first.is_sidechain)
            }
            _ => serde_json::Value::Null,
        };
        let spans: Vec<serde_json::Value> = occurrences
            .iter()
            .filter_map(|(placement, _, _)| {
                placement.span.as_ref().map(|span| {
                    serde_json::json!({
                        "placement_id": placement.id.as_str(),
                        "document": placement.source_document_id.as_str(),
                        "start": span.start,
                        "end": span.end,
                    })
                })
            })
            .collect();
        let payload = serde_json::json!({
            "role": entity.role,
            "text": entity.text,
            "timestamp": entity.timestamp,
            "parent": parent,
            "parent_native_id": parent_native_id,
            "is_sidechain": is_sidechain,
            "session": session_id.as_str(),
            "sessions": [session_id.as_str()],
            "span": spans.first().map(|span| serde_json::json!({
                "start": span["start"],
                "end": span["end"],
            })),
            "spans": spans,
        })
        .to_string()
        .into_bytes();
        entries.push((entity.id, payload, entity.text));
    }

    let session_payload = serde_json::json!({
        "documents": [document_id.as_str()],
        "document": document_id.as_str(),
        "messages": member_ids,
    })
    .to_string();
    let document_payload = serde_json::json!({
        "provider": provider_id,
        "variant": variant,
        "fingerprint": fingerprint,
        "len": source_len,
    })
    .to_string();
    // Source-scoped Resume Metadata 声明（ADR-0009）：把 provider 观察归一为
    // claim，随本批 source 事务原子写入；缺失/歧义已折叠进 state。
    let resume_claim = if provider_id == "empty" {
        None
    } else {
        Some(SourceResumeClaim::from_observation(
            provider_id,
            session_id.as_str(),
            &staged.report.session_observation,
        ))
    };

    entries.push((session_id, session_payload.into_bytes(), String::new()));
    let document_wire = document_id.as_str().to_string();
    entries.push((document_id, document_payload.into_bytes(), String::new()));

    // 工具活动锚点解析（设计 R5.3）：把 provider-native 锚点 id 解析为本批的
    // 稳定消息 id。规则与消息实体去重一致（seq 顺序首现匹配、native 优先、
    // 缺省回退派生）；锚点消息未被 emit（skipped/非对话）→ 活动丢弃，绝不臆造。
    let mut activities = Vec::new();
    for staged_activity in &staged.activities {
        let Some(anchor) = staged
            .messages
            .iter()
            .find(|message| message.native_id == staged_activity.message_native_id)
        else {
            continue;
        };
        let id = if anchor.native_id.trim().is_empty() {
            StableId::derive(
                IdKind::Message,
                Stability::Unstable,
                &[
                    provider_id.as_bytes(),
                    variant.as_bytes(),
                    document_wire.as_bytes(),
                    &anchor.seq.to_le_bytes(),
                ],
            )
        } else {
            StableId::native(IdKind::Message, &anchor.native_id)
        };
        activities.push(SourceActivity {
            message_id: id,
            activity: staged_activity.activity.clone(),
        });
    }

    Ok(SourceBatch {
        source_path: path.to_string(),
        entries,
        placements,
        edges,
        activities,
        relation_complete: staged.report.skipped == 0,
        len_bytes: Some(source_len as i64),
        fingerprint: Some(fingerprint.to_string()),
        provider_id: discovered_provider_id.map(str::to_string),
        resume_claim,
    })
}

/// 读取原始 .jsonl 文件并 ingest：
/// capture 快照 → stage（probe+缓冲 parse）→ verify 快照 → 单事务原子提交。
///
/// 落实：
/// - RFC-0002 §4 ReadOnlySourceSnapshot：len+mtime+fingerprint 复核；
/// - RFC-0002 §5 source-level staging：parse 只缓冲，成功后才 commit_batch。
///
/// 会话 fact 用文件路径，保证同文件重 ingest 得到稳定 id（幂等重索引）。
fn ingest_file(
    store: &SqliteStore,
    path: &str,
) -> Result<(serde_json::Value, Vec<String>), CliError> {
    let path_ref = std::path::Path::new(path);
    // 1) 捕获只读源快照（只计算 len/mtime/fingerprint，不保留源字节）。
    let snap = capture(path_ref).map_err(ProtocolError::from)?;
    let source = open_snapshot_source(path_ref, &snap).map_err(ProtocolError::from)?;

    // 2) probe-select + stage：每个 probe/parse 都从只读 source 重新打开 bounded reader。
    let (staged, variant) = stage_with_source(&source)?;

    // 3) 提交前复核：源在 stage 期间被改写则拒绝提交（RFC-0002 §4）。
    verify_snapshot(path_ref, &snap).map_err(ProtocolError::from)?;

    // 4) 派生 id + 构造该源的完整 scan 结果（消息 + 会话/文档目录行），按
    //    source membership 提交。同文件重 ingest 时，本次消失的 id 会被推导为 tombstone。
    let provider = variant.split('/').next().unwrap_or(&variant).to_string();
    let source = staged_to_source(
        path,
        &staged,
        &provider,
        &variant,
        &snap.fingerprint,
        snap.len,
    )?;
    let changed = store
        .commit_source_batches_if_changed(std::slice::from_ref(&source))
        .map_err(ProtocolError::from)?;

    let generation = store.active_generation().map_err(ProtocolError::from)?;
    let warnings = diagnostic_warnings(
        staged.report.diagnostics.iter().map(String::as_str),
        staged.report.diagnostics.len(),
    );
    Ok((
        serde_json::json!({
            "variant": variant,
            "emitted": staged.messages.len(),
            "committed": if changed { staged.messages.len() } else { 0 },
            "unchanged": if changed { 0 } else { staged.messages.len() },
            "skipped": staged.report.skipped,
            "diagnostics": staged.report.diagnostics.len(),
            "generation": generation,
            "source_fp": snap.fingerprint,
        }),
        warnings,
    ))
}

/// JSONL 源健康度三态分类。改编自 fast-resume 的 `jsonl_health`
/// （`src/adapters/shared.rs`，MIT License，Copyright (c) 2025 Stanislas Lange）：
///
/// - `Clean`：全部非空行都是合法 JSON；
/// - `Partial`：存在坏行但之后仍有合法行——recoverable，解析时逐行跳过
///   （provider 既有的 recoverable-skip 语义，此处只做源级分类）；
/// - `Invalid`：坏行之后没有合法行——典型是尾部截断（EOF 落在记录中间，
///   agent 正在写文件）。对已索引源采取 Retain：保留旧索引、不重 parse。
///
/// 行切分按字节（`\n`，容忍 `\r\n`），首行剥离 UTF-8 BOM，与 provider
/// 解析语义一致。经 [`for_each_bounded_source_line`] 流式遍历，内存上界为
/// 单条记录（RFC-0002 §7），绝不把整源读入内存。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonlHealth {
    Clean,
    Partial,
    Invalid,
}

fn jsonl_health(
    path: &std::path::Path,
    snapshot: &agent_session_grep_ports::SourceSnapshot,
) -> Result<JsonlHealth, agent_session_grep_ports::ProviderError> {
    let source = open_snapshot_source(path, snapshot)
        .map_err(|error| agent_session_grep_ports::ProviderError::Io(error.to_string()))?;
    let mut valid_rows = 0usize;
    let mut malformed_rows = 0usize;
    let mut valid_after_last_malformed = false;
    agent_session_grep_ports::for_each_bounded_source_line(
        &source,
        agent_session_grep_ports::STREAM_RECORD_MAX_BYTES,
        |line| {
            if line.bytes.iter().all(u8::is_ascii_whitespace) {
                return Ok(());
            }
            if serde_json::from_slice::<serde_json::Value>(line.bytes).is_err() {
                malformed_rows += 1;
                valid_after_last_malformed = false;
            } else {
                valid_rows += 1;
                if malformed_rows > 0 {
                    valid_after_last_malformed = true;
                }
            }
            Ok(())
        },
    )?;
    Ok(match (valid_rows, malformed_rows) {
        (_, 0) => JsonlHealth::Clean,
        (0, _) => JsonlHealth::Invalid,
        _ if valid_after_last_malformed => JsonlHealth::Partial,
        _ => JsonlHealth::Invalid,
    })
}

/// 同步显式给定的源文件：所有文件先完成 capture + stage + verify，之后才提交
/// 一个 durable batch。这样任一文件失败都不会留下其它文件的部分更新。
/// `progress` 为 true（仅 jsonl 模式）时逐源发 progress frame——staging 是
/// 长任务里唯一逐文件推进的阶段，提交本身是单事务不可分。
///
/// 扫描期健康分诊（fast-resume `jsonl_health` 模式）：已索引过的源若检出截断尾
/// （`JsonlHealth::Invalid`，agent 正在写），Retain——不重 parse、不推进指纹、
/// 不提交，保留旧索引并报诊断，避免 rebuild churn 与误 tombstone；写完后再 sync
/// 会因指纹不匹配走完整重扫。新源（无缓存指纹）没有旧索引可保留，照常走
/// recoverable-skip：有效前缀提交、截断行计入 skipped + 诊断（relation_complete
/// = false，store 层不推导 tombstone）。
fn sync_files(
    store: &SqliteStore,
    paths: &[String],
    provider: Option<&str>,
    progress: SyncProgress,
    request_id: Option<&str>,
) -> Result<(serde_json::Value, Vec<String>), CliError> {
    if paths.is_empty() {
        return Err(CliError::usage("sync <file>... requires at least one file"));
    }
    // 新手第一本能是给 sync 传整个目录；目录不是 .jsonl 文件，捕获要读它时会
    // 报"拒绝访问 (os error 5)"，误导新手去折腾权限/杀毒（10 角色体验测试缺陷）。
    // 这里显式拦截并给出正确用法。消息不带路径（隐私：用户目录布局不外泄），
    // 展开示例保持平台中立（不给 PowerShell-only 的 Get-ChildItem 例子，R2.2）。
    // discover 路径不走此 guard——它已经枚举了文件而非目录。
    for path in paths {
        if std::path::Path::new(path).is_dir() {
            return Err(CliError::usage(
                "sync 接受一个或多个 .jsonl 文件，不接受目录；\
                 需要同步整个目录时，请用你的 shell 展开文件列表，把文件逐个传给 sync",
            ));
        }
    }
    // 重复路径去重（保持出现顺序）：同一文件列两次是书写冗余而非两个源；
    // 不去重会让 store 层把同一路径当两个 source batch 提交而判 catalog_error
    // （exit 6）——sync 幂等语义下应提前归一为单个源。
    let mut unique: Vec<String> = Vec::with_capacity(paths.len());
    for path in paths {
        let path = source_path_identity(path);
        if !unique.iter().any(|existing| existing == &path) {
            unique.push(path);
        }
    }
    // `--provider` 是显式消歧输入：内容同形的 provider（pi/openclaw）在没有
    // discover root 上下文时无法归属，此时用户点名即唯一证据。走与 discover
    // 相同的通道（`discovered_provider_ids`），所以只在同分时生效——点错名字
    // 不会把一个格式硬塞给另一个 provider。
    let ctx = match provider {
        Some(id) => SyncContext {
            discovered_provider_ids: unique
                .iter()
                .map(|path| (path.clone(), id.to_string()))
                .collect(),
            ..SyncContext::default()
        },
        None => SyncContext::default(),
    };
    sync_files_inner(store, &unique, &ctx, false, progress, request_id)
}

/// `sync_files_inner` 的 discover 扩展上下文：plain sync 全部为空，`sync --discover`
/// 注入合成空批、完整性降级集合与 path→provider 归属表。
#[derive(Default)]
struct SyncContext {
    /// discover 为已删除源合成的空批（`relation_complete = true`），不经过
    /// capture（文件已不在磁盘上），直接追加进提交批次以触发 tombstone。
    synthetic_batches: Vec<SourceBatch>,
    /// 部分 root scan 的 provider：其批次 relation_complete 降级为 false。
    incomplete_providers: BTreeSet<String>,
    /// 强制重扫之前关系不完整的源（无视指纹缓存）。
    relation_recovery_paths: BTreeSet<String>,
    /// 之前扫描 skipped>0 的源路径（同样绕过指纹跳过）。
    incomplete_paths: BTreeSet<String>,
    /// discover 归属表：源路径 → provider id；落入 `source_scans.provider_id`。
    discovered_provider_ids: BTreeMap<String, String>,
    /// 无 provider 认领（或 variant 歧义）的源是否跳过而非中止整轮。
    ///
    /// discover 自己枚举 provider 数据根，用户没有点名任何一个文件；根下混进一个
    /// 非 transcript 文件（编辑器备份、半截日志、别的工具的产物）过去会让整轮
    /// `sync --discover` 以 exit 2 结束、0 条消息入库——一个坏文件掀翻整轮
    /// （M2P-5）。行级破损早已是"跳过、计数、末尾报告、绝不中止"（D11），文件级
    /// 现在与之对齐。显式 `sync <file>` 保持报错：路径是用户亲手给的。
    skip_unrecognized_sources: bool,
}

/// 校验 `sync --provider <id>`：必须是注册表里真实存在的 provider。
///
/// 权威是 [`provider_registry`] 而非能力矩阵：矩阵含两个 deferred provider
/// （没有 adapter），点名它们只会得到"没人认领"这种误导性错误。这里只接受
/// 真正能 probe 的那些 id，并把全部可选值列进错误消息（R2.2 可行动性）。
fn validated_sync_provider(id: &str) -> Result<String, CliError> {
    let registry = provider_registry();
    if registry.iter().any(|a| a.provider_id() == id) {
        return Ok(id.to_string());
    }
    let mut ids: Vec<&str> = registry.iter().map(|a| a.provider_id()).collect();
    ids.sort_unstable();
    Err(CliError::usage(format!(
        "sync --provider: unknown provider {id}; expected one of {}",
        ids.join("|")
    )))
}

/// `sync_files` / `sync_discover` 共享的核心流程。
///
/// `paths` 应已去重且不含目录（调用方负责）。
/// One source's fully prepared, store-independent ingest product.
///
/// Everything in here is derived from the read-only source plus values read from
/// the store *before* any thread starts (`cached` fingerprints, the discover
/// context). Nothing in here touches the connection, which is what makes the
/// preparation safe to run off the writer thread — see [`prepare_sources`].
struct PreparedSource {
    /// The captured snapshot, re-verified before commit on the writer thread.
    snapshot: agent_session_grep_ports::SourceSnapshot,
    /// `Some` when this source was parsed this run; `None` for the three
    /// no-parse outcomes (fingerprint hit, truncated-tail retain, skipped).
    parsed: Option<ParsedSource>,
    outcome: PrepareOutcome,
}

/// A parsed source's committable batch plus the counters the report needs.
struct ParsedSource {
    batch: SourceBatch,
    message_count: usize,
    skipped: usize,
    diagnostics: Vec<String>,
}

/// Which of the four per-source paths a prepared source took. Drives the
/// progress wording and the report counters, both of which stay on the writer
/// thread so their order is the caller's path order, never completion order.
enum PrepareOutcome {
    /// Parsed and staged; carries no extra state (the batch is in `parsed`).
    Scanned,
    /// Fingerprint cache hit: parse skipped, this many messages already indexed.
    Unchanged(usize),
    /// Truncated tail on an already-indexed source: keep the previous index.
    Retained,
    /// Not attributable to any provider (unclaimed or ambiguous); skipped.
    Unrecognized(SourceRejection),
}

/// Opt-in per-phase timing for a sync run, written to stderr.
///
/// Set `ASG_SYNC_TRACE=1` to see where a sync's wall time actually goes.
/// Modeled on fast-resume's `FAST_RESUME_TRACE_REFRESH` (`src/refresh.rs`,
/// MIT License, Copyright (c) 2025 Stanislas Lange): an env probe gates
/// `eprintln!` timings around each phase, so an unset variable costs one
/// `var_os` per sync — not per source — and stdout stays a clean protocol
/// stream (diagnostics belong on stderr, never in the envelope).
///
/// This exists because the first attempt at this function's performance
/// problem guessed wrong: parallelizing the parse phase moved full-corpus
/// throughput by under 10%, which only became obvious once each phase was
/// timed separately. Keep the trace so the next person measures instead of
/// guessing.
#[derive(Clone, Copy)]
struct SyncTrace(bool);

impl SyncTrace {
    fn from_env() -> Self {
        SyncTrace(std::env::var_os("ASG_SYNC_TRACE").is_some())
    }

    /// Report `label`'s duration since `mark` and return a fresh mark for the
    /// next phase. Returns the new instant even when tracing is off so callers
    /// chain phases identically in both modes.
    fn phase(self, label: &str, mark: std::time::Instant) -> std::time::Instant {
        let now = std::time::Instant::now();
        if self.0 {
            eprintln!(
                "asg-sync-trace: {label} {:.1}ms",
                now.duration_since(mark).as_secs_f64() * 1000.0
            );
        }
        now
    }
}

/// Run `work` over `0..len` across the machine's cores, funneling each result
/// back over a channel, and return the results in **index order**.
///
/// Borrowed from `agf`'s `start_stale_scan` (`src/cache.rs`, MIT License,
/// Copyright (c) 2025 subinium): "one worker thread per stale agent, each worker
/// sends its result back over a channel", plus fast-resume's `refresh.rs`
/// (MIT License, Copyright (c) 2025 Stanislas Lange) for dropping the original
/// sender so the drain terminates when the last worker exits. Two deliberate
/// departures, because the unit of work here is a source file rather than a
/// whole provider:
///
/// - **Bounded pool, not one thread per unit.** A 5,000-file corpus cannot get
///   5,000 threads. Workers claim indices from a shared atomic cursor, so the
///   thread count comes from `available_parallelism()` and never scales with
///   the corpus.
/// - **Order restored before use.** Both upstreams ingest results in completion
///   order, which is fine for a session list that gets sorted anyway. Here
///   completion order must not be observable: message identity for providers
///   that emit an empty `native_id` is derived per document, and sync
///   diagnostics are numbered "source N of M". So each result carries its index
///   and the drain writes it into a slot.
///
/// `on_ready` is invoked on the draining thread in strictly increasing index
/// order — the contiguous ready prefix is flushed after each receive, so
/// progress output stays both incremental and deterministic.
///
/// The lowest-index error wins, matching a serial loop's early return.
///
/// No `store` is in scope by construction, which is how the single-writer rule
/// (`adapters-sqlite/src/lease.rs`) survives the fan-out: parsing goes wide,
/// SQLite stays single-threaded in the caller.
fn parallel_map_indexed<T, W, R>(len: usize, work: W, mut on_ready: R) -> Result<Vec<T>, CliError>
where
    T: Send,
    W: Fn(usize) -> Result<T, CliError> + Send + Sync,
    R: FnMut(usize, &T),
{
    // A single unit needs no fan-out; skip the pool so `sync <one file>` keeps
    // its current cost profile exactly.
    if len <= 1 {
        let mut out = Vec::with_capacity(len);
        for index in 0..len {
            let value = work(index)?;
            on_ready(index, &value);
            out.push(value);
        }
        return Ok(out);
    }

    let workers = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .min(len);
    let cursor = std::sync::atomic::AtomicUsize::new(0);
    let mut slots: Vec<Option<Result<T, CliError>>> = (0..len).map(|_| None).collect();

    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<(usize, Result<T, CliError>)>();
        for _ in 0..workers {
            let tx = tx.clone();
            let cursor = &cursor;
            let work = &work;
            scope.spawn(move || {
                loop {
                    let index = cursor.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if index >= len {
                        return;
                    }
                    // The receiver lives until the scope joins every worker, so
                    // a send error is unreachable; stop claiming work rather
                    // than dropping a result, which would silently lose a unit.
                    if tx.send((index, work(index))).is_err() {
                        return;
                    }
                }
            });
        }
        // Drop the original sender so the drain ends once every worker is done.
        drop(tx);

        let mut next = 0usize;
        for (index, value) in rx {
            slots[index] = Some(value);
            while next < len {
                match slots[next].as_ref() {
                    Some(Ok(value)) => on_ready(next, value),
                    // A failed unit gets no callback; the error surfaces from
                    // the ordered collect below.
                    Some(Err(_)) => {}
                    None => break,
                }
                next += 1;
            }
        }
    });

    let mut out = Vec::with_capacity(len);
    for slot in slots {
        out.push(slot.expect("every index is filled before the channel closes")?);
    }
    Ok(out)
}

/// Prepare a single source: capture, health-triage, parse, derive the
/// committable batch. Pure with respect to the store; runs on a worker thread.
///
/// Everything it reads from the store was read before any thread started
/// (`cached` fingerprints, `unchanged_counts`, the discover context), so no
/// connection is touched here.
fn prepare_one(
    path: &str,
    registry: &[&dyn ProviderAdapter],
    ctx: &SyncContext,
    cached: &BTreeMap<String, agent_session_grep_adapters_sqlite::SourceFingerprint>,
    unchanged_counts: &BTreeMap<String, usize>,
) -> Result<PreparedSource, CliError> {
    let path_ref = std::path::Path::new(path);
    let snapshot = capture(path_ref).map_err(ProtocolError::from)?;
    let source = open_snapshot_source(path_ref, &snapshot).map_err(ProtocolError::from)?;
    let cached_fp = cached.get(path).and_then(|(_, fp)| fp.clone());

    // 空文件（0 字节）不能走指纹跳过：它必须作为"整源清空"批次提交
    // 以 tombstone 旧消息；跳过会退化成空批 no-op，丢失 tombstone 语义。
    if !source.is_empty()
        && cached_fp.as_deref() == Some(snapshot.fingerprint.as_str())
        && !ctx.relation_recovery_paths.contains(path)
        && !ctx.incomplete_paths.contains(path)
    {
        // 字节未变：跳过 parse。store 层仍会做 no-op 判定（entries 为空时
        // 会走 membership/scan 对比），因此这里只需空 staged 占位。
        let already = unchanged_counts.get(path).copied().unwrap_or(0);
        return Ok(PreparedSource {
            snapshot,
            parsed: None,
            outcome: PrepareOutcome::Unchanged(already),
        });
    }

    // 截断尾（EOF 落在记录中间）＝agent 正在写这个源。已索引过的源
    // 必须 Retain：不重 parse、不推进指纹、不提交——旧索引原样保留，
    // 不产生 rebuild churn，也不误 tombstone（fast-resume
    // Invalid→Retain 语义）。只报诊断；文件写完后再 sync 会因指纹
    // 不匹配走完整重扫。新源（cached_fp 为 None）无旧索引可保留，
    // 落到下面走既有 recoverable-skip。
    if !source.is_empty()
        && cached_fp.is_some()
        && jsonl_health(path_ref, &snapshot).map_err(ProtocolError::from)? == JsonlHealth::Invalid
    {
        return Ok(PreparedSource {
            snapshot,
            parsed: None,
            outcome: PrepareOutcome::Retained,
        });
    }

    // discover 记下的 root 归属。同时是消歧输入（pi/openclaw 这类内容同形的
    // provider 只有 root 能区分）与归属成功后的标签。
    let discovered_provider = ctx.discovered_provider_ids.get(path).map(String::as_str);
    let (staged, variant) = match stage_with_registry(registry, &source, discovered_provider) {
        Ok(staged) => staged,
        // 文件级宽容（M2P-5）：源不被任何 provider 认领，或多个 provider
        // 同分而歧义（repo 原则：歧义拒绝，绝不猜）。两者都是关于这一个
        // 文件的判定，不是整轮的失败——跳过、计数，末尾经既有
        // diagnostics/warnings 通道如实报告，与行级 skip 同一条通道。
        Err(error) if ctx.skip_unrecognized_sources && source_rejection(&error).is_some() => {
            let reason = source_rejection(&error).expect("checked by guard");
            return Ok(PreparedSource {
                snapshot,
                parsed: None,
                outcome: PrepareOutcome::Unrecognized(reason),
            });
        }
        Err(error) => return Err(error.into()),
    };

    let provider = variant.split('/').next().unwrap_or(&variant).to_string();
    let mut batch = staged_to_source_with_provider(
        path,
        &staged,
        &provider,
        &variant,
        &snapshot.fingerprint,
        snapshot.len,
        discovered_provider,
    )?;
    if ctx.incomplete_providers.contains(&provider) {
        batch.relation_complete = false;
    }

    Ok(PreparedSource {
        parsed: Some(ParsedSource {
            batch,
            message_count: staged.messages.len(),
            skipped: staged.report.skipped,
            diagnostics: staged.report.diagnostics.clone(),
        }),
        snapshot,
        outcome: PrepareOutcome::Scanned,
    })
}

/// 同步进度的呈现方式。
///
/// 三态而非 bool:jsonl 走版本化协议 frame(stdout),human 走 stderr 单行,
/// `--robot`/json 完全静默。契约 §6 明确"进程诊断永远走 stderr",而 §4 只禁
/// `--robot` 的 progress —— 所以 human 模式在 stderr 上报进度是合规的,且
/// **stdout 字节不变**(既有断言 stdout 版式的测试不会受影响)。
///
/// 为什么需要它:首次全量索引在真实语料上要跑几分钟,而 human 模式此前
/// 全程零输出 —— 用户无法区分"正在工作"与"卡死了"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncProgress {
    /// 不报进度(`--robot` 与 json 单 envelope 模式)。
    Silent,
    /// stderr 单行文本,给人看。
    HumanStderr,
    /// stdout 版本化 progress frame,给程序看。
    JsonlFrames,
}

impl SyncProgress {
    /// `--robot` 在上游已被归一为 [`protocol::OutputMode::Json`](protocol::OutputMode),
    /// 所以这里只看 mode 就够 —— Json 一律静默,同时覆盖 `--robot` 与
    /// `--output json` 两条路径(两者都只允许单个 envelope,插入进度会破坏它)。
    fn for_mode(mode: protocol::OutputMode) -> Self {
        match mode {
            protocol::OutputMode::Jsonl => Self::JsonlFrames,
            protocol::OutputMode::Human => Self::HumanStderr,
            protocol::OutputMode::Json => Self::Silent,
        }
    }

    /// 报一条进度。`message` 已由调用方按四种路径如实措辞。
    fn emit(self, message: &str, request_id: Option<&str>) {
        match self {
            Self::Silent => {}
            Self::HumanStderr => eprintln!("asg sync: {message}"),
            Self::JsonlFrames => {
                protocol::write_stdout_line(&protocol::progress_frame("sync", message, request_id));
            }
        }
    }
}

fn sync_files_inner(
    store: &SqliteStore,
    paths: &[String],
    ctx: &SyncContext,
    allow_empty: bool,
    progress: SyncProgress,
    request_id: Option<&str>,
) -> Result<(serde_json::Value, Vec<String>), CliError> {
    let synthetic_batches = &ctx.synthetic_batches;
    if paths.is_empty() && synthetic_batches.is_empty() && !allow_empty {
        return Err(CliError::usage("sync <file>... requires at least one file"));
    }
    let mut sources = Vec::with_capacity(paths.len() + synthetic_batches.len());
    let mut message_count = 0usize;
    let mut skipped_count = 0usize;
    let mut retained_count = 0usize;
    let mut unrecognized_count = 0usize;
    let mut diagnostic_count = 0usize;
    let mut diagnostics = Vec::new();

    // 指纹缓存：capture 后先与已存指纹比对，未变化的源跳过重复解析。
    // 指纹缓存缺失/不匹配才走完整路径。
    // 两次 store 读取都在 fan-out 之前完成，之后的准备阶段不再触碰连接。
    let trace = SyncTrace::from_env();
    let mark = std::time::Instant::now();
    let cached = store
        .source_fingerprints(paths)
        .map_err(ProtocolError::from)?;
    let mut unchanged_messages = 0usize;
    let mut provider_id_backfills = Vec::new();
    let unchanged_counts = store
        .source_message_counts(paths)
        .map_err(ProtocolError::from)?;
    let mark = trace.phase("store_read", mark);

    // 准备阶段跨核并行；提交阶段单线程（SQLite 单写者，见 lease.rs）。
    // 每个源的 capture(BLAKE3) + probe/parse + id 派生互不相干；
    // 顺序由 parallel_map_indexed 还原为 path 顺序。
    let registry = provider_registry();
    let refs: Vec<&dyn ProviderAdapter> = registry.iter().map(|a| a.as_ref()).collect();
    let prepared = parallel_map_indexed(
        paths.len(),
        |index| prepare_one(&paths[index], &refs, ctx, &cached, &unchanged_counts),
        |index, prepared: &PreparedSource| {
            if progress == SyncProgress::Silent {
                return;
            }
            // 措辞如实区分四种路径：指纹命中只是 checked（未 parse），
            // 走完整解析的才是 scanned，截断尾 retain 是 kept，无法归属的是
            // skipped——不得谎报缓存命中的源为 "staged (0 messages)"。
            let total = paths.len();
            let message = match &prepared.outcome {
                PrepareOutcome::Scanned => format!(
                    "scanned source {}/{} ({} messages)",
                    index + 1,
                    total,
                    prepared
                        .parsed
                        .as_ref()
                        .map_or(0, |parsed| parsed.message_count)
                ),
                PrepareOutcome::Retained => format!(
                    "retained source {}/{} (truncated tail — keeping previous index)",
                    index + 1,
                    total
                ),
                PrepareOutcome::Unrecognized(_) => format!(
                    "skipped source {}/{} (not a recognized agent transcript)",
                    index + 1,
                    total
                ),
                PrepareOutcome::Unchanged(_) => {
                    format!("checked source {}/{} (unchanged)", index + 1, total)
                }
            };
            progress.emit(&message, request_id);
        },
    )?;
    let mark = trace.phase("prepare", mark);

    // 提交前复核：所有源都完成 staging 之后才 verify，保持 RFC-0002 §4 的
    // "staged 全部就绪 → 复核 → 单事务提交" 顺序。复核本身是每源独立的
    // BLAKE3 全量重读，同样跨核并行。
    parallel_map_indexed(
        paths.len(),
        |index| {
            verify_snapshot(
                std::path::Path::new(&paths[index]),
                &prepared[index].snapshot,
            )
            .map_err(ProtocolError::from)?;
            Ok(())
        },
        |_, _: &()| {},
    )?;
    let mark = trace.phase("verify", mark);

    // 单写者路径：按 path 顺序归并计数、诊断与批次。顺序由
    // parallel_map_indexed 保证，与线程完成顺序无关。
    for (index, prepare) in prepared.into_iter().enumerate() {
        match prepare.outcome {
            PrepareOutcome::Unchanged(already) => unchanged_messages += already,
            PrepareOutcome::Retained => {
                retained_count += 1;
                diagnostics.push(format!(
                    "source {} of {}: truncated tail (a JSON record is cut off at EOF, \
                     the file may still be written); keeping previously indexed content \
                     — re-run sync when the file is complete",
                    index + 1,
                    paths.len()
                ));
                diagnostic_count += 1;
            }
            PrepareOutcome::Unrecognized(reason) => {
                unrecognized_count += 1;
                diagnostics.push(match reason {
                    SourceRejection::Unclaimed => format!(
                        "source {} of {}: not a recognized agent transcript, skipped \
                         (nothing was indexed from it)",
                        index + 1,
                        paths.len()
                    ),
                    // 歧义源确实被认领了，只是无法唯一归属——措辞不能说
                    // "不是 transcript"，那会把用户引向错误的方向。
                    SourceRejection::Ambiguous => format!(
                        "source {} of {}: matched more than one provider format equally \
                         well, skipped rather than guessed; sync this file explicitly to \
                         see which formats collided",
                        index + 1,
                        paths.len()
                    ),
                });
                diagnostic_count += 1;
                continue;
            }
            PrepareOutcome::Scanned => {}
        }
        match prepare.parsed {
            Some(parsed) => {
                message_count += parsed.message_count;
                skipped_count += parsed.skipped;
                diagnostic_count += parsed.diagnostics.len();
                diagnostics.extend(parsed.diagnostics);
                sources.push(parsed.batch);
            }
            // 跳过的文件没有 source 行可回填，也不该被登记成某 provider 的源
            // （Unrecognized 已在上面 `continue`）。
            None => {
                if let Some(provider_id) = ctx.discovered_provider_ids.get(&paths[index]) {
                    provider_id_backfills.push((paths[index].clone(), provider_id.clone()));
                }
            }
        }
    }

    // discover 合成的空批（已删除源的 tombstone）追加进提交批次。
    sources.extend(synthetic_batches.iter().cloned());
    let mark = trace.phase("merge", mark);

    // 提交是首次索引里最慢的一步（实测占 98.2%，且代价随 catalog 增长），
    // 所以进入它之前先说一声——否则 human 模式会在这里静默数分钟，用户无法
    // 区分"正在写库"和"卡死了"。
    //
    // 空批次不报："committing 0 source(s)" 会宣告一件不会发生的事——全部源都
    // 指纹命中时根本没有提交要做。报数为 0 的进度比不报更糟，它让读者以为
    // 写库发生过。
    if !sources.is_empty() {
        progress.emit(
            &format!("committing {} source(s) to the store", sources.len()),
            request_id,
        );
    }

    let changed = store
        .commit_source_batches_if_changed(&sources)
        .map_err(ProtocolError::from)?;
    store
        .backfill_source_provider_ids(&provider_id_backfills)
        .map_err(ProtocolError::from)?;
    let generation = store.active_generation().map_err(ProtocolError::from)?;
    trace.phase("commit", mark);
    // `emitted` 只统计本次实际解析的消息；指纹缓存命中的源按已存消息数
    // 计入 unchanged（与 emitted 同单位：消息数）。截断尾被 retain 的源既不
    // 解析也不提交，单列 `retained`（源数），其诊断进 warnings 通道。
    let committed = if changed { message_count } else { 0 };
    // 文件级跳过与行级 skip 走同一条 warnings 通道（D11：宽容但报数）。逐源
    // 诊断已在 diagnostics 里；再加一条汇总，让"这轮少收了几个文件"一眼可见。
    if unrecognized_count > 0 {
        diagnostics.push(format!(
            "{unrecognized_count} discovered file(s) could not be attributed to a provider \
             and were skipped; everything else in this run was indexed normally"
        ));
        diagnostic_count += 1;
    }
    let warnings = diagnostic_warnings(diagnostics.iter().map(String::as_str), diagnostic_count);
    let source_count = paths.len() + synthetic_batches.len();
    Ok((
        serde_json::json!({
            "sources": source_count,
            "emitted": message_count,
            "messages": message_count,
            "committed": committed,
            "unchanged": if changed { unchanged_messages } else { message_count + unchanged_messages },
            "retained": retained_count,
            "skipped": skipped_count,
            "unrecognized": unrecognized_count,
            "diagnostics": diagnostic_count,
            "generation": generation,
        }),
        warnings,
    ))
}

/// 把应用结果投影为 (outcome, data, page, warnings)：截断 → partial（exit 10），
/// 分页令牌 → envelope `page`，可核验的降级事实 → warnings。前端只做投影，
/// 不再解释语义。
fn render(
    response: AppResponse,
) -> (
    protocol::Outcome,
    serde_json::Value,
    protocol::Page,
    Vec<String>,
) {
    match response {
        AppResponse::Search {
            hits,
            next_cursor,
            generation,
            truncation,
            retrieval_mode: effective_mode,
            fallback_warning,
            time_filter_excluded,
        } => {
            let outcome = outcome_of(&truncation);
            let page = protocol::Page {
                has_more: next_cursor.is_some(),
                next_cursor,
            };
            let mut warnings = Vec::new();
            if let Some(warning) = fallback_warning {
                warnings.push(warning);
            }
            // D11（宽容但报数）：时间窗把无时间戳的记录静默排除了，如实报数并
            // 给出下一步命令。0（无时间窗或无排除）时不发 warning，输出字节不变。
            if time_filter_excluded > 0 {
                warnings.push(format!(
                    "time filter excluded {time_filter_excluded} records with no timestamp; \
                     re-run without --since/--until to see them"
                ));
            }
            let data = serde_json::json!({
                "retrieval_mode": effective_mode.as_str(),
                "hits": hits
                    .into_iter()
                    .map(|hit| {
                        // search-match-guidance：guidance 为追加字段——空集合时
                        // 整个键省略，与既有机器人输出字节兼容。
                        let mut json = serde_json::json!({
                            "id": hit.id.as_str(),
                            "score": hit.score,
                            // R4（ADR-0008）：命中携带所属会话 wire id 与正文摘要
                            // （追加字段，schema minor：不删除任何既有字段）。
                            // `text` 字节已计入 Application 的 clamp_items 预算
                            // （R4.2）；人类渲染器把同一摘要打印为 snippet 行。
                            "session_id": hit.session_id,
                            "text": hit.text,
                        });
                        if !hit.why_matched.is_empty() {
                            json["why_matched"] = serde_json::json!(hit.why_matched);
                        }
                        if !hit.suggested_next_commands.is_empty() {
                            json["suggested_next_commands"] =
                                serde_json::json!(hit.suggested_next_commands);
                        }
                        // R3 occurrences：非归并命中恒为 1，与 guidance 一致采用
                        // "等于默认值即省略"的追加字段约定，保持既有输出字节兼容。
                        if hit.occurrences > 1 {
                            json["occurrences"] = serde_json::json!(hit.occurrences);
                        }
                        // resume_available（ADR-0009）：恒序列化，schema 1.1 声明。
                        json["resume_available"] = serde_json::json!(hit.resume_available);
                        json
                    })
                    .collect::<Vec<_>>(),
                "generation": generation,
                "truncation": truncation_json(&truncation),
            });
            (outcome, data, page, warnings)
        }
        AppResponse::Get { payload } => (
            protocol::Outcome::Success,
            serde_json::json!({
                "payload": payload.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            }),
            protocol::Page::default(),
            Vec::new(),
        ),
        // Resume Metadata（ADR-0009）：固定可空字段恒在；缺失统一 null，
        // 绝不回显 transcript/source path。
        AppResponse::SessionResume(metadata) => (
            protocol::Outcome::Success,
            serde_json::json!({
                "session_id": metadata.session_id.as_str(),
                "provider_id": metadata.provider_id,
                "resume_available": metadata.resume_available,
                "provider_session_id": metadata.provider_session_id,
                "original_working_directory": metadata.original_working_directory,
                "unavailable_reason": metadata.unavailable_reason,
            }),
            protocol::Page::default(),
            Vec::new(),
        ),
        // show 与 get 的区别：get 回原始 payload 字节，show 把存储的 canonical
        // JSON payload 展开成结构化 entity（含 role/text/parent/timestamp/threading）。
        // 未找到时 entity 为 null；payload 非合法 JSON 时按裸文本兜底。
        AppResponse::Show { payload } => (
            protocol::Outcome::Success,
            serde_json::json!({
                "entity": payload.map(|bytes| {
                    match serde_json::from_slice::<serde_json::Value>(&bytes) {
                        Ok(value) => value,
                        Err(_) => serde_json::json!({
                            "role": null,
                            "text": String::from_utf8_lossy(&bytes),
                        }),
                    }
                }),
            }),
            protocol::Page::default(),
            Vec::new(),
        ),
        AppResponse::List {
            entries,
            next_cursor,
            generation,
            truncation,
        } => {
            let outcome = outcome_of(&truncation);
            let page = protocol::Page {
                has_more: next_cursor.is_some(),
                next_cursor,
            };
            let data = serde_json::json!({
                "entries": entries
                    .into_iter()
                    .map(|entry| serde_json::json!({
                        "id": entry.id.as_str(),
                        "payload": String::from_utf8_lossy(&entry.payload),
                    }))
                    .collect::<Vec<_>>(),
                "generation": generation,
                "truncation": truncation_json(&truncation),
            });
            (outcome, data, page, Vec::new())
        }
        AppResponse::Context {
            session_id,
            session,
            branch_leaf,
            branch_leaf_placement_id,
            messages,
            evidence,
            tool_activities,
            requested_level,
            effective_level,
            talks,
            summary,
            hint,
            truncation,
            generation,
        } => {
            let outcome = outcome_of(&truncation);
            // 降级如实上报：legacy（无 span）行的证据精度是 unknown，调用方应知道
            // 重新 ingest 可恢复字节级定位（design §0.2）。
            let unknown = evidence
                .iter()
                .filter(|dto| dto.precision == Precision::Unknown)
                .count();
            let warnings = if unknown > 0 {
                vec![format!(
                    "{unknown} of {} evidence spans have unknown precision \
                     (legacy rows; re-ingest to restore byte spans)",
                    evidence.len()
                )]
            } else {
                Vec::new()
            };
            let data = serde_json::json!({
                "session_id": session_id,
                "session": session,
                "branch_leaf": branch_leaf,
                "branch_leaf_placement_id": branch_leaf_placement_id,
                "tool_activities": tool_activities,
                "messages": messages
                    .into_iter()
                    .map(|message| serde_json::json!({
                        "id": message.id,
                        "placement_id": message.placement_id,
                        "message_id": message.message_id,
                        "payload": message.payload,
                    }))
                    .collect::<Vec<_>>(),
                "evidence": evidence,
                "requested_level": requested_level,
                "effective_level": effective_level,
                "talks": talks,
                "summary": summary,
                "hint": hint,
                "truncation": truncation_json(&truncation),
                "generation": generation,
            });
            (outcome, data, protocol::Page::default(), warnings)
        }
        AppResponse::Message { window } => {
            let outcome = outcome_of(&window.truncation);
            let data = serde_json::json!({
                "message_id": window.message_id,
                "session_id": window.session_id,
                "anchor_placement_id": window.anchor_placement_id,
                "messages": window.messages
                    .into_iter()
                    .map(|message| serde_json::json!({
                        "id": message.id,
                        "placement_id": message.placement_id,
                        "message_id": message.message_id,
                        "payload": message.payload,
                    }))
                    .collect::<Vec<_>>(),
                "truncation": truncation_json(&window.truncation),
                "generation": window.generation,
            });
            (outcome, data, protocol::Page::default(), Vec::new())
        }
        AppResponse::MessageContexts {
            message_id,
            candidates,
        } => (
            protocol::Outcome::Success,
            serde_json::json!({
                "message_id": message_id,
                "candidates": candidates,
            }),
            protocol::Page::default(),
            Vec::new(),
        ),
        AppResponse::Status {
            catalog_count,
            active_generation,
            placements,
            source_placement_claims,
        } => (
            protocol::Outcome::Success,
            serde_json::json!({
                "catalog_count": catalog_count,
                "generation": active_generation,
                "placements": placements,
                "source_placement_claims": source_placement_claims,
            }),
            protocol::Page::default(),
            Vec::new(),
        ),
    }
}

fn outcome_of(truncation: &Truncation) -> protocol::Outcome {
    if truncation.truncated {
        protocol::Outcome::Partial
    } else {
        protocol::Outcome::Success
    }
}

fn truncation_json(truncation: &Truncation) -> serde_json::Value {
    serde_json::json!({
        "truncated": truncation.truncated,
        "reason": truncation.reason,
    })
}

fn arg<'a>(rest: &'a [String], index: usize, usage: &str) -> Result<&'a str, CliError> {
    rest.get(index)
        .map(String::as_str)
        .ok_or_else(|| CliError::usage(format!("missing argument: {usage}")))
}

/// 校验位置参数个数不超过 `expected`（命令名之外的裸参数）：多余的参数是
/// 用法错误（exit 2），不静默忽略——静默丢弃会让调用方误以为参数被接受。
fn no_extra_args(rest: &[String], expected: usize, usage: &str) -> Result<(), CliError> {
    if rest.len() > expected + 1 {
        return Err(CliError::usage(format!(
            "unexpected extra argument(s); usage: {usage}"
        )));
    }
    Ok(())
}

/// 变长位置参数（`sync <file>...`）不能用 `no_extra_args` 收口——路径数量本就
/// 不定。但把 `-` 开头的 token 当路径会产生一个**指向错误方向的诊断**：文件打
/// 不开报 `source_io`「确认源文件路径存在且可读」，而路径其实存在可读，真因是
/// 那个 flag 不存在。用户会去查权限、查盘符、查转义。
///
/// 因此位置参数里出现 `-` 开头的 token 一律报 `invalid_request` 并回显该 token。
/// 单独的 `-`（stdin 惯例）不在此列：它是合法的位置参数形态，不是拼错的 flag。
fn no_flag_like_positional(args: &[String], usage: &str) -> Result<(), CliError> {
    for arg in args {
        if arg.starts_with('-') && arg != "-" {
            return Err(CliError::usage(format!(
                "unknown flag {arg}; usage: {usage}"
            )));
        }
    }
    Ok(())
}

/// 读 `--from-file` 的源清单：每行一个路径，空行与 `#` 注释行忽略。
///
/// 行内不做 glob 展开、不做 shell 引号解析：一行就是一个路径，字面。这样含空格
/// 的路径不需要引号，也不会被误当成两个路径——清单文件的存在意义就是绕开
/// shell 的分词。
fn read_source_list(path: &str) -> Result<Vec<String>, CliError> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        // 清单本身读不到是用法错误（用户给错了 --from-file 的路径），不是
        // source_io——后者指 transcript 源读不到，两者的下一步动作不同。
        CliError::usage(format!("sync --from-file: 无法读取清单 {path}: {error}"))
    })?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_session_grep_application::{EvidenceSpanDto, StagedMessage};
    use agent_session_grep_ports::ParseReport;

    /// `parallel_map_indexed` must return results in index order and invoke
    /// `on_ready` in index order, no matter which worker finishes first.
    ///
    /// This is the property the whole parallel sync path rests on: message
    /// identity for providers that emit an empty `native_id` is derived per
    /// document, so if completion order ever leaked into the writer's view of
    /// the batch, ids and "source N of M" diagnostics would shift between runs.
    /// Reversed sleeps make completion order the exact opposite of index order,
    /// so an implementation that forwarded results as they landed would fail.
    #[test]
    fn parallel_map_indexed_restores_index_order_despite_completion_order() {
        let len = 24;
        let ready = std::sync::Mutex::new(Vec::new());
        let out = parallel_map_indexed(
            len,
            |index| {
                // Later indices finish first.
                std::thread::sleep(std::time::Duration::from_millis(((len - index) * 2) as u64));
                Ok(index * 10)
            },
            |index, value: &usize| ready.lock().unwrap().push((index, *value)),
        )
        .expect("no unit fails");

        let expected: Vec<usize> = (0..len).map(|index| index * 10).collect();
        assert_eq!(out, expected, "results must come back in index order");
        let expected_ready: Vec<(usize, usize)> =
            (0..len).map(|index| (index, index * 10)).collect();
        assert_eq!(
            *ready.lock().unwrap(),
            expected_ready,
            "on_ready must fire in index order, not completion order"
        );
    }

    /// The lowest-index failure must win, matching a serial loop's early return,
    /// so the same corpus always reports the same first error.
    #[test]
    fn parallel_map_indexed_reports_the_lowest_index_error() {
        // Index 3 and index 9 both fail; index 9 finishes first.
        let result = parallel_map_indexed(
            16,
            |index| {
                if index == 9 {
                    return Err(CliError::usage("late failure"));
                }
                if index == 3 {
                    std::thread::sleep(std::time::Duration::from_millis(40));
                    return Err(CliError::usage("early failure"));
                }
                Ok(index)
            },
            |_, _: &usize| {},
        );
        let error = result.expect_err("two units fail");
        assert!(
            error.0.message.contains("early failure"),
            "expected the lowest-index error, got {:?}",
            error.0.message
        );
    }

    /// A single unit takes the inline path; it must still behave identically.
    #[test]
    fn parallel_map_indexed_handles_zero_and_one_unit() {
        let empty = parallel_map_indexed(0, |_| Ok(1usize), |_, _: &usize| {}).unwrap();
        assert!(empty.is_empty());

        let ready = std::sync::Mutex::new(Vec::new());
        let one = parallel_map_indexed(
            1,
            |index| Ok(index + 7),
            |index, value: &usize| ready.lock().unwrap().push((index, *value)),
        )
        .unwrap();
        assert_eq!(one, vec![7]);
        assert_eq!(*ready.lock().unwrap(), vec![(0usize, 7usize)]);
    }

    fn staged_batch(
        messages: Vec<StagedMessage>,
        skipped: usize,
        session_native_id: &str,
    ) -> StagedBatch {
        let committed = messages.len();
        StagedBatch {
            messages,
            activities: Vec::new(),
            session_native_id: Some(session_native_id.into()),
            report: ParseReport {
                committed,
                skipped,
                diagnostics: if skipped == 0 {
                    Vec::new()
                } else {
                    vec!["synthetic skipped record".into()]
                },
                session_native_id: Some(session_native_id.into()),
                session_observation: ProviderSessionObservation::default(),
            },
        }
    }

    fn staged_message(seq: u32, native_id: &str, span: (u64, u64)) -> StagedMessage {
        StagedMessage {
            seq,
            native_id: native_id.into(),
            parent_native_id: None,
            role: "user".into(),
            text: "synthetic body".into(),
            timestamp: Some("2026-07-28T00:00:00Z".into()),
            is_sidechain: false,
            span: Some(span),
        }
    }

    #[test]
    fn provider_data_root_rejects_unknown_provider() {
        assert!(provider_data_root("unknown-provider").is_none());
    }

    /// `discover` 声明与实际注册的 root 必须一对一（M1-8）。
    ///
    /// 这条缺失时矩阵给 claude-code/codex 之外的四个已注册 provider
    /// （openclaw / tencent-codebuddy / antigravity / opencode）报
    /// `discover: Unsupported`，而 `sync --discover` 其实一直在扫它们的 root。
    /// 实测（伪 HOME 下每个 root 种一份 golden fixture）六个 root 全部
    /// `root_state=scanned found=1`，所以假的是声明那一侧。
    ///
    /// 双向断言：漏声明（注册了 root 却报 Unsupported）与过度声明
    /// （声明 Native 却没注册 root）都会失败。
    #[test]
    fn matrix_discover_claim_matches_registered_roots() {
        use agent_session_grep_ports::capability::CapabilityLevel;
        for capability in &ProviderCapabilityMatrix::current().providers {
            let registered = provider_root_subpath(&capability.provider_id).is_some();
            let claims_native = capability.discover == CapabilityLevel::Native;
            assert_eq!(
                claims_native, registered,
                "provider {}: discover={:?} but provider_root_subpath registered={registered}. \
                 `discover: Native` 当且仅当注册了 discovery root——要么注册 root，\
                 要么把声明改成 Unsupported，不要让矩阵自我认证。",
                capability.provider_id, capability.discover
            );
        }
    }

    /// 未注册 root 的 provider 必须声明 `discover: Unsupported` 而不是 `Unknown`：
    /// "没有 root" 是已确定的事实，不是待评估项。deferred provider
    /// （无 transcript 证据）例外——它们整行都是 `Unknown`。
    #[test]
    fn unregistered_providers_declare_discover_unsupported() {
        use agent_session_grep_ports::capability::CapabilityLevel;
        for capability in &ProviderCapabilityMatrix::current().providers {
            if capability.maturity == ProviderMaturity::Unsupported
                || provider_root_subpath(&capability.provider_id).is_some()
            {
                continue;
            }
            assert_eq!(
                capability.discover,
                CapabilityLevel::Unsupported,
                "provider {}: 未注册 discovery root 的已实现 provider 应声明 Unsupported",
                capability.provider_id
            );
        }
    }

    #[test]
    fn provider_matrix_data_adds_semantic_surface_without_touching_rows() {
        let data = provider_matrix_data();
        // 加法键：data 只有 providers（原样 16 行）与 semantic 两个键。
        assert_eq!(data.as_object().unwrap().len(), 2);
        assert_eq!(
            data["providers"].as_array().unwrap().len(),
            ProviderCapabilityMatrix::current().providers.len()
        );
        let semantic = &data["semantic"];
        // 键集跨构建稳定。
        let keys = semantic
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            ["default_model", "feature", "runtime"]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
        );
        // 诚实默认向量化器事实：任何构建都是 bigram-hash-v1。
        assert_eq!(
            semantic["default_model"],
            agent_session_grep_application::embedding::BIGRAM_HASH_MODEL_ID
        );
        // feature/runtime 严格跟随 cfg(feature = "semantic-candle")：
        // 默认构建断言 null 形状，feature 构建断言稳定标识（CI 两种构建都会跑到）。
        if cfg!(feature = "semantic-candle") {
            assert_eq!(semantic["feature"], "semantic-candle");
            assert_eq!(semantic["runtime"], "candle-e5-local");
        } else {
            assert!(semantic["feature"].is_null());
            assert!(semantic["runtime"].is_null());
        }
    }

    #[test]
    fn doctor_semantic_feature_flag_tracks_build() {
        let flag = semantic_feature_flag();
        if cfg!(feature = "semantic-candle") {
            assert_eq!(flag, serde_json::json!(true));
        } else {
            assert!(flag.is_null());
        }
    }

    /// 一行合法 claude-code transcript，用于发现期 probe 判定。
    #[cfg(test)]
    fn discovery_test_transcript() -> String {
        format!(
            "{}\n",
            concat!(
                r#"{"type":"user","uuid":"d1c00000-0000-4000-8000-000000000001","#,
                r#""parentUuid":null,"sessionId":"d1c00000-0000-4000-8000-000000000002","#,
                r#""timestamp":"2026-08-14T01:00:00.000Z","#,
                r#""message":{"role":"user","content":"discovery fixture"}}"#,
            )
        )
    }

    #[test]
    fn discover_provider_sources_collects_recognized_transcripts_recursively() {
        // 判定改由 provider probe 回答（M2P-7），不再看扩展名：真 transcript 收下，
        // 同目录下的非 transcript 文件（哪怕叫 .jsonl）不收。
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("one.jsonl"), discovery_test_transcript()).unwrap();
        std::fs::write(nested.join("two.txt"), b"not a source").unwrap();
        // 扩展名对了但内容不是 transcript：旧的 `.jsonl` 过滤会收下它。
        std::fs::write(nested.join("decoy.jsonl"), b"not a source either").unwrap();
        let registry = provider_registry();
        let refs: Vec<&dyn ProviderAdapter> = registry.iter().map(|a| a.as_ref()).collect();
        let (paths, complete) = discover_provider_sources(dir.path(), &refs);
        assert!(complete);
        assert_eq!(paths.len(), 1, "{paths:?}");
        assert!(paths[0].ends_with("nested/one.jsonl"), "{paths:?}");
    }

    #[test]
    fn discover_provider_sources_collects_sqlite_transcripts() {
        // `.jsonl` 过滤让 SQLite 源（opencode.db / state.vscdb）在零配置路径上
        // 永远不可达（M2P-7）。magic header 即家族签名，权威判定留给 sync。
        let dir = tempfile::tempdir().unwrap();
        let mut sqlite_bytes = agent_session_grep_ports::SQLITE_MAGIC_HEADER.to_vec();
        sqlite_bytes.extend_from_slice(&[0_u8; 64]);
        std::fs::write(dir.path().join("opencode.db"), &sqlite_bytes).unwrap();
        let registry = provider_registry();
        let refs: Vec<&dyn ProviderAdapter> = registry.iter().map(|a| a.as_ref()).collect();
        let (paths, complete) = discover_provider_sources(dir.path(), &refs);
        assert!(complete);
        assert_eq!(paths.len(), 1, "{paths:?}");
        assert!(paths[0].ends_with("opencode.db"), "{paths:?}");
    }

    #[cfg(unix)]
    #[test]
    fn discover_provider_sources_does_not_follow_symlink_root() {
        let dir = tempfile::tempdir().unwrap();
        let real_root = dir.path().join("real-root");
        std::fs::create_dir(&real_root).unwrap();
        std::fs::write(real_root.join("hidden.jsonl"), discovery_test_transcript()).unwrap();
        let linked_root = dir.path().join("linked-root");
        std::os::unix::fs::symlink(&real_root, &linked_root).unwrap();

        let registry = provider_registry();
        let refs: Vec<&dyn ProviderAdapter> = registry.iter().map(|a| a.as_ref()).collect();
        let (paths, complete) = discover_provider_sources(&linked_root, &refs);
        assert!(paths.is_empty());
        assert!(!complete);
    }

    #[test]
    fn installation_namespace_groups_sources_by_provider_root() {
        assert_eq!(
            installation_namespace("C:/profiles/one/.claude/projects/a.jsonl", "claude-code"),
            installation_namespace("C:/profiles/one/.claude/projects/b.jsonl", "claude-code")
        );
        assert_ne!(
            installation_namespace("C:/profiles/one/.claude/projects/a.jsonl", "claude-code"),
            installation_namespace("D:/profiles/two/.claude/projects/a.jsonl", "claude-code")
        );
        assert_ne!(
            installation_namespace("C:/profiles/one/.claude/projects/a.jsonl", "claude-code"),
            installation_namespace("C:/profiles/one/.codex/sessions/a.jsonl", "codex")
        );
    }

    #[test]
    fn installation_namespace_fallback_groups_sibling_sources() {
        assert_eq!(
            installation_namespace("C:/fixtures/head.jsonl", "synthetic"),
            installation_namespace("C:/fixtures/tail.jsonl", "synthetic")
        );
        assert_ne!(
            installation_namespace("C:/fixtures/head.jsonl", "synthetic"),
            installation_namespace("D:/other/head.jsonl", "synthetic")
        );
    }

    /// 把内容写入临时文件并 capture，再按生产路径的流式分类。
    fn health_of(content: &[u8]) -> JsonlHealth {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("health.jsonl");
        std::fs::write(&path, content).unwrap();
        let snap = capture(&path).unwrap();
        jsonl_health(&path, &snap).unwrap()
    }

    #[test]
    fn jsonl_health_classifies_clean_partial_and_truncated() {
        // 完全合法（含无尾换行、CRLF、空行、首行 UTF-8 BOM、空文件）→ Clean。
        assert_eq!(health_of(b"{\"a\":1}\n{\"b\":2}\n"), JsonlHealth::Clean);
        assert_eq!(health_of(b"{\"a\":1}"), JsonlHealth::Clean);
        assert_eq!(health_of(b"{\"a\":1}\r\n{\"b\":2}\r\n"), JsonlHealth::Clean);
        assert_eq!(health_of(b"\n  \n{\"a\":1}\n"), JsonlHealth::Clean);
        assert_eq!(
            health_of(b"\xEF\xBB\xBF{\"a\":1}\n{\"b\":2}\n"),
            JsonlHealth::Clean
        );
        assert_eq!(health_of(b""), JsonlHealth::Clean);
        // 坏行后仍有合法行 → Partial（recoverable-skip，provider 逐行跳过）。
        assert_eq!(
            health_of(b"{\"a\":1}\n{\n{\"b\":2}\n"),
            JsonlHealth::Partial
        );
        // 尾部截断（EOF 落在记录中间）与全垃圾 → Invalid（已索引源触发 Retain）。
        assert_eq!(health_of(b"{\"a\":1}\n{\"b\":2"), JsonlHealth::Invalid);
        assert_eq!(health_of(b"{\"a\":1\n"), JsonlHealth::Invalid);
        assert_eq!(health_of(b"{\n"), JsonlHealth::Invalid);
        assert_eq!(health_of(b"garbage"), JsonlHealth::Invalid);
        // 与 fast-resume 测试矩阵对齐（shared.rs tests::classifies_clean_partial_and_invalid_jsonl）。
        assert_eq!(
            health_of(b"{\"valid\":true}\n{\n{\"later\":true}\n"),
            JsonlHealth::Partial
        );
        assert_eq!(health_of(b"{\"valid\":true}\n{\n"), JsonlHealth::Invalid);
    }

    #[test]
    fn jsonl_health_never_claims_completeness_on_error() {
        // failed-scan 不变量在分类层的体现：任何无法完整解析的状态（截断/垃圾/
        // 不可读）都不得给出 Clean——Clean 是"可安全替换旧索引"的唯一信号。
        assert_ne!(health_of(b"{\"a\":1}\n{\"b\":2"), JsonlHealth::Clean);
        assert_ne!(health_of(b"{\n"), JsonlHealth::Clean);
        assert_ne!(health_of(b"{\"a\":1}\n{\n"), JsonlHealth::Clean);
    }

    #[test]
    fn fallback_message_id_is_path_independent_but_placement_is_installation_scoped() {
        let staged = staged_batch(vec![staged_message(0, "", (0, 4))], 0, "session-1");
        let first = staged_to_source(
            "C:/one/transcript.jsonl",
            &staged,
            "synthetic",
            "synthetic/jsonl-v1",
            "same-fingerprint",
            8,
        )
        .unwrap();
        let second = staged_to_source(
            "D:/moved/transcript.jsonl",
            &staged,
            "synthetic",
            "synthetic/jsonl-v1",
            "same-fingerprint",
            8,
        )
        .unwrap();

        let first_message = first
            .entries
            .iter()
            .find(|(id, _, _)| id.kind() == IdKind::Message)
            .unwrap();
        let second_message = second
            .entries
            .iter()
            .find(|(id, _, _)| id.kind() == IdKind::Message)
            .unwrap();
        assert_eq!(first_message.0, second_message.0);
        assert_eq!(first_message.0.stability(), Stability::Unstable);
        assert_ne!(first.placements[0].id, second.placements[0].id);
        assert_ne!(first.source_path, second.source_path);
    }

    #[test]
    fn duplicate_native_message_keeps_one_entity_and_every_placement() {
        let staged = staged_batch(
            vec![
                staged_message(0, "shared-native", (0, 4)),
                staged_message(1, "shared-native", (5, 9)),
            ],
            0,
            "session-1",
        );
        let source = staged_to_source(
            "synthetic.jsonl",
            &staged,
            "synthetic",
            "synthetic/jsonl-v1",
            "fingerprint",
            16,
        )
        .unwrap();

        assert_eq!(
            source
                .entries
                .iter()
                .filter(|(id, _, _)| id.kind() == IdKind::Message)
                .count(),
            1
        );
        assert_eq!(source.placements.len(), 2);
        let message_payload: serde_json::Value = serde_json::from_slice(
            &source
                .entries
                .iter()
                .find(|(id, _, _)| id.kind() == IdKind::Message)
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(message_payload["spans"].as_array().unwrap().len(), 2);
        let session_payload: serde_json::Value = serde_json::from_slice(
            &source
                .entries
                .iter()
                .find(|(id, _, _)| id.kind() == IdKind::Session)
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(session_payload["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn stable_projection_conflict_does_not_disclose_native_id() {
        let private_native_id = "private-provider-native-id";
        let first = staged_message(0, private_native_id, (0, 4));
        let mut second = staged_message(1, private_native_id, (5, 9));
        second.text = "different stable text".into();
        let staged = staged_batch(vec![first, second], 0, "session-1");
        let error = staged_to_source(
            "synthetic.jsonl",
            &staged,
            "synthetic",
            "synthetic/jsonl-v1",
            "fingerprint",
            16,
        );
        let error = match error {
            Err(error) => error,
            Ok(_) => panic!("expected stable projection conflict"),
        };
        assert!(error.0.message.contains("conflicting stable projections"));
        assert!(!error.0.message.contains(private_native_id));
    }

    #[test]
    fn skipped_records_keep_source_relation_incomplete() {
        let staged = staged_batch(vec![staged_message(0, "native", (0, 4))], 1, "session-1");
        let source = staged_to_source(
            "synthetic.jsonl",
            &staged,
            "synthetic",
            "synthetic/jsonl-v1",
            "fingerprint",
            8,
        )
        .unwrap();
        assert!(!source.relation_complete);
    }

    #[test]
    fn parse_report_committed_count_must_match_emitted_messages() {
        let mut staged = staged_batch(vec![staged_message(0, "native", (0, 4))], 0, "session-1");
        staged.report.committed = 2;
        let error = staged_to_source(
            "synthetic.jsonl",
            &staged,
            "synthetic",
            "synthetic/jsonl-v1",
            "fingerprint",
            8,
        );
        let error = match error {
            Err(error) => error,
            Ok(_) => panic!("expected committed/emitted mismatch"),
        };
        assert_eq!(error.0.code, CanonicalCode::Internal);
    }

    fn dto(precision: Precision) -> EvidenceSpanDto {
        EvidenceSpanDto {
            occurrence_id: "occ".into(),
            message_id: "msg_v1_x".into(),
            source_document_id: None,
            generation: 1,
            source_fingerprint: None,
            byte_start: None,
            byte_end: None,
            line_start: None,
            line_end: None,
            record_ordinal: Some(0),
            snippet_char_start: None,
            snippet_char_end: None,
            precision,
        }
    }

    fn context_response(evidence: Vec<EvidenceSpanDto>) -> AppResponse {
        AppResponse::Context {
            session_id: "ses_v1_s".into(),
            session: serde_json::json!({}),
            branch_leaf: None,
            branch_leaf_placement_id: None,
            messages: Vec::new(),
            evidence,
            tool_activities: Vec::new(),
            requested_level: ContextLevel::Raw,
            effective_level: ContextLevel::Raw,
            talks: Vec::new(),
            summary: None,
            hint: None,
            truncation: Truncation {
                truncated: false,
                reason: None,
            },
            generation: 1,
        }
    }

    #[test]
    fn context_render_warns_on_unknown_precision_evidence() {
        // 3 条证据中 2 条 unknown → 一条如实计数的 warning（design §0.2）。
        let (_, _, _, warnings) = render(context_response(vec![
            dto(Precision::Byte),
            dto(Precision::Unknown),
            dto(Precision::Unknown),
        ]));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("2 of 3"), "{warnings:?}");
        assert!(warnings[0].contains("re-ingest"), "{warnings:?}");
    }

    #[test]
    fn context_render_stays_silent_on_full_precision() {
        let (_, _, _, warnings) = render(context_response(vec![dto(Precision::Byte)]));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    // ---- 参数解析（Minor-2 / Minor-3）----

    #[test]
    fn known_subcommand_and_unknown_error_share_one_source_of_truth() {
        // M2P-8：过去 known_subcommand 与 unknown-subcommand 报错各有一份手写列表，
        // 报错那份漏了 handoff/resume/hook/serve/providers/model/get-session-resume。
        // 现在两者都从 KNOWN_SUBCOMMANDS 派生——本测试在任一侧漂移时失败。
        let message = unknown_subcommand_error("definitely-not-a-command")
            .0
            .message;
        for command in KNOWN_SUBCOMMANDS {
            assert!(
                known_subcommand(command),
                "{command} 在 KNOWN_SUBCOMMANDS 里却不被 known_subcommand 承认"
            );
            assert!(
                message.contains(*command),
                "unknown-subcommand 报错漏了 {command}: {message}"
            );
        }
        // help 拦截与报错共用同一判定：非列表成员一律不是子命令。
        assert!(!known_subcommand("handof"));
        assert!(!known_subcommand(""));
        // 报错里出现的每个候选都必须是真命令（防止列表混入不存在的名字）。
        let listed = message
            .rsplit_once("Available commands: ")
            .map(|(_, tail)| tail)
            .and_then(|tail| tail.split_once('\n'))
            .map(|(list, _)| list)
            .expect("报错必须包含可用命令表");
        let listed: Vec<&str> = listed.split(", ").collect();
        assert_eq!(
            listed, KNOWN_SUBCOMMANDS,
            "报错列表必须与 KNOWN_SUBCOMMANDS 逐项一致"
        );
    }

    #[test]
    fn unknown_subcommand_suggests_the_closest_real_command() {
        // did-you-mean：一次编辑距离内的笔误必须被指出来。
        assert_eq!(closest_subcommand("handof"), Some("handoff"));
        assert_eq!(closest_subcommand("serch"), Some("search"));
        assert_eq!(closest_subcommand("statuss"), Some("status"));
        assert_eq!(closest_subcommand("SEARCH"), Some("search"));
        // 差得太远时不瞎猜。
        assert_eq!(closest_subcommand("completely-different"), None);
        assert_eq!(closest_subcommand(""), None);
        // 短名字只容一次编辑：两次编辑会把它变成另一个真命令。
        assert_eq!(closest_subcommand("gt"), Some("get"));
    }

    #[test]
    fn store_path_resolution_prefers_flag_then_env_then_default() {
        // M2P-3：`--db` > `$ASG_DB` > 平台数据目录默认值。flag 分支不读环境，
        // 因此这条断言不受测试进程环境影响。
        let (path, origin) = resolve_store_path(Some("explicit.db".into())).expect("flag path");
        assert_eq!(path, "explicit.db");
        assert_eq!(origin, DbOrigin::Flag);

        // 默认与环境分支解析出的路径无需 `--db` 前缀，显式 flag 必须带上。
        assert_eq!(db_flag_prefix("x.db", DbOrigin::Default), "");
        assert_eq!(db_flag_prefix("x.db", DbOrigin::Env), "");
        assert_eq!(db_flag_prefix("x.db", DbOrigin::Flag), "--db x.db ");
        assert_eq!(
            sync_discover_command("x.db", DbOrigin::Default),
            "asg sync --discover"
        );
        assert_eq!(
            sync_discover_command("x.db", DbOrigin::Flag),
            "asg --db x.db sync --discover"
        );
    }

    #[test]
    fn missing_store_error_is_not_found_and_names_the_next_command() {
        // M2P-2：读路径缺库是 not_found（exit 4），消息里必须有路径与全命令。
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("absent.db");
        let missing_s = missing.to_string_lossy().into_owned();
        let error = require_existing_store(&missing_s, DbOrigin::Flag)
            .expect_err("缺库必须报错而不是静默新建");
        assert_eq!(error.0.code, CanonicalCode::NotFound);
        assert_eq!(error.0.code.exit_code(), 4);
        assert!(error.0.message.contains(&missing_s), "{}", error.0.message);
        assert!(error.0.message.contains("asg --db"), "{}", error.0.message);
        assert!(
            error.0.message.contains("sync --discover"),
            "{}",
            error.0.message
        );
        // 检查本身无副作用：不得把库或父目录造出来。
        assert!(!missing.exists());

        // 库存在时放行。
        std::fs::write(&missing, b"").expect("touch store");
        assert!(require_existing_store(&missing_s, DbOrigin::Flag).is_ok());
    }

    #[test]
    fn parse_db_flag_keeps_flag_named_tokens_after_command_as_query_text() {
        // 命令名之后的 token 即使形如 flag 也是位置参数（查询文本），
        // 不得被全局 flag 跳过逻辑吞掉。
        let (db, rest) = parse_db_flag(&[
            "--db".into(),
            "store.db".into(),
            "search".into(),
            "--output".into(),
        ])
        .expect("valid db flag");
        assert_eq!(db.as_deref(), Some("store.db"));
        assert_eq!(rest, vec!["search".to_string(), "--output".to_string()]);

        let (_, rest) = parse_db_flag(&[
            "--db".into(),
            "store.db".into(),
            "--robot".into(),
            "search".into(),
            "--help".into(),
        ])
        .expect("valid db flag");
        assert_eq!(rest, vec!["search".to_string(), "--help".to_string()]);
    }

    #[test]
    fn parse_db_flag_treats_flags_after_command_as_positionals() {
        // `status --db x`：命令后的 --db 是多余位置参数（由 no_extra_args 拒绝），
        // 不再被当全局 flag 消费——flag 只在前缀位置识别。
        let (db, rest) = parse_db_flag(&[
            "--db".into(),
            "s.db".into(),
            "status".into(),
            "--db".into(),
            "x".into(),
        ])
        .expect("valid db flag");
        assert_eq!(db.as_deref(), Some("s.db"));
        assert_eq!(
            rest,
            vec!["status".to_string(), "--db".to_string(), "x".to_string(),]
        );
    }

    #[test]
    fn source_list_ignores_blank_lines_and_comments_but_keeps_spaces() {
        // `--from-file` 存在的意义是绕开 shell 分词，所以含空格的路径必须原样
        // 保留（不做引号解析、不做 glob 展开）——否则清单又要处理引号，缺口没补上。
        let dir = tempfile::tempdir().expect("tempdir");
        let list = dir.path().join("sources.txt");
        std::fs::write(
            &list,
            "# 注释行\n\
             \n\
             C:/dir with spaces/a.jsonl\n\
             \t  b.jsonl  \t\n\
             # 又一条注释\n\
             c.jsonl\n",
        )
        .expect("write list");

        let paths = read_source_list(&list.to_string_lossy()).expect("a readable list must parse");
        assert_eq!(
            paths,
            vec![
                "C:/dir with spaces/a.jsonl".to_string(),
                "b.jsonl".to_string(),
                "c.jsonl".to_string(),
            ]
        );
    }

    #[test]
    fn sync_provider_flag_accepts_only_registered_adapters() {
        // 权威是 provider_registry：能力矩阵里那两个 deferred provider 没有
        // adapter，点名它们只会换来"没人认领"这种误导性错误。
        assert_eq!(validated_sync_provider("pi").unwrap(), "pi");
        assert_eq!(validated_sync_provider("openclaw").unwrap(), "openclaw");

        let err = validated_sync_provider("not-a-provider")
            .expect_err("未注册的 provider 必须拒绝，绝不静默忽略");
        assert_eq!(err.0.code, CanonicalCode::InvalidRequest);
        // R2.2 可行动性：必须列出可选值，否则用户只能猜。
        assert!(
            err.0.message.contains("pi") && err.0.message.contains("openclaw"),
            "错误消息必须列出全部可选 provider: {}",
            err.0.message
        );
    }

    #[test]
    fn missing_source_list_is_a_usage_error_not_a_source_io_error() {
        // 清单读不到 = 用户给错了 --from-file；transcript 读不到 = source_io。
        // 两者的下一步动作不同，不能塌成同一个码。
        let err = read_source_list("this-list-does-not-exist.txt")
            .expect_err("a missing list must be rejected");
        assert_eq!(err.0.code, CanonicalCode::InvalidRequest);
        assert!(
            err.0.message.contains("--from-file"),
            "message must name the flag: {}",
            err.0.message
        );
    }

    #[test]
    fn flag_like_positionals_name_the_flag_instead_of_blaming_the_path() {
        // 变长位置参数（`sync <file>...`）若把 flag 当路径，用户会拿到
        // `source_io`「确认源文件路径存在且可读」——而路径其实存在可读。
        let usage = "sync <file>... | sync --discover";
        let err = no_flag_like_positional(&["--provider".into(), "a.jsonl".into()], usage)
            .expect_err("a flag-like positional must be rejected");
        assert!(
            err.0.message.contains("--provider"),
            "the message must name the offending flag: {}",
            err.0.message
        );
        assert_eq!(err.0.code, CanonicalCode::InvalidRequest);

        // 真正的路径不受影响，`-`（stdin 惯例）也是合法位置参数。
        assert!(no_flag_like_positional(&["a.jsonl".into(), "b.jsonl".into()], usage).is_ok());
        assert!(no_flag_like_positional(&["-".into()], usage).is_ok());
        assert!(no_flag_like_positional(&[], usage).is_ok());
    }

    #[test]
    fn extra_positional_arguments_are_usage_errors() {
        assert!(no_extra_args(&["get".into(), "id".into()], 1, "get <wire-id>").is_ok());
        assert!(
            no_extra_args(
                &["get".into(), "id".into(), "extra".into()],
                1,
                "get <wire-id>"
            )
            .is_err()
        );
        assert!(no_extra_args(&["status".into()], 0, "status").is_ok());
        assert!(no_extra_args(&["status".into(), "x".into()], 0, "status").is_err());
        assert!(no_extra_args(&["index".into(), "rebuild".into()], 1, "index rebuild").is_ok());
        assert!(
            no_extra_args(
                &["index".into(), "rebuild".into(), "x".into()],
                1,
                "index rebuild"
            )
            .is_err()
        );
    }

    #[test]
    fn bare_positionals_skip_value_and_bare_flags() {
        assert_eq!(
            bare_positionals(&[
                "--robot".into(),
                "doctor".into(),
                "--db".into(),
                "s.db".into(),
                "--output".into(),
                "json".into(),
            ]),
            vec!["doctor".to_string()]
        );
        // doctor 多余位置参数 → 用法错误。
        assert_eq!(
            bare_positionals(&["doctor".into(), "bogus".into()]),
            vec!["doctor".to_string(), "bogus".to_string()]
        );
    }

    #[test]
    fn extract_request_id_only_reads_prefix_position() {
        // 前缀位置（命令名之前）的 --request-id 正常抽取。
        assert_eq!(
            extract_request_id(&[
                "--db".into(),
                "s.db".into(),
                "--request-id".into(),
                "corr.1".into(),
                "status".into(),
            ])
            .expect("valid request id"),
            Some("corr.1".to_string())
        );
        // 命令名之后的同名 token 是查询文本，不是 flag。
        assert_eq!(
            extract_request_id(&["search".into(), "--request-id".into()]).expect("no flag"),
            None
        );
        // 带值 flag 的取值跳过，不会误停扫描；缺失值仍是用法错误。
        assert_eq!(
            extract_request_id(&["--db".into(), "s.db".into()]).expect("no flag"),
            None
        );
        assert!(extract_request_id(&["--request-id".into()]).is_err());
    }

    // ---- Provider capability matrix entry-point projection ----

    #[test]
    fn provider_output_has_every_current_matrix_row_and_enum_maturity() {
        let matrix = ProviderCapabilityMatrix::current();
        let output = provider_matrix_data();
        let rows = output["providers"].as_array().expect("providers array");
        assert_eq!(rows.len(), matrix.providers.len());
        for (row, capability) in rows.iter().zip(&matrix.providers) {
            assert_eq!(row["provider_id"], capability.provider_id);
            assert_eq!(
                row["maturity"],
                serde_json::to_value(capability.maturity).expect("serialize maturity")
            );
            assert_eq!(
                row["maturity_target"],
                serde_json::to_value(ProviderMaturity::target_for(&capability.provider_id))
                    .expect("serialize maturity target")
            );
        }
    }

    #[test]
    fn deferred_provider_output_has_null_maturity_target() {
        let output = provider_matrix_data();
        let rows = output["providers"].as_array().expect("providers array");
        for provider_id in ["deepseek-harness", "zcode"] {
            let row = rows
                .iter()
                .find(|row| row["provider_id"] == provider_id)
                .unwrap_or_else(|| panic!("missing deferred provider {provider_id}"));
            assert!(row["maturity_target"].is_null(), "{row}");
        }
    }

    // ---- help/version 提前拦截（ADR-0006，R3）----

    const KNOWN_COMMANDS: [&str; 20] = [
        "ingest",
        "sync",
        "index",
        "search",
        "handoff",
        "get-message",
        "get-session-resume",
        "resume",
        "hook",
        "get",
        "show",
        "list",
        "context",
        "status",
        "mcp",
        "tui",
        "doctor",
        "providers",
        "config",
        "model",
    ];

    #[test]
    fn intercept_top_level_help_and_version() {
        assert_eq!(intercept_help_or_version(&[]), None);
        assert_eq!(
            intercept_help_or_version(&["--help".into()]),
            Some(HelpRequest::TopLevelHelp)
        );
        assert_eq!(
            intercept_help_or_version(&["-h".into()]),
            Some(HelpRequest::TopLevelHelp)
        );
        assert_eq!(
            intercept_help_or_version(&["--version".into()]),
            Some(HelpRequest::TopLevelVersion)
        );
        assert_eq!(
            intercept_help_or_version(&["-V".into()]),
            Some(HelpRequest::TopLevelVersion)
        );
        // 带值 flag 的取值跳过，不影响顶层拦截。
        assert_eq!(
            intercept_help_or_version(&["--db".into(), "s.db".into(), "--help".into()]),
            Some(HelpRequest::TopLevelHelp)
        );
        assert_eq!(
            intercept_help_or_version(&["--output".into(), "json".into(), "--version".into()]),
            Some(HelpRequest::TopLevelVersion)
        );
        // 两者同时出现时帮助优先（与旧行为一致）。
        assert_eq!(
            intercept_help_or_version(&["--help".into(), "--version".into()]),
            Some(HelpRequest::TopLevelHelp)
        );
    }

    #[test]
    fn intercept_subcommand_help_for_every_known_command() {
        for cmd in KNOWN_COMMANDS {
            assert_eq!(
                intercept_help_or_version(&[cmd.into(), "--help".into()]),
                Some(HelpRequest::SubcommandHelp(cmd.into())),
                "{cmd} --help"
            );
            assert_eq!(
                intercept_help_or_version(&[cmd.into(), "-h".into()]),
                Some(HelpRequest::SubcommandHelp(cmd.into())),
                "{cmd} -h"
            );
            // --db 前置时同样拦截——help 不要求 --db（R3.1）。
            assert_eq!(
                intercept_help_or_version(&[
                    "--db".into(),
                    "s.db".into(),
                    cmd.into(),
                    "--help".into()
                ]),
                Some(HelpRequest::SubcommandHelp(cmd.into())),
                "--db s.db {cmd} --help"
            );
        }
        // index rebuild --help / -h 也覆盖。
        assert_eq!(
            intercept_help_or_version(&["index".into(), "rebuild".into(), "--help".into()]),
            Some(HelpRequest::SubcommandHelp("index".into()))
        );
        assert_eq!(
            intercept_help_or_version(&[
                "--db".into(),
                "s.db".into(),
                "index".into(),
                "rebuild".into(),
                "-h".into()
            ]),
            Some(HelpRequest::SubcommandHelp("index".into()))
        );
    }

    #[test]
    fn help_flag_after_query_text_is_not_intercepted() {
        // `search foo --help`：--help 不在命令名的紧跟位，是查询文本（R3.1/R9.4）。
        assert_eq!(
            intercept_help_or_version(&["search".into(), "foo".into(), "--help".into()]),
            None
        );
        // 未知命令后的 --help 不拦截，留给 dispatch 报 unknown subcommand。
        assert_eq!(
            intercept_help_or_version(&["bogus".into(), "--help".into()]),
            None
        );
        // 无帮助旗标的普通命令也不拦截。
        assert_eq!(intercept_help_or_version(&["status".into()]), None);
        assert_eq!(
            intercept_help_or_version(&["index".into(), "rebuild".into()]),
            None
        );
    }

    #[test]
    fn every_known_subcommand_has_help_text() {
        for cmd in KNOWN_COMMANDS {
            let text = subcommand_help_text(cmd);
            assert!(text.contains(cmd), "{cmd}: {text}");
            assert!(!text.is_empty(), "{cmd}");
        }
        // 顶层帮助列出 index rebuild，且能到达（R3.1 可达性）。
        assert!(help_text().contains("index rebuild"));
    }

    #[test]
    fn machine_mode_help_is_a_success_envelope() {
        let envelope = help_envelope(
            "search",
            serde_json::json!({ "help_text": "search <query>..." }),
            Some("req-9"),
        );
        let v: serde_json::Value = serde_json::from_str(&envelope).expect("valid JSON");
        assert_eq!(v["frame_type"], "response");
        assert_eq!(v["command"], "search");
        assert_eq!(v["ok"], true);
        assert_eq!(v["request_id"], "req-9");
        assert_eq!(v["data"]["help_text"], "search <query>...");

        let version = help_envelope(
            "version",
            serde_json::json!({ "version": "agent-session-grep 0.1.0" }),
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&version).expect("valid JSON");
        assert_eq!(v["command"], "version");
        assert_eq!(v["data"]["version"], "agent-session-grep 0.1.0");
    }

    #[test]
    fn subcommand_help_works_without_db() {
        // run() 在拦截阶段就返回，不进入 parse_db_flag / 存储打开（R3.1：help
        // 无需 --db，且不创建任何文件）。
        assert!(run(&["--help".into()], protocol::OutputMode::Human, None, false).is_ok());
        assert!(
            run(
                &["--version".into()],
                protocol::OutputMode::Human,
                None,
                false
            )
            .is_ok()
        );
        assert!(
            run(
                &["search".into(), "--help".into()],
                protocol::OutputMode::Human,
                None,
                false
            )
            .is_ok()
        );
        assert!(
            run(
                &["index".into(), "rebuild".into(), "--help".into()],
                protocol::OutputMode::Human,
                None,
                false
            )
            .is_ok()
        );
        // 机器模式同样成功（exit 0），不发裸文本。
        assert!(
            run(
                &["--robot".into(), "--help".into()],
                protocol::OutputMode::Json,
                None,
                false
            )
            .is_ok()
        );
    }

    // ---- 解析健壮性（R8）----

    #[test]
    fn db_flag_rejects_flag_named_value() {
        // `--db --robot status` 曾造出名为 `--robot` 的文件；取值是已知 flag
        // 一律用法错误（R8.1），任何取值都不落入文件系统。
        for value in [
            "--robot",
            "--output",
            "--request-id",
            "--help",
            "--version",
            "--db",
            "-h",
            "-V",
            "--cursor",
            "--max-items",
            "--max-bytes",
            "--max-messages",
            "--policy",
            "--level",
            "--provider",
            "--since",
            "--until",
            "--session",
            "--around",
            "--snapshot-json",
            "--offline",
        ] {
            let error = parse_db_flag(&["--db".into(), value.into(), "status".into()])
                .expect_err("flag-named value must be rejected");
            assert_eq!(error.0.code, CanonicalCode::InvalidRequest, "{value}");
        }
        // 缺值同样是用法错误。
        let error = parse_db_flag(&["--db".into()]).expect_err("missing value rejected");
        assert_eq!(error.0.code, CanonicalCode::InvalidRequest);
    }

    #[test]
    fn db_flag_rejects_duplicate() {
        // `--db a --db b status` 曾静默 last-wins；重复 --db 是用法错误（R8.2）。
        let error = parse_db_flag(&[
            "--db".into(),
            "a.db".into(),
            "--db".into(),
            "b.db".into(),
            "status".into(),
        ])
        .expect_err("duplicate --db rejected");
        assert_eq!(error.0.code, CanonicalCode::InvalidRequest);
        assert!(error.0.message.contains("duplicate"));
    }

    #[test]
    fn request_id_rejects_flag_value_and_duplicate() {
        // `--request-id --robot` 的 --robot 能通过 id 字符集校验，曾静默当作合法
        // id；取值是已知 flag 与重复 --request-id 都是用法错误（R8.1/R8.2）。
        assert!(
            extract_request_id(&["--request-id".into(), "--robot".into(), "status".into()])
                .is_err()
        );
        assert!(
            extract_request_id(&[
                "--request-id".into(),
                "a".into(),
                "--request-id".into(),
                "b".into(),
                "status".into()
            ])
            .is_err()
        );
        assert!(extract_request_id(&["--request-id".into()]).is_err());
        // 合法取值不受影响。
        assert_eq!(
            extract_request_id(&["--request-id".into(), "corr.1".into(), "status".into()])
                .expect("valid id"),
            Some("corr.1".to_string())
        );
    }

    #[test]
    fn doctor_and_config_reject_unknown_positionals() {
        // `--robot doctor --bogus` 曾静默丢弃 --bogus 后 exit 0（R8.3）。
        assert!(
            doctor(
                &["doctor".into(), "--bogus".into()],
                protocol::OutputMode::Human,
                None,
                false
            )
            .is_err()
        );
        // doctor 的 --db 同样受取值守卫保护：flag 当取值 / 重复 / 缺值都拒绝。
        assert!(
            doctor(
                &["doctor".into(), "--db".into(), "--robot".into()],
                protocol::OutputMode::Human,
                None,
                false
            )
            .is_err()
        );
        assert!(
            doctor(
                &[
                    "doctor".into(),
                    "--db".into(),
                    "a.db".into(),
                    "--db".into(),
                    "b.db".into()
                ],
                protocol::OutputMode::Human,
                None,
                false
            )
            .is_err()
        );
        assert!(
            doctor(
                &["doctor".into(), "--db".into()],
                protocol::OutputMode::Human,
                None,
                false
            )
            .is_err()
        );
        // `--robot config paths --bogus` 曾 exit 0。
        assert!(
            run(
                &["config".into(), "paths".into(), "--bogus".into()],
                protocol::OutputMode::Human,
                None,
                false
            )
            .is_err()
        );
        assert!(
            run(
                &[
                    "--robot".into(),
                    "config".into(),
                    "paths".into(),
                    "--bogus".into()
                ],
                protocol::OutputMode::Json,
                None,
                false
            )
            .is_err()
        );
    }

    #[test]
    fn command_name_points_at_failing_token() {
        // 未知 '-' 开头 token 不是 flag——它是命令名笔误，错误 envelope 的
        // command 必须指向它，而不是后面的真命令（R8.4）。
        assert_eq!(
            command_name(&["--bogus".into(), "--robot".into(), "status".into()]),
            "--bogus"
        );
        assert_eq!(
            command_name(&[
                "--db".into(),
                "s.db".into(),
                "--bogus".into(),
                "status".into()
            ]),
            "--bogus"
        );
        // 已知 flag 与其取值跳过，命令名正常识别。
        assert_eq!(command_name(&["--robot".into(), "status".into()]), "status");
        assert_eq!(
            command_name(&[
                "--db".into(),
                "s.db".into(),
                "--output".into(),
                "json".into(),
                "search".into()
            ]),
            "search"
        );
        assert_eq!(
            command_name(&[
                "--request-id".into(),
                "r1".into(),
                "sync".into(),
                "a.jsonl".into()
            ]),
            "sync"
        );
        assert_eq!(
            command_name(&["--robot".into(), "--help".into()]),
            "unknown"
        );
    }

    // ---- `--offline` 全局 flag（design D5：零出网是硬要求）----

    #[test]
    fn extract_offline_flag_only_reads_prefix_position() {
        // 前缀位置（命令名之前）识别；命令名之后同名 token 是位置参数。
        assert!(extract_offline_flag(&["--offline".into(), "status".into()]));
        assert!(extract_offline_flag(&[
            "--db".into(),
            "s.db".into(),
            "--offline".into(),
            "status".into()
        ]));
        assert!(extract_offline_flag(&["--offline".into()]));
        assert!(!extract_offline_flag(&["status".into()]));
        assert!(!extract_offline_flag(&[
            "status".into(),
            "--offline".into()
        ]));
        // 带值 flag 的取值跳过，不会把取值误当命令名。
        assert!(extract_offline_flag(&[
            "--db".into(),
            "s.db".into(),
            "--offline".into(),
            "search".into()
        ]));
        assert!(!extract_offline_flag(&["--db".into(), "--offline".into()]));
    }

    #[test]
    fn offline_is_registered_in_all_prefix_scanners() {
        // command_name 跳过 --offline，命令名正确识别。
        assert_eq!(
            command_name(&["--offline".into(), "status".into()]),
            "status"
        );
        // is_known_flag_name 覆盖 --offline：`--db --offline` 不得把 --offline 当路径。
        assert!(is_known_flag_name("--offline"));
        assert!(parse_db_flag(&["--db".into(), "--offline".into(), "status".into()]).is_err());
        // bare_positionals 跳过 --offline：doctor 的多余参数校验不受影响。
        assert_eq!(
            bare_positionals(&["--offline".into(), "doctor".into()]),
            vec!["doctor".to_string()]
        );
        // parse_db_flag 跳过 --offline：命令名之后是 rest，不吞命令。
        let (db, rest) = parse_db_flag(&[
            "--db".into(),
            "s.db".into(),
            "--offline".into(),
            "search".into(),
        ])
        .expect("offline must not break parse_db_flag");
        assert_eq!(db.as_deref(), Some("s.db"));
        assert_eq!(rest, vec!["search".to_string()]);
        // help 拦截：--offline --help 仍是顶层帮助。
        assert_eq!(
            intercept_help_or_version(&["--offline".into(), "--help".into()]),
            Some(HelpRequest::TopLevelHelp)
        );
    }

    #[test]
    fn offline_gate_refuses_network_capabilities_and_passes_local_ones() {
        // 未来联网命令在 offline 下必须拒绝，且 code 稳定为 capability_not_supported。
        let error = offline_capability_gate(true, "model-download")
            .expect_err("offline must refuse network capability");
        assert_eq!(error.0.code, CanonicalCode::CapabilityNotSupported);
        assert_eq!(error.0.code.exit_code(), 7);
        assert!(!error.0.code.retryable());
        assert!(error.0.message.contains("model-download"));
        // 非 offline 时正常放行；任何命令在非 offline 下都不该被 gate 拦。
        assert!(offline_capability_gate(false, "model-download").is_ok());
        assert!(offline_capability_gate(false, "telemetry").is_ok());
    }

    #[test]
    fn offline_combines_with_sync_and_search_dispatch() {
        // offline + 既有本地命令（sync/search）必须正常：flag 不改动既有行为。
        let store = SqliteStore::open_in_memory().expect("in-memory store opens");
        let (command, outcome, data, _, _) = dispatch(
            &store,
            "test.db",
            DbOrigin::Flag,
            &["search".into(), "foo".into()],
            protocol::OutputMode::Json,
            None,
            true,
        )
        .expect("offline search must succeed");
        assert_eq!(command, "search");
        assert_eq!(outcome, protocol::Outcome::Success);
        assert_eq!(data["hits"].as_array().expect("hits").len(), 0);
        // sync 目录拒绝在 offline 下同样走 usage error（不吞错误）。
        let dir = std::env::temp_dir().to_string_lossy().into_owned();
        let error = dispatch(
            &store,
            "test.db",
            DbOrigin::Flag,
            &["sync".into(), dir.clone()],
            protocol::OutputMode::Json,
            None,
            true,
        )
        .expect_err("offline sync directory must still be rejected");
        assert_eq!(error.0.code, CanonicalCode::InvalidRequest);
        assert!(!error.0.message.contains(&dir), "{:?}", error.0.message);
    }

    #[test]
    fn doctor_accepts_offline_without_error() {
        // doctor 在 --offline 下照常自检（offline 是稳定显式模式，不新增错误路径）；
        // offline 字段由 emit_result 输出，unit 层只验证成功与诊断字段存在性。
        assert!(doctor(&["doctor".into()], protocol::OutputMode::Human, None, true).is_ok());
        // 与不带 --db 的默认 doctor 等价，offline 不改变退出语义。
        assert!(
            doctor(
                &["--offline".into(), "doctor".into()],
                protocol::OutputMode::Human,
                None,
                true
            )
            .is_ok()
        );
    }

    #[test]
    fn hook_search_filters_maps_providers_and_decay() {
        // 空配置：不过滤（EMPTY filters）。
        let empty = hooks::HookConfig::default();
        let filters = hook_search_filters(&empty, 1_000_000).expect("empty config is valid");
        assert!(filters.providers.is_empty());
        assert!(filters.since.is_none());
        assert!(filters.until.is_none());

        // provider 白名单：claude/codex 别名映射到 SearchProvider。
        let providers = hooks::HookConfig {
            providers: vec!["claude".into(), "codex".into()],
            ..Default::default()
        };
        let filters = hook_search_filters(&providers, 0).expect("providers valid");
        assert_eq!(
            filters.providers,
            vec![SearchProvider::claude_code(), SearchProvider::codex()]
        );
        assert!(filters.since.is_none());

        // The hook consumes the same provider values as search. Canonical ids
        // emitted by machine surfaces must resolve exactly like legacy aliases.
        let canonical = hooks::HookConfig {
            providers: vec!["claude-code".into()],
            ..Default::default()
        };
        let filters = hook_search_filters(&canonical, 0).expect("canonical id valid");
        assert_eq!(filters.providers, vec![SearchProvider::claude_code()]);

        // 未知 provider 是用法错误，不静默忽略（fail-closed）；错误信息与
        // search --provider 同风格回显取值（provider 值不是路径/secret）。
        let bad = hooks::HookConfig {
            providers: vec!["nope".into()],
            ..Default::default()
        };
        let error = hook_search_filters(&bad, 0).expect_err("unknown provider rejected");
        assert_eq!(error.0.code, CanonicalCode::InvalidRequest);
        assert!(error.0.message.contains("nope"), "{:?}", error.0.message);

        // 时间衰减：decay_days = 7 → since ≈ now - 7 天。
        let decay = hooks::HookConfig {
            decay_days: 7,
            ..Default::default()
        };
        let now_ms = 1_000_000_000i64;
        let filters = hook_search_filters(&decay, now_ms).expect("decay valid");
        let since_ms = filters.since.expect("decay sets since").unix_seconds * 1_000;
        assert_eq!(since_ms, now_ms - 7 * 86_400_000);
    }

    // ---- 隐私（R2）----

    #[test]
    fn not_found_error_never_echoes_the_wire_id() {
        let store = SqliteStore::open_in_memory().expect("in-memory store opens");
        let wire = "msg_v1_private-wire-id";
        for cmd in ["get", "show"] {
            let error = dispatch(
                &store,
                "test.db",
                DbOrigin::Flag,
                &[cmd.into(), wire.into()],
                protocol::OutputMode::Human,
                None,
                false,
            )
            .expect_err("missing entity must fail");
            // exit 4 契约（ADR-0005）不变，只改消息。
            assert_eq!(error.0.code, CanonicalCode::NotFound, "{cmd}");
            assert_eq!(error.0.message, "entity not found", "{cmd}");
            assert!(
                !error.0.message.contains(wire),
                "{cmd} message must not echo the wire id"
            );
        }
    }

    #[test]
    fn sync_directory_rejection_is_path_free_and_platform_neutral() {
        let store = SqliteStore::open_in_memory().expect("in-memory store opens");
        let dir = std::env::temp_dir();
        let dir_str = dir.to_string_lossy().into_owned();
        let error = dispatch(
            &store,
            "test.db",
            DbOrigin::Flag,
            &["sync".into(), dir_str.clone()],
            protocol::OutputMode::Json,
            None,
            false,
        )
        .expect_err("directory must be rejected");
        assert_eq!(error.0.code, CanonicalCode::InvalidRequest);
        let message = &error.0.message;
        assert!(!message.contains(&dir_str), "path must not leak: {message}");
        assert!(
            !message.contains("PowerShell"),
            "platform-neutral: {message}"
        );
        assert!(
            !message.contains("Get-ChildItem"),
            "platform-neutral: {message}"
        );
    }

    #[test]
    fn sync_discover_rejects_extra_flags() {
        let store = SqliteStore::open_in_memory().expect("in-memory store opens");
        let error = dispatch(
            &store,
            "test.db",
            DbOrigin::Flag,
            &["sync".into(), "--discover".into(), "--bogus".into()],
            protocol::OutputMode::Json,
            None,
            false,
        )
        .expect_err("discover must reject unknown extra flags");
        assert_eq!(error.0.code, CanonicalCode::InvalidRequest);
    }

    #[test]
    fn take_bool_flag_removes_and_reports_presence() {
        // R2/R3 布尔旗标：在场移除 token 并返回 true，缺场返回 false；不消费取值。
        let mut args = vec![
            "foo".into(),
            "--include-system".into(),
            "--group-by-session".into(),
        ];
        assert!(take_bool_flag(&mut args, "--include-system"));
        assert!(take_bool_flag(&mut args, "--group-by-session"));
        assert!(!take_bool_flag(&mut args, "--include-system"));
        assert_eq!(args, vec![String::from("foo")]);
    }

    #[test]
    fn search_dispatch_accepts_r2r3_flags_on_empty_store() {
        // 空库上 search 返回零命中但不应把 R2/R3 旗标当多余参数拒绝（R2/R3 是
        // 合法命令级 flag）。缺实现时 --include-system 会触发 usage error。
        let store = SqliteStore::open_in_memory().expect("in-memory store opens");
        let (command, outcome, data, _, _) = dispatch(
            &store,
            "test.db",
            DbOrigin::Flag,
            &[
                "search".into(),
                "foo".into(),
                "--include-system".into(),
                "--group-by-session".into(),
            ],
            protocol::OutputMode::Json,
            None,
            false,
        )
        .expect("search with r2r3 flags must succeed");
        assert_eq!(command, "search");
        assert_eq!(outcome, protocol::Outcome::Success);
        assert_eq!(data["hits"].as_array().expect("hits").len(), 0);
    }

    #[test]
    fn render_search_emits_session_context_fields() {
        // SearchHit.session_id/text 由 application 装配（批量 session_of +
        // 批量取 payload 截取摘要）；render 原样投影——Some → 字符串，
        // None → null（不臆造会话/摘要；human 渲染器不补行）。
        let response = AppResponse::Search {
            hits: vec![
                agent_session_grep_ports::SearchHit {
                    id: StableId::from_wire("msg_v1_aaaa").expect("valid id"),
                    score: 2.0,
                    session_id: Some("ses_v1_aaaa".into()),
                    text: Some("正文预览".into()),
                    why_matched: Vec::new(),
                    suggested_next_commands: Vec::new(),
                    occurrences: 1,
                    resume_available: false,
                },
                agent_session_grep_ports::SearchHit {
                    id: StableId::from_wire("msg_v1_bbbb").expect("valid id"),
                    score: 1.0,
                    session_id: None,
                    text: None,
                    why_matched: Vec::new(),
                    suggested_next_commands: Vec::new(),
                    occurrences: 1,
                    resume_available: false,
                },
            ],
            next_cursor: None,
            generation: 3,
            truncation: Truncation {
                truncated: false,
                reason: None,
            },
            retrieval_mode: RetrievalMode::Lexical,
            fallback_warning: None,
            time_filter_excluded: 0,
        };
        let (_, data, _, _) = render(response);
        assert_eq!(data["hits"][0]["session_id"], "ses_v1_aaaa");
        assert_eq!(data["hits"][0]["text"], "正文预览");
        assert_eq!(data["hits"][1]["session_id"], serde_json::Value::Null);
        assert_eq!(data["hits"][1]["text"], serde_json::Value::Null);
        assert!(
            data["hits"][0].get("why_matched").is_none(),
            "empty collections must be omitted: {}",
            data["hits"][0]
        );
        assert!(
            data["hits"][0].get("suggested_next_commands").is_none(),
            "empty collections must be omitted: {}",
            data["hits"][0]
        );
    }

    #[test]
    fn render_search_emits_guidance_when_present() {
        let response = AppResponse::Search {
            hits: vec![agent_session_grep_ports::SearchHit {
                id: StableId::from_wire("msg_v1_aaaa").expect("valid id"),
                score: 2.0,
                session_id: Some("ses_v1_aaaa".into()),
                text: Some("preview".into()),
                why_matched: vec!["needle".into()],
                suggested_next_commands: vec![
                    "agent-session-grep get-message msg_v1_aaaa --session ses_v1_aaaa --around 2"
                        .into(),
                    "agent-session-grep context ses_v1_aaaa".into(),
                ],
                occurrences: 1,
                resume_available: false,
            }],
            next_cursor: None,
            generation: 3,
            truncation: Truncation {
                truncated: false,
                reason: None,
            },
            retrieval_mode: RetrievalMode::Lexical,
            fallback_warning: None,
            time_filter_excluded: 0,
        };
        let (_, data, _, _) = render(response);
        let hit = &data["hits"][0];
        assert_eq!(hit["why_matched"], serde_json::json!(["needle"]));
        assert_eq!(
            hit["suggested_next_commands"],
            serde_json::json!([
                "agent-session-grep get-message msg_v1_aaaa --session ses_v1_aaaa --around 2",
                "agent-session-grep context ses_v1_aaaa"
            ])
        );
    }

    #[test]
    fn machine_render_never_emits_snippet_field() {
        // SearchHit 已无 snippet 字段（ADR-0008 由 text 承接摘要）：render 的
        // 命中 JSON 只携带 {id, score, session_id, text}，任何输出模式下都
        // 不出现 snippet 键（机器模式协议兼容约束由构造层保证，不再需要剥除）。
        let response = AppResponse::Search {
            hits: vec![agent_session_grep_ports::SearchHit {
                id: StableId::from_wire("msg_v1_aaaa").expect("valid id"),
                score: 2.0,
                session_id: Some("ses_v1_aaaa".into()),
                text: Some("preview".into()),
                why_matched: Vec::new(),
                suggested_next_commands: Vec::new(),
                occurrences: 1,
                resume_available: false,
            }],
            next_cursor: None,
            generation: 3,
            truncation: Truncation {
                truncated: false,
                reason: None,
            },
            retrieval_mode: RetrievalMode::Lexical,
            fallback_warning: None,
            time_filter_excluded: 0,
        };
        let (_, data, _, _) = render(response);
        let hit = &data["hits"][0];
        assert!(hit.get("snippet").is_none(), "{hit}");
        assert_eq!(hit["id"], "msg_v1_aaaa");
        assert!(hit["score"].is_number(), "{hit}");
        assert_eq!(hit["session_id"], "ses_v1_aaaa");
        assert_eq!(hit["text"], "preview");
    }

    #[test]
    fn render_message_window_projects_typed_placement_fields() {
        let response = AppResponse::Message {
            window: agent_session_grep_application::MessageWindow {
                message_id: "msg_v1_anchor".into(),
                session_id: "ses_v1_s".into(),
                anchor_placement_id: "plc_v1_anchor".into(),
                messages: vec![agent_session_grep_application::ContextMessage {
                    id: "msg_v1_anchor".into(),
                    placement_id: "plc_v1_anchor".into(),
                    message_id: "msg_v1_anchor".into(),
                    payload: serde_json::json!({"role": "user", "text": "hi"}),
                }],
                truncation: Truncation {
                    truncated: false,
                    reason: None,
                },
                generation: 5,
            },
        };
        let (outcome, data, page, warnings) = render(response);
        assert!(matches!(outcome, protocol::Outcome::Success));
        assert!(warnings.is_empty());
        assert!(!page.has_more);
        assert_eq!(data["message_id"], "msg_v1_anchor");
        assert_eq!(data["session_id"], "ses_v1_s");
        assert_eq!(data["anchor_placement_id"], "plc_v1_anchor");
        assert_eq!(data["generation"], 5);
        let messages = data["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["placement_id"], "plc_v1_anchor");
        assert_eq!(messages[0]["payload"]["text"], "hi");
    }
}
