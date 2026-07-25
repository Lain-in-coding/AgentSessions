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
    App, AppError, AppRequest, AppResponse, StagedMessage, select_and_stage,
};
use agentsessions_domain::{DomainError, IdKind, Stability, StableId};
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
        Ok(()) => {}
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
/// 跳过带值 flag（`--db <path>` / `--output <mode>`）及其取值，第一个裸参数即命令。
fn command_name(args: &[String]) -> String {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--db" | "--output" => {
                it.next(); // 消费其取值
            }
            s if s.starts_with('-') => {}
            s => return s.to_string(),
        }
    }
    "unknown".into()
}

fn run(args: &[String], mode: protocol::OutputMode) -> Result<(), CliError> {
    let started = std::time::Instant::now();
    // --help / --version / doctor 在解析 --db 之前拦截——它们不需要数据库。
    // 放在最前面，使 `agentsessions --help`（无 --db）也能正常工作。
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(());
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
    let (command, outcome, data) = dispatch(&store, &rest)?;
    let duration_ms = started.elapsed().as_millis() as u64;
    protocol::emit(command, mode, outcome, data, duration_ms);
    Ok(())
}

fn config_paths(mode: protocol::OutputMode) -> Result<(), CliError> {
    let paths = platform_paths()?;
    protocol::emit("config.paths", mode, protocol::Outcome::Success, paths, 0);
    Ok(())
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
    search <query>         全文检索，按相关性降序返回命中
    get <wire-id>          按实体 id 取回原始 payload
    show <wire-id>         按实体 id 取回并归一化展示（role/text 结构）
    list [limit]           稳定排序列出 catalog 实体（默认 20）
    status                 报告 catalog 实体总数
    doctor                 环境自检（可选 --db 校验存储可打开）
    config paths           报告当前平台的 config/data/cache/logs 路径

GLOBAL:
    --db <path>            SQLite 数据存储路径（除 doctor/help/version 外必需）
    --output human|json|jsonl  输出模式（默认 human；切片期 human 仍为 JSON envelope）
    --robot                等价 --output json，无颜色/进度（stdout 只输出协议）
    -h, --help             打印本帮助
    -V, --version          打印版本",
        name = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
    );
}

