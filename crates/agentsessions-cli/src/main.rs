//! AgentSessions CLI：组合根（composition root）。
//!
//! 本 crate 是唯一把抽象端口与具体 adapter 绑定的地方——它 `new` 出 [`SqliteStore`]
//! 并注入 [`App`]，其余各层对具体后端一无所知；分层依赖保持
//! domain ← ports ← application ← adapters。
//! 首个垂直切片只暴露两个子命令，用于端到端打通 discovery→catalog→search 骨架：
//!
//! ```text
//! agentsessions --db <path> index <id-fact> <text>   # 写入 catalog 并索引
//! agentsessions --db <path> search <query>           # 全文检索
//! agentsessions --db <path> get <wire-id>            # 按 id 取回 payload
//! ```
//!
//! 参数解析刻意手写、不引第三方 CLI 框架——切片阶段只需最小可用面。
//! 输出走 Robot-JSON 雏形（每行一个 JSON 对象），为后续 CONTRACT 对齐留口。

mod protocol;

use agentsessions_adapters_sqlite::{SourceBatch, SqliteStore, capture, verify_snapshot};
use agentsessions_application::{
    App, AppError, AppRequest, AppResponse, ResponseBudget, StagedBatch, Truncation,
    select_and_stage,
};
use agentsessions_domain::{ContextPolicy, DomainError, IdKind, Stability, StableId};
use agentsessions_ports::ProviderAdapter;
use agentsessions_provider_claude::ClaudeCodeAdapter;
use agentsessions_provider_codex::CodexAdapter;
use protocol::{CanonicalCode, ProtocolError};

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
            println!("{}", protocol::error_envelope(&command, &err));
            std::process::exit(err.code.exit_code());
        }
    };
    match run(&args, mode) {
        Ok(protocol::Outcome::Success) => {}
        // 部分成功（预算截断）按 contract §5 exit 10——结果可用但不完整，不伪装 success。
        Ok(protocol::Outcome::Partial) => std::process::exit(10),
        Err(CliError(err)) => {
            // 错误 envelope 只写 stdout 一个对象；进程级诊断（人类模式）走 stderr。
            match mode {
                protocol::OutputMode::Human => {
                    eprintln!("error [{}]: {}", err.code.as_str(), err.message);
                }
                protocol::OutputMode::Json | protocol::OutputMode::Jsonl => {
                    println!("{}", protocol::error_envelope(&command, &err));
                }
            }
            std::process::exit(err.code.exit_code());
        }
    }
}

/// 从参数里解出子命令名（用于错误 envelope 的 `command` 字段）。
///
/// 跳过带值 flag（`--db <path>` / `--output <mode>` / 分页与预算 flag）及其取值，
/// 第一个裸参数即命令。
fn command_name(args: &[String]) -> String {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" | "--output" | "--cursor" | "--max-items" | "--max-bytes" | "--max-messages"
            | "--policy" => {
                it.next(); // 消费其取值
            }
            s if s.starts_with('-') => {}
            s => return s.to_string(),
        }
    }
    "unknown".into()
}

fn run(args: &[String], mode: protocol::OutputMode) -> Result<protocol::Outcome, CliError> {
    let started = std::time::Instant::now();
    // --help / --version / doctor 在解析 --db 之前拦截——它们不需要数据库。
    // 放在最前面，使 `agentsessions --help`（无 --db）也能正常工作。
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(protocol::Outcome::Success);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(protocol::Outcome::Success);
    }
    if command_name(args) == "doctor" {
        return doctor(args, mode);
    }
    if command_name(args) == "config" {
        if args.iter().any(|arg| arg == "paths") {
            return config_paths(mode);
        }
        return Err(CliError::usage(
            "config paths is the only supported config command",
        ));
    }

    let (db, rest) = parse_db_flag(args)?;
    // 写入子命令抢 data-root writer lease；读路径不抢，允许多读者并发。
    let writes = rest
        .first()
        .map(|c| matches!(c.as_str(), "index" | "ingest" | "sync"))
        .unwrap_or(false);
    let store = if writes {
        SqliteStore::open_for_write(&db)
    } else {
        SqliteStore::open(&db)
    }
    .map_err(ProtocolError::from)?;
    // catalog 与 index 是同一个 SqliteStore；App 泛型接受同一实例的两次移动，
    // 故这里克隆一个连接语义上的第二把手不可行——改为让 App 持有单一 store。
    let (command, outcome, data, page) = dispatch(&store, &rest)?;
    let duration_ms = started.elapsed().as_millis() as u64;
    protocol::emit(command, mode, outcome, data, duration_ms, &page);
    Ok(outcome)
}

fn config_paths(mode: protocol::OutputMode) -> Result<protocol::Outcome, CliError> {
    let paths = platform_paths()?;
    protocol::emit(
        "config.paths",
        mode,
        protocol::Outcome::Success,
        paths,
        0,
        &protocol::Page::default(),
    );
    Ok(protocol::Outcome::Success)
}

fn platform_paths() -> Result<serde_json::Value, CliError> {
    platform_paths_impl()
}

#[cfg(windows)]
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

fn print_help() {
    println!(
        "{name} {version}
AI coding-agent history search engine.

USAGE:
    agentsessions --db <path> <COMMAND> [ARGS]
    agentsessions doctor [--db <path>]
    agentsessions --help | --version

COMMANDS:
    ingest <file>          解析原始 .jsonl 文件并入库（只读源）
    sync <file>...          原子扫描多个 .jsonl 文件；无变化时不生成新 generation
    index <id-fact> <text> 直接写入一条 catalog + 索引（切片期写入入口）
    index rebuild          从权威 catalog 全量重投影 FTS 索引（维护命令）
    search <query>         全文检索，按相关性降序返回命中（支持分页/预算 flag）
    get <wire-id>          按实体 id 取回原始 payload
    show <wire-id>         按实体 id 取回并归一化展示（role/text 结构）
    list [limit]           稳定排序列出 catalog 实体（默认 20；支持分页/预算 flag）
    context <ses-id>       装配会话上下文：分支消息链 + 证据区间
    status                 报告 catalog 实体总数
    doctor                 环境自检（可选 --db 校验存储可打开）
    config paths           报告当前平台的 config/data/cache/logs 路径

PAGINATION / BUDGET (search, list):
    --cursor <token>       上一页 envelope `page.next_cursor` 的续读令牌
    --max-items <n>        页大小上限（同时作为响应条目预算）
    --max-bytes <n>        响应字节预算（最低 4096）

CONTEXT:
    --policy mainline|full 分支策略（默认 mainline：排除 sidechain 沿 parent 链）
    --max-messages <n>     消息条数预算
    --max-bytes <n>        响应字节预算

GLOBAL:
    --db <path>            SQLite 数据存储路径（除 doctor/help/version 外必需）
    --output human|json|jsonl  输出模式（默认 human；切片期 human 仍为 JSON envelope）
    --robot                等价 --output json，无颜色/进度（stdout 只输出协议）
    -h, --help             打印本帮助
    -V, --version          打印版本

EXIT CODES:
    0 成功；10 部分成功（预算截断，结果可用但不完整）；其余见 error catalog",
        name = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
    );
}

/// doctor：最小环境自检。报告版本；若给了 --db，尝试打开存储并报告 schema。
fn doctor(args: &[String], mode: protocol::OutputMode) -> Result<protocol::Outcome, CliError> {
    let started = std::time::Instant::now();
    let db_opt = args
        .iter()
        .position(|a| a == "--db")
        .and_then(|i| args.get(i + 1));
    let data = match db_opt {
        None => serde_json::json!({
            "tool": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "db": "not-checked",
            "schema": null,
        }),
        Some(path) => {
            let store = SqliteStore::open(path).map_err(ProtocolError::from)?;
            let schema = store.schema_version().map_err(ProtocolError::from)?;
            // generation 与待收敛 intent 数是 durable outbox 中断恢复与一致性的只读证据。
            let generation = store.active_generation().map_err(ProtocolError::from)?;
            let interrupted = store
                .interrupted_batch_count()
                .map_err(ProtocolError::from)?;
            serde_json::json!({
                "tool": env!("CARGO_PKG_NAME"),
                "version": env!("CARGO_PKG_VERSION"),
                "db": "ok",
                "schema": schema,
                "generation": generation,
                "interrupted_batches": interrupted,
            })
        }
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    protocol::emit(
        "doctor",
        mode,
        protocol::Outcome::Success,
        data,
        duration_ms,
        &protocol::Page::default(),
    );
    Ok(protocol::Outcome::Success)
}

/// 从 `--db <path>` 抽出数据库路径，返回其余参数。
fn parse_db_flag(args: &[String]) -> Result<(String, Vec<String>), CliError> {
    let mut db = None;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" => {
                let v = it
                    .next()
                    .ok_or_else(|| CliError::usage("--db requires a path"))?;
                db = Some(v.clone());
            }
            // 输出模式 flag 在 main 里已消费；这里跳过，避免落入 rest 被当子命令。
            "--output" => {
                it.next();
            }
            "--robot" | "--no-color" | "--help" | "-h" | "--version" | "-V" => {}
            other => rest.push(other.to_string()),
        }
    }
    let db = db.ok_or_else(|| CliError::usage("--db <path> is required"))?;
    Ok((db, rest))
}