/// doctor：最小环境自检。报告版本；若给了 --db，尝试打开存储并报告 schema。
fn doctor(args: &[String], mode: protocol::OutputMode) -> Result<(), CliError> {
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
    );
    Ok(())
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
) -> Result<(&'static str, protocol::Outcome, serde_json::Value), CliError> {
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
                ))
            } else {
                let fact = arg(rest, 1, "index <id-fact> <text>")?;
                let text = arg(rest, 2, "index <id-fact> <text>")?;
                Ok((
                    "index",
                    protocol::Outcome::Success,
                    index_one(store, fact, text)?,
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
            ))
        }
        // sync 对显式列出的多个源执行同一套只读快照 + staging，并在全部成功后
        // 通过一次 durable batch 提交，避免部分 source 已写入、后续 source 失败。
        "sync" => Ok((
            "sync",
            protocol::Outcome::Success,
            sync_files(store, &rest[1..])?,
        )),
        "search" => {
            let query = arg(rest, 1, "search <query>")?;
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Search {
                query: query.to_string(),
                limit: 20,
            })?;
            Ok((
                "search",
                protocol::Outcome::Success,
                response_data(response),
            ))
        }
        "get" => {
            let wire = arg(rest, 1, "get <wire-id>")?;
            let id = StableId::from_wire(wire)
                .ok_or_else(|| CliError::usage(format!("not a valid entity id: {wire}")))?;
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Get { id })?;
            Ok(("get", protocol::Outcome::Success, response_data(response)))
        }
        "show" => {
            let wire = arg(rest, 1, "show <wire-id>")?;
            let id = StableId::from_wire(wire)
                .ok_or_else(|| CliError::usage(format!("not a valid entity id: {wire}")))?;
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Show { id })?;
            Ok(("show", protocol::Outcome::Success, response_data(response)))
        }
        "list" => {
            let limit = rest
                .get(1)
                .map(|s| s.parse::<usize>())
                .transpose()
                .map_err(|_| CliError::usage("list [limit]: limit must be an integer"))?
                .unwrap_or(20);
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::List { limit })?;
            Ok(("list", protocol::Outcome::Success, response_data(response)))
        }
        "status" => {
            let app = App::new(store_ref(store), store_ref(store));
            let response = app.handle(AppRequest::Status)?;
            Ok((
                "status",
                protocol::Outcome::Success,
                response_data(response),
            ))
        }
        other => Err(CliError::usage(format!("unknown subcommand: {other}"))),
    }
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
/// `select_and_stage` 只回 staged 消息，但 ingest/sync 输出要报 variant；这里
/// 复用同一份 registry 借用为 `&[&dyn ...]` 后交给编排层，选中的 variant 由
/// 事后对全体 adapter 再 probe 取最高置信度得到（与选择逻辑一致）。
fn stage_with_registry(bytes: &[u8]) -> Result<(Vec<StagedMessage>, String), CliError> {
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

/// 把一个源的 staged 消息转成 (id, payload, index-text) 三元组。
///
/// 身份优先用 provider-native id（Claude Code 的 `uuid`，tier `Native`），
/// 使身份跨 data-root 迁移与文件重命名存活；provider 未给 native id 时回退到
/// path+seq 的 `Reconstructed` 派生。payload 存 canonical JSON（承载 role/text/
/// parent/timestamp/sidechain），使 threading 元数据穿过存储、供 `show` 展开；
/// FTS 索引正文仍只喂纯 text。
fn staged_to_entries(path: &str, staged: &[StagedMessage]) -> Vec<(StableId, Vec<u8>, String)> {
    staged
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
                "timestamp": message.timestamp,
                "is_sidechain": message.is_sidechain,
            })
            .to_string();
            (id, payload.into_bytes(), message.text.clone())
        })
        .collect()
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

    // 4) 派生 id + 构造该源的完整 scan 结果，按 source membership 提交。
    //    同文件重 ingest 时，本次消失的 message id 会被推导为 tombstone。
    let source = SourceBatch {
        source_path: path.to_string(),
        entries: staged_to_entries(path, &staged),
    };
    let changed = store
        .commit_source_batches_if_changed(std::slice::from_ref(&source))
        .map_err(ProtocolError::from)?;

    let generation = store.active_generation().map_err(ProtocolError::from)?;
    Ok(serde_json::json!({
        "variant": variant,
        "committed": if changed { staged.len() } else { 0 },
        "unchanged": if changed { 0 } else { staged.len() },
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
        let (staged, _variant) = stage_with_registry(&bytes)?;
        message_count += staged.len();
        sources.push(SourceBatch {
            source_path: path.clone(),
            entries: staged_to_entries(path, &staged),
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

fn response_data(response: AppResponse) -> serde_json::Value {
    match response {
        AppResponse::Search { hits } => serde_json::json!({
            "hits": hits
                .into_iter()
                .map(|hit| serde_json::json!({
                    "id": hit.id.as_str(),
                    "score": hit.score,
                }))
                .collect::<Vec<_>>(),
        }),
        AppResponse::Get { payload } => serde_json::json!({
            "payload": payload.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        }),
        // show 与 get 的区别：get 回原始 payload 字节，show 把存储的 canonical
        // JSON payload 展开成结构化 entity（含 role/text/parent/timestamp/threading）。
        // 未找到时 entity 为 null；payload 非合法 JSON 时按裸文本兜底。
        AppResponse::Show { payload } => serde_json::json!({
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
        AppResponse::List { entries } => serde_json::json!({
            "entries": entries
                .into_iter()
                .map(|entry| serde_json::json!({
                    "id": entry.id.as_str(),
                    "payload": String::from_utf8_lossy(&entry.payload),
                }))
                .collect::<Vec<_>>(),
        }),
        AppResponse::Status {
            catalog_count,
            active_generation,
        } => serde_json::json!({
            "catalog_count": catalog_count,
            "generation": active_generation,
        }),
    }
}

fn arg<'a>(rest: &'a [String], index: usize, usage: &str) -> Result<&'a str, CliError> {
    rest.get(index)
        .map(String::as_str)
        .ok_or_else(|| CliError::usage(format!("missing argument: {usage}")))
}