/// 分发子命令。`store` 同时充当 CatalogStore 与 SearchIndex（同一 SqliteStore）。
fn dispatch(
    store: &SqliteStore,
    rest: &[String],
) -> Result<
    (
        &'static str,
        protocol::Outcome,
        serde_json::Value,
        protocol::Page,
    ),
    CliError,
> {
    let cmd = rest
        .first()
        .ok_or_else(|| CliError::usage("missing subcommand"))?
        .as_str();
    match cmd {
        // index 是切片期的写入入口：派生一个 Reconstructed 消息 id，写 catalog + 索引。
        // 它不经 Application（Application 首片只暴露读用例），直接用端口写入。
        // `index rebuild` 是维护子命令：从权威 catalog 全量重投影 FTS 索引。
        "index" => {
            if rest.get(1).map(String::as_str) == Some("rebuild") {
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
                ))
            } else {
                let fact = arg(rest, 1, "index <id-fact> <text>")?;
                let text = arg(rest, 2, "index <id-fact> <text>")?;
                Ok((
                    "index",
                    protocol::Outcome::Success,
                    index_one(store, fact, text)?,
                    protocol::Page::default(),
                ))
            }
        }
        // ingest 打通 ingestion→storage→search 全链路：读取原始 .jsonl →
        // provider.probe 判定 variant → provider.parse 流式产出 Canonical 消息 →
        // 每条写 catalog + FTS 索引。文件严格只读（RFC-0002 §7）。
        "ingest" => {
            let path = arg(rest, 1, "ingest <file>")?;
            Ok((
                "ingest",
                protocol::Outcome::Success,
                ingest_file(store, path)?,
                protocol::Page::default(),
            ))
        }
        // sync 对显式列出的多个源执行同一套只读快照 + staging，并在全部成功后
        // 通过一次 durable batch 提交，避免部分 source 已写入、后续 source 失败。
        "sync" => Ok((
            "sync",
            protocol::Outcome::Success,
            sync_files(store, &rest[1..])?,
            protocol::Page::default(),
        )),
        "search" => {
            let mut args = rest.to_vec();
            let cursor = extract_flag(&mut args, "--cursor")?;
            let max_items = extract_flag(&mut args, "--max-items")?;
            let max_bytes = extract_flag(&mut args, "--max-bytes")?;
            let budget = budget_from_flags(max_items.as_deref(), max_bytes.as_deref(), None)?;
            // 页大小旋钮即 --max-items；未给时保守默认 20（App 内仍与 budget 取小）。
            let limit = if max_items.is_some() {
                budget.max_items
            } else {
                20
            };
            let query = arg(&args, 1, "search <query>")?.to_string();
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Search {
                query,
                limit,
                cursor,
                budget,
            })?;
            let (outcome, data, page) = render(response);
            Ok(("search", outcome, data, page))
        }
        "get" => {
            let wire = arg(rest, 1, "get <wire-id>")?;
            let id = StableId::from_wire(wire)
                .ok_or_else(|| CliError::usage(format!("not a valid entity id: {wire}")))?;
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Get { id })?;
            let (outcome, data, page) = render(response);
            Ok(("get", outcome, data, page))
        }
        "show" => {
            let wire = arg(rest, 1, "show <wire-id>")?;
            let id = StableId::from_wire(wire)
                .ok_or_else(|| CliError::usage(format!("not a valid entity id: {wire}")))?;
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Show { id })?;
            let (outcome, data, page) = render(response);
            Ok(("show", outcome, data, page))
        }
        "list" => {
            let mut args = rest.to_vec();
            let cursor = extract_flag(&mut args, "--cursor")?;
            let max_items = extract_flag(&mut args, "--max-items")?;
            let max_bytes = extract_flag(&mut args, "--max-bytes")?;
            let budget = budget_from_flags(max_items.as_deref(), max_bytes.as_deref(), None)?;
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
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::List {
                limit,
                cursor,
                budget,
            })?;
            let (outcome, data, page) = render(response);
            Ok(("list", outcome, data, page))
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
            let budget = budget_from_flags(None, max_bytes.as_deref(), max_messages.as_deref())?;
            let wire = arg(&args, 1, "context <session-wire-id>")?;
            let session_id = StableId::from_wire(wire)
                .ok_or_else(|| CliError::usage(format!("not a valid entity id: {wire}")))?;
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Context {
                session_id,
                policy,
                budget,
            })?;
            let (outcome, data, page) = render(response);
            Ok(("context", outcome, data, page))
        }
        "status" => {
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Status)?;
            let (outcome, data, page) = render(response);
            Ok(("status", outcome, data, page))
        }
        other => Err(CliError::usage(format!("unknown subcommand: {other}"))),
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
///
/// 由于 App<C,S> 按值持有两个后端，而我们只有一个 SqliteStore 实例，
/// 这里用 `&SqliteStore` 满足两个 trait 约束（trait 对 &T 亦实现）。
fn store_ref(store: &SqliteStore) -> &SqliteStore {
    store
}

/// 组合根持有的 provider adapter 清单。ingest/sync 用它 probe-select，
/// 由 [`select_and_stage`] 挑出认领此源的 adapter（见 RFC-0002 §3）。
/// 新增 provider 只需在此登记一行。
fn provider_registry() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(ClaudeCodeAdapter::new()),
        Box::new(CodexAdapter::new()),
    ]
}

/// 对一个源字节流 probe-select 并 stage，同时返回选中 adapter 判定的 variant。
///
/// `select_and_stage` 回完整 [`StagedBatch`]（消息 + provider 报告的会话 native id），
/// ingest/sync 输出要报 variant；这里复用同一份 registry 借用为 `&[&dyn ...]` 后交给
/// 编排层，选中的 variant 由事后对全体 adapter 再 probe 取最高置信度得到（与选择逻辑一致）。
fn stage_with_registry(bytes: &[u8]) -> Result<(StagedBatch, String), CliError> {
    let registry = provider_registry();
    let refs: Vec<&dyn ProviderAdapter> = registry.iter().map(|a| a.as_ref()).collect();
    let staged = select_and_stage(&refs, bytes)?;
    // 选中的 variant：取所有 adapter 中 probe 成功且置信度最高的那个 variant_id。
    let variant = refs
        .iter()
        .filter_map(|a| a.probe(bytes).ok())
        .filter(|p| !matches!(p.confidence, agentsessions_ports::Confidence::Ambiguous))
        .max_by_key(|p| match p.confidence {
            agentsessions_ports::Confidence::Confirmed => 3,
            agentsessions_ports::Confidence::High => 2,
            agentsessions_ports::Confidence::Low => 1,
            agentsessions_ports::Confidence::Ambiguous => 0,
        })
        .map(|p| p.variant_id)
        .unwrap_or_else(|| "unknown".into());
    Ok((staged, variant))
}

/// 把一个源的完整 staging 产物转成 (id, payload, index-text) 目录条目。
///
/// 产出三类实体，随同一 [`SourceBatch`] 单事务提交：
///
/// - **消息**：身份优先用 provider-native id（Claude Code 的 `uuid`，tier `Native`），
///   使身份跨 data-root 迁移与文件重命名存活；provider 未给 native id 时回退到
///   path+seq 的 `Reconstructed` 派生。payload 存 canonical JSON（role/text/parent/
///   timestamp/sidechain/span/session），供 `show` 展开；FTS 索引正文仍只喂纯 text。
/// - **会话**：id 优先取 provider 报告的 native 会话 id（`ses_v1_` Native tier），
///   缺失回退对 document wire id 的 `Reconstructed` 派生。payload 引用 document
///   与按 seq 排序的成员消息 wire id。
/// - **文档**：内容寻址 `Reconstructed` 派生（provider/variant/fingerprint），
///   不含路径——身份不编码位置（RFC-0001）。payload 携 provider/variant/fingerprint/len。
///
/// span 单位是"已验证快照字节"（end 排他）；`None` 显式缺失，不臆造。
fn staged_to_entries(
    path: &str,
    staged: &StagedBatch,
    provider_id: &str,
    variant: &str,
    fingerprint: &str,
    source_len: u64,
) -> Vec<(StableId, Vec<u8>, String)> {
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
    let session_id = match staged.session_native_id.as_deref() {
        Some(sid) if !sid.trim().is_empty() => StableId::native(IdKind::Session, sid),
        _ => StableId::derive(
            IdKind::Session,
            Stability::Reconstructed,
            &[document_id.as_str().as_bytes()],
        ),
    };

    let mut entries: Vec<(StableId, Vec<u8>, String)> = staged
        .messages
        .iter()
        .map(|message| {
            let id = if message.native_id.is_empty() {
                StableId::derive(
                    IdKind::Message,
                    Stability::Reconstructed,
                    &[path.as_bytes(), &message.seq.to_le_bytes()],
                )
            } else {
                StableId::native(IdKind::Message, &message.native_id)
            };
            let payload = serde_json::json!({
                "role": message.role,
                "text": message.text,
                "parent_native_id": message.parent_native_id,
                // 已解析父边：provider native 父指针按同一 native 派生规则映射为消息
                // wire id（跨文件父边同样可表示；父不在库中由消费方平滑容忍）。
                "parent": message
                    .parent_native_id
                    .as_deref()
                    .filter(|p| !p.trim().is_empty())
                    .map(|p| StableId::native(IdKind::Message, p).as_str().to_string()),
                "timestamp": message.timestamp,
                "is_sidechain": message.is_sidechain,
                "session": session_id.as_str(),
                "span": message.span.map(|(start, end)| serde_json::json!({
                    "start": start,
                    "end": end,
                })),
            })
            .to_string();
            (id, payload.into_bytes(), message.text.clone())
        })
        .collect();

    // 会话/文档目录行：payload 为结构化 JSON，索引正文为空——容器实体不参与全文命中
    // （存储层对非 Message kind 也不会写 fts 行，双保险）。
    let member_ids: Vec<&str> = entries.iter().map(|(id, _, _)| id.as_str()).collect();
    let session_payload = serde_json::json!({
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
    entries.push((session_id, session_payload.into_bytes(), String::new()));
    entries.push((document_id, document_payload.into_bytes(), String::new()));
    entries
}

/// 读取原始 .jsonl 文件并 ingest：
/// capture 快照 → stage（probe+缓冲 parse）→ verify 快照 → 单事务原子提交。
///
/// 落实：
/// - RFC-0002 §4 ReadOnlySourceSnapshot：len+mtime+fingerprint 复核；
/// - RFC-0002 §5 source-level staging：parse 只缓冲，成功后才 commit_batch。
///
/// 会话 fact 用文件路径，保证同文件重 ingest 得到稳定 id（幂等重索引）。
fn ingest_file(store: &SqliteStore, path: &str) -> Result<serde_json::Value, CliError> {
    let path_ref = std::path::Path::new(path);
    // 1) 捕获只读源快照 + 字节（严格只读打开源文件）。
    let (snap, bytes) = capture(path_ref).map_err(ProtocolError::from)?;

    // 2) probe-select + stage：从 registry 挑出认领此源的 provider；失败即弃，不写库。
    let (staged, variant) = stage_with_registry(&bytes)?;

    // 3) 提交前复核：源在 stage 期间被改写则拒绝提交（RFC-0002 §4）。
    verify_snapshot(path_ref, &snap).map_err(ProtocolError::from)?;

    // 4) 派生 id + 构造该源的完整 scan 结果（消息 + 会话/文档目录行），按
    //    source membership 提交。同文件重 ingest 时，本次消失的 id 会被推导为 tombstone。
    let provider = variant.split('/').next().unwrap_or(&variant).to_string();
    let source = SourceBatch {
        source_path: path.to_string(),
        entries: staged_to_entries(
            path,
            &staged,
            &provider,
            &variant,
            &snap.fingerprint,
            snap.len,
        ),
    };
    let changed = store
        .commit_source_batches_if_changed(std::slice::from_ref(&source))
        .map_err(ProtocolError::from)?;

    let generation = store.active_generation().map_err(ProtocolError::from)?;
    Ok(serde_json::json!({
        "variant": variant,
        "committed": if changed { staged.messages.len() } else { 0 },
        "unchanged": if changed { 0 } else { staged.messages.len() },
        "skipped": 0,
        "generation": generation,
        "source_fp": snap.fingerprint,
    }))
}

/// 同步显式给定的源文件：所有文件先完成 capture + stage + verify，之后才提交
/// 一个 durable batch。这样任一文件失败都不会留下其它文件的部分更新。
fn sync_files(store: &SqliteStore, paths: &[String]) -> Result<serde_json::Value, CliError> {
    if paths.is_empty() {
        return Err(CliError::usage("sync <file>... requires at least one file"));
    }
    let mut sources = Vec::with_capacity(paths.len());
    let mut snapshots = Vec::with_capacity(paths.len());
    let mut message_count = 0usize;

    for path in paths {
        let path_ref = std::path::Path::new(path);
        let (snap, bytes) = capture(path_ref).map_err(ProtocolError::from)?;
        let (staged, variant) = stage_with_registry(&bytes)?;
        message_count += staged.messages.len();
        let provider = variant.split('/').next().unwrap_or(&variant).to_string();
        sources.push(SourceBatch {
            source_path: path.clone(),
            entries: staged_to_entries(
                path,
                &staged,
                &provider,
                &variant,
                &snap.fingerprint,
                snap.len,
            ),
        });
        snapshots.push((path_ref.to_path_buf(), snap));
    }

    for (path, snapshot) in &snapshots {
        verify_snapshot(path, snapshot).map_err(ProtocolError::from)?;
    }

    let changed = store
        .commit_source_batches_if_changed(&sources)
        .map_err(ProtocolError::from)?;
    let generation = store.active_generation().map_err(ProtocolError::from)?;
    Ok(serde_json::json!({
        "sources": paths.len(),
        "messages": message_count,
        "committed": if changed { message_count } else { 0 },
        "unchanged": if changed { 0 } else { message_count },
        "generation": generation,
    }))
}

/// 把应用结果投影为 (outcome, data, page)：截断 → partial（exit 10），
/// 分页令牌 → envelope `page`。前端只做投影，不再解释语义。
fn render(response: AppResponse) -> (protocol::Outcome, serde_json::Value, protocol::Page) {
    match response {
        AppResponse::Search {
            hits,
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
                "hits": hits
                    .into_iter()
                    .map(|hit| serde_json::json!({
                        "id": hit.id.as_str(),
                        "score": hit.score,
                    }))
                    .collect::<Vec<_>>(),
                "generation": generation,
                "truncation": truncation_json(&truncation),
            });
            (outcome, data, page)
        }
        AppResponse::Get { payload } => (
            protocol::Outcome::Success,
            serde_json::json!({
                "payload": payload.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            }),
            protocol::Page::default(),
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
            (outcome, data, page)
        }
        AppResponse::Context {
            session_id,
            session,
            branch_leaf,
            messages,
            evidence,
            truncation,
            generation,
        } => {
            let outcome = outcome_of(&truncation);
            let data = serde_json::json!({
                "session_id": session_id,
                "session": session,
                "branch_leaf": branch_leaf,
                "messages": messages
                    .into_iter()
                    .map(|(id, payload)| serde_json::json!({
                        "id": id,
                        "payload": payload,
                    }))
                    .collect::<Vec<_>>(),
                "evidence": evidence,
                "truncation": truncation_json(&truncation),
                "generation": generation,
            });
            (outcome, data, protocol::Page::default())
        }
        AppResponse::Status {
            catalog_count,
            active_generation,
        } => (
            protocol::Outcome::Success,
            serde_json::json!({
                "catalog_count": catalog_count,
                "generation": active_generation,
            }),
            protocol::Page::default(),
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
