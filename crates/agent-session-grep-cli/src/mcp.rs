//! MCP stdio server：JSON-RPC 2.0 over stdin/stdout（CONTRACT §8）。
//!
//! 每行一个完整 JSON-RPC 消息；stdout 只输出协议 frame（唯一出口
//! [`protocol::write_stdout_line`]），诊断一律走 stderr。handler 只做协议校验与
//! 参数映射：良构参数构造 [`AppRequest`] 交给 Application，成功结果复用 CLI
//! 同一个 [`crate::render`] 投影——不复制搜索/分支/分页/预算业务规则。
//!
//! 错误分层（task design §0.3）：
//! - 协议层问题（坏 JSON、批量数组、未知方法/工具、非法参数、未初始化）→
//!   JSON-RPC error 对象；`-32602` 携带 `data.canonical_code = invalid_request`；
//! - 良构 [`AppRequest`] 之后的业务失败（cursor_invalid、not_found、...）→
//!   成功 JSON-RPC response，result 携 `isError: true` + canonical error 结构。

use crate::protocol::{self, CanonicalCode, Outcome, ProtocolError};
use crate::{CliError, provider_registry, render, store_ref};
use agent_session_grep_adapters_sqlite::SqliteStore;
use agent_session_grep_application::{App, AppRequest, ResponseBudget};
use agent_session_grep_domain::{ContextPolicy, StableId};
use serde_json::{Map, Value, json};

/// 支持的 MCP 协议版本（新→旧）。协商绝不谎报支持：请求版本在列才回显。
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// 钉住的最新协议版本：请求版本不在支持列表时的协商回落值。
const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// JSON-RPC 2.0 预定义错误码。
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// 在已打开的只读 store 上服务 MCP，直到 stdin EOF（→ 干净停机）。
///
/// 空行跳过；stdin 读错误归 `source_io`。stdout 写失败/EPIPE 的退出语义由
/// [`protocol::write_stdout_line`] 统一执行（design §0.8）。
pub(crate) fn serve(store: &SqliteStore) -> Result<Outcome, CliError> {
    let mut server = McpServer {
        store,
        initialized: false,
        initialize_seen: false,
    };
    for line in std::io::stdin().lines() {
        let line = line.map_err(|error| {
            ProtocolError::new(
                CanonicalCode::SourceIo,
                format!("cannot read stdin: {error}"),
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(frame) = server.handle_line(&line) {
            protocol::write_stdout_line(&frame);
        }
    }
    Ok(Outcome::Success)
}

/// 单连接 MCP 服务状态：注入的只读 store + initialize 门闩。
struct McpServer<'a> {
    store: &'a SqliteStore,
    /// `notifications/initialized` 之前只放行 initialize/ping（design §0.7）。
    initialized: bool,
    /// 是否收到过 initialize 请求：门闩只在握手之后打开，未握手先发
    /// initialized 通知是协议违规，不得开门（Minor-8）。
    initialize_seen: bool,
}

/// 工具调用的两类失败（design §0.3）：结构/校验问题 → JSON-RPC `-32602`；
/// 良构 [`AppRequest`] 之后的业务失败 → `isError: true` 工具结果。
enum ToolError {
    Params(String),
    Business(ProtocolError),
}

impl McpServer<'_> {
    /// 处理一行输入：notification（无 `id` 键）永不回应，request 必回一帧。
    /// 坏 JSON → `-32700`（id null）；非对象（含批量数组）→ `-32600`。
    fn handle_line(&mut self, line: &str) -> Option<String> {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                return Some(error_frame(
                    Value::Null,
                    PARSE_ERROR,
                    &format!("parse error: {error}"),
                    None,
                ));
            }
        };
        let Value::Object(message) = value else {
            // 2025-06-18 已移除 JSON-RPC batching；数组与其它非对象一律拒绝。
            return Some(error_frame(
                Value::Null,
                INVALID_REQUEST,
                "request must be a single JSON object (batching is not supported)",
                None,
            ));
        };
        let method = message.get("method").and_then(Value::as_str);
        match message.get("id") {
            // notification：JSON-RPC 规定永不回应；未知 notification 方法静默忽略。
            // `notifications/cancelled` 是 documented no-op（v0 顺序执行请求）。
            // 缺 method / method 非字符串的不是合法 notification → -32600；
            // `notifications/initialized` 只在收到过 initialize 请求后开门闩。
            None => match method {
                Some("notifications/initialized") => {
                    if self.initialize_seen {
                        self.initialized = true;
                    }
                    None
                }
                Some(_) => None,
                None => Some(error_frame(
                    Value::Null,
                    INVALID_REQUEST,
                    "notification must carry a string method",
                    None,
                )),
            },
            Some(id) => {
                let id = id.clone();
                let Some(method) = method else {
                    return Some(error_frame(
                        id,
                        INVALID_REQUEST,
                        "method must be a string",
                        None,
                    ));
                };
                Some(self.handle_request(id, method, message.get("params")))
            }
        }
    }

    /// request 分发。initialize/ping 始终放行；其余方法要求已初始化（design §0.7），
    /// 门闩优先于方法分发——未初始化时未知方法同样回 `-32600`。
    fn handle_request(&mut self, id: Value, method: &str, params: Option<&Value>) -> String {
        match method {
            "initialize" => {
                // 记录握手已发生：此后的 initialized 通知才有权开门闩。
                self.initialize_seen = true;
                // params 非对象是参数校验失败（-32602），不得静默按缺失处理。
                let requested = match params {
                    None => None,
                    Some(Value::Object(object)) => {
                        object.get("protocolVersion").and_then(Value::as_str)
                    }
                    Some(_) => {
                        return error_frame(
                            id,
                            INVALID_PARAMS,
                            "params must be an object",
                            Some(invalid_request_data()),
                        );
                    }
                };
                result_frame(
                    id,
                    json!({
                        "protocolVersion": negotiate_version(requested),
                        "capabilities": { "tools": {} },
                        "serverInfo": {
                            "name": "agent-session-grep",
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                    }),
                )
            }
            "ping" => result_frame(id, json!({})),
            _ if !self.initialized => {
                error_frame(id, INVALID_REQUEST, "server not initialized", None)
            }
            "tools/list" => result_frame(id, json!({ "tools": tool_catalog() })),
            "tools/call" => self.handle_tools_call(id, params),
            other => error_frame(
                id,
                METHOD_NOT_FOUND,
                &format!("method not found: {other}"),
                None,
            ),
        }
    }

    /// tools/call：解出 name/arguments 后按 [`ToolError`] 分层投影。
    fn handle_tools_call(&self, id: Value, params: Option<&Value>) -> String {
        let Some(params) = params.and_then(Value::as_object) else {
            return error_frame(
                id,
                INVALID_PARAMS,
                "params must be an object carrying name/arguments",
                Some(invalid_request_data()),
            );
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return error_frame(
                id,
                INVALID_PARAMS,
                "params.name must be a string",
                Some(invalid_request_data()),
            );
        };
        let empty = Map::new();
        let arguments = match params.get("arguments") {
            None => &empty,
            Some(Value::Object(map)) => map,
            Some(_) => {
                return error_frame(
                    id,
                    INVALID_PARAMS,
                    "params.arguments must be an object",
                    Some(invalid_request_data()),
                );
            }
        };
        match self.call_tool(name, arguments) {
            Ok(payload) => {
                // content.text 与 structuredContent 是同一 payload 的两种载体
                // （2025-06-18 字段；老客户端忽略未知字段，design §0.2）。
                let text = payload.to_string();
                result_frame(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": text }],
                        "structuredContent": payload,
                        "isError": false,
                    }),
                )
            }
            Err(ToolError::Params(message)) => {
                error_frame(id, INVALID_PARAMS, &message, Some(invalid_request_data()))
            }
            Err(ToolError::Business(error)) => result_frame(id, business_error_result(&error)),
        }
    }

    /// 工具名分发（合同 §8 的 6 个工具）；未知工具是请求校验失败 → `-32602`。
    fn call_tool(&self, name: &str, args: &Map<String, Value>) -> Result<Value, ToolError> {
        match name {
            "search_sessions" => self.tool_search(args),
            "get_session_context" => self.tool_context(args),
            "list_sessions" => self.tool_list(args),
            "list_providers" => {
                reject_unknown_keys(args, &[])?;
                Ok(providers_payload())
            }
            "get_status" => {
                reject_unknown_keys(args, &[])?;
                self.run_app(AppRequest::Status)
            }
            "doctor" => {
                reject_unknown_keys(args, &[])?;
                self.tool_doctor()
            }
            other => Err(ToolError::Params(format!("unknown tool: {other}"))),
        }
    }

    fn tool_search(&self, args: &Map<String, Value>) -> Result<Value, ToolError> {
        reject_unknown_keys(
            args,
            &["query", "limit", "cursor", "max_items", "max_bytes"],
        )?;
        let query = required_str(args, "query")?;
        let limit = opt_usize(args, "limit")?;
        // schema 声明 limit/max_items minimum 1（design §3）；additionalProperties
        // 同理代码侧强制。0 在协议层拒绝（-32602），不落成 App 层 isError 业务帧。
        if limit == Some(0) {
            return Err(ToolError::Params("limit must be >= 1".into()));
        }
        let cursor = opt_str(args, "cursor")?;
        let max_items = opt_usize(args, "max_items")?;
        let max_bytes = opt_usize(args, "max_bytes")?;
        if max_items == Some(0) {
            return Err(ToolError::Params("max_items must be >= 1".into()));
        }
        self.run_app(AppRequest::Search {
            query,
            limit: limit.or(max_items).unwrap_or(20),
            cursor,
            budget: budget_with(max_items, max_bytes, None),
        })
    }

    fn tool_context(&self, args: &Map<String, Value>) -> Result<Value, ToolError> {
        reject_unknown_keys(args, &["session_id", "policy", "max_messages", "max_bytes"])?;
        let wire = required_str(args, "session_id")?;
        // wire id 解析失败属请求校验（→ -32602）；格式合法但库中不存在则走
        // Application 的 not_found 业务路径（design §2 note）。
        let session_id = StableId::from_wire(&wire).ok_or_else(|| {
            ToolError::Params(format!("session_id is not a valid entity id: {wire}"))
        })?;
        let policy = match opt_str(args, "policy")?.as_deref() {
            None | Some("mainline") => ContextPolicy::Mainline,
            Some("full") => ContextPolicy::Full,
            Some(other) => {
                return Err(ToolError::Params(format!(
                    "policy must be mainline|full, got {other}"
                )));
            }
        };
        let max_messages = opt_usize(args, "max_messages")?;
        let max_bytes = opt_usize(args, "max_bytes")?;
        self.run_app(AppRequest::Context {
            session_id,
            policy,
            budget: budget_with(None, max_bytes, max_messages),
        })
    }

    fn tool_list(&self, args: &Map<String, Value>) -> Result<Value, ToolError> {
        reject_unknown_keys(args, &["limit", "cursor", "max_items", "max_bytes"])?;
        let limit = opt_usize(args, "limit")?;
        // 与 search_sessions 同层（协议层 -32602）：limit:0 / max_items:0 不得
        // 漏到 App 层变 isError 业务帧（分层一致性，Minor-7）。
        if limit == Some(0) {
            return Err(ToolError::Params("limit must be >= 1".into()));
        }
        let cursor = opt_str(args, "cursor")?;
        let max_items = opt_usize(args, "max_items")?;
        let max_bytes = opt_usize(args, "max_bytes")?;
        if max_items == Some(0) {
            return Err(ToolError::Params("max_items must be >= 1".into()));
        }
        self.run_app(AppRequest::List {
            limit: limit.or(max_items).unwrap_or(20),
            cursor,
            budget: budget_with(max_items, max_bytes, None),
        })
    }

    /// doctor 不经 App：直接读 store 只读事实，data 形状与 CLI doctor 对齐。
    fn tool_doctor(&self) -> Result<Value, ToolError> {
        let schema = self.store.schema_version().map_err(business)?;
        let generation = self.store.active_generation().map_err(business)?;
        let interrupted = self.store.interrupted_batch_count().map_err(business)?;
        Ok(success_payload(
            Outcome::Success,
            json!({
                "tool": env!("CARGO_PKG_NAME"),
                "version": env!("CARGO_PKG_VERSION"),
                "db": "ok",
                "schema": schema,
                "generation": generation,
                "interrupted_batches": interrupted,
            }),
            &protocol::Page::default(),
            &[],
        ))
    }

    /// 良构请求进 Application，成功走 CLI 同一个 [`render`] 投影；
    /// 失败即业务错误——此后不再产生 `-32602`（design §2 note）。
    fn run_app(&self, request: AppRequest) -> Result<Value, ToolError> {
        let app = App::new(store_ref(self.store), store_ref(self.store));
        match app.handle(request) {
            Ok(response) => {
                let (outcome, data, page, warnings) = render(response);
                Ok(success_payload(outcome, data, &page, &warnings))
            }
            Err(error) => Err(ToolError::Business(error.into())),
        }
    }
}

/// 版本协商（design §0.1）：回显受支持的请求版本，否则回落钉住的最新版。
/// 只返回支持集合内的静态字符串，结构上排除"谎报支持"的可能。
fn negotiate_version(requested: Option<&str>) -> &'static str {
    for supported in SUPPORTED_PROTOCOL_VERSIONS {
        if Some(supported) == requested {
            return supported;
        }
    }
    LATEST_PROTOCOL_VERSION
}

/// tools/list 目录：合同 §8 固定顺序的 6 个工具。schema 与代码侧校验一致
/// （`additionalProperties: false`、整数非负、search 的 limit 最小 1）；描述
/// 如实陈述 v0 限制（命中是消息级实体；list_sessions 分页全部 catalog 实体）。
fn tool_catalog() -> Value {
    json!([
        {
            "name": "search_sessions",
            "description": "Full-text search over ingested AI coding-agent session \
                history. Hits are message-level entities (msg_v1_ ids) in relevance \
                order; pass page.next_cursor back as cursor to fetch the next page.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Full-text query." },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Page size; defaults to 20."
                    },
                    "cursor": {
                        "type": "string",
                        "description": "Continuation token from the previous page's \
                            page.next_cursor."
                    },
                    "max_items": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Response item budget; also caps the page size. Minimum 1 (runtime rejects 0)."
                    },
                    "max_bytes": {
                        "type": "integer",
                        "minimum": 4096,
                        "description": "Response byte budget. Minimum 4096 (runtime rejects smaller)."
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        },
        {
            "name": "get_session_context",
            "description": "Assemble one session's branch: ordered messages plus \
                evidence spans. policy mainline (default) walks the parent chain \
                excluding sidechains; full returns every message in seq order.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session wire id (ses_v1_ prefix)."
                    },
                    "policy": {
                        "type": "string",
                        "enum": ["mainline", "full"],
                        "description": "Branch selection policy; defaults to mainline."
                    },
                    "max_messages": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Message count budget. Minimum 1 (runtime rejects 0)."
                    },
                    "max_bytes": {
                        "type": "integer",
                        "minimum": 4096,
                        "description": "Response byte budget. Minimum 4096 (runtime rejects smaller)."
                    }
                },
                "required": ["session_id"],
                "additionalProperties": false
            }
        },
        {
            "name": "list_sessions",
            "description": "Page catalog entities in stable wire-id order. v0 \
                limitation: pages ALL entity kinds (messages msg_v1_, sessions \
                ses_v1_, documents doc_v1_), not only sessions; filter on the \
                ses_v1_ prefix client-side.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Page size; defaults to 20. Minimum 1 (runtime rejects 0)."
                    },
                    "cursor": {
                        "type": "string",
                        "description": "Continuation token from the previous page's \
                            page.next_cursor."
                    },
                    "max_items": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Response item budget; also caps the page size. Minimum 1 (runtime rejects 0)."
                    },
                    "max_bytes": {
                        "type": "integer",
                        "minimum": 4096,
                        "description": "Response byte budget. Minimum 4096 (runtime rejects smaller)."
                    }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "list_providers",
            "description": "List the provider adapters this build can ingest \
                (stable ids such as claude-code).",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "get_status",
            "description": "Report catalog entity count and the active generation.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "doctor",
            "description": "Read-only store health check: schema version, active \
                generation, interrupted batch count.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }
    ])
}

/// 成功工具 payload（6 个工具同形，design §2）：outcome/data/warnings/page。
fn success_payload(
    outcome: Outcome,
    data: Value,
    page: &protocol::Page,
    warnings: &[String],
) -> Value {
    let outcome_str = match outcome {
        Outcome::Success => "success",
        Outcome::Partial => "partial",
    };
    json!({
        "outcome": outcome_str,
        "data": data,
        "warnings": warnings,
        "page": {
            "next_cursor": page.next_cursor.as_deref().map_or(Value::Null, |c| json!(c)),
            "has_more": page.has_more,
        },
    })
}

/// list_providers 不经 App：组合根的 registry 即权威清单（design §3）。
fn providers_payload() -> Value {
    let providers: Vec<Value> = provider_registry()
        .iter()
        .map(|adapter| json!({ "id": adapter.provider_id() }))
        .collect();
    success_payload(
        Outcome::Success,
        json!({ "providers": providers }),
        &protocol::Page::default(),
        &[],
    )
}

/// 业务失败的工具结果：`isError: true` + canonical error（design §2）。
fn business_error_result(error: &ProtocolError) -> Value {
    json!({
        "content": [{ "type": "text", "text": error.message }],
        "structuredContent": {
            "error": {
                "canonical_code": error.code.as_str(),
                "message": error.message,
                "retryable": error.code.retryable(),
                "details": error.details,
            }
        },
        "isError": true,
    })
}

fn business(error: impl Into<ProtocolError>) -> ToolError {
    ToolError::Business(error.into())
}

/// `-32602` 的 `error.data`：结构化 canonical 标注（design §2 note）。
fn invalid_request_data() -> Value {
    json!({
        "canonical_code": CanonicalCode::InvalidRequest.as_str(),
        "retryable": CanonicalCode::InvalidRequest.retryable(),
    })
}

fn result_frame(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_frame(id: Value, code: i64, message: &str, data: Option<Value>) -> String {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error }).to_string()
}

/// schema `additionalProperties: false` 的代码侧强制：未知键 → `-32602`。
fn reject_unknown_keys(args: &Map<String, Value>, allowed: &[&str]) -> Result<(), ToolError> {
    for key in args.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ToolError::Params(format!("unknown parameter: {key}")));
        }
    }
    Ok(())
}

fn required_str(args: &Map<String, Value>, key: &str) -> Result<String, ToolError> {
    match args.get(key) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(ToolError::Params(format!("{key} must be a string"))),
        None => Err(ToolError::Params(format!(
            "missing required parameter: {key}"
        ))),
    }
}

fn opt_str(args: &Map<String, Value>, key: &str) -> Result<Option<String>, ToolError> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(ToolError::Params(format!("{key} must be a string"))),
    }
}

/// 整数参数（design §3）：必须是非负整数 JSON number 且装得进 usize；
/// 负数、小数、字符串数字一律 `-32602`。
fn opt_usize(args: &Map<String, Value>, key: &str) -> Result<Option<usize>, ToolError> {
    match args.get(key) {
        None => Ok(None),
        Some(value) => {
            let n = value.as_u64().ok_or_else(|| {
                ToolError::Params(format!("{key} must be a non-negative integer"))
            })?;
            usize::try_from(n)
                .map(Some)
                .map_err(|_| ToolError::Params(format!("{key} exceeds the platform usize range")))
        }
    }
}

/// 默认预算 + 工具参数覆盖。CLI 的 `budget_from_flags` 是字符串 flag 形，
/// 这里参数已是类型化 usize，故独立构造；下限校验仍由 App 层统一执行。
fn budget_with(
    max_items: Option<usize>,
    max_bytes: Option<usize>,
    max_messages: Option<usize>,
) -> ResponseBudget {
    let mut budget = ResponseBudget::default();
    if let Some(value) = max_items {
        budget.max_items = value;
    }
    if let Some(value) = max_bytes {
        budget.max_response_bytes = value;
    }
    if let Some(value) = max_messages {
        budget.max_messages = value;
    }
    budget
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_session_grep_domain::{IdKind, Stability};

    fn open_store(dir: &tempfile::TempDir) -> SqliteStore {
        let path = dir.path().join("mcp-test.db");
        SqliteStore::open_for_write(path.to_str().expect("temp path must be utf-8"))
            .expect("open store for write")
    }

    /// 两条可检索消息（都命中 "hello"），供搜索/分页/status 用例。
    fn seeded_store(dir: &tempfile::TempDir) -> SqliteStore {
        let store = open_store(dir);
        let entries = [
            (
                StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"t1"]),
                b"payload-1".to_vec(),
                "hello world".to_string(),
            ),
            (
                StableId::derive(IdKind::Message, Stability::Reconstructed, &[b"t2"]),
                b"payload-2".to_vec(),
                "hello there".to_string(),
            ),
        ];
        store
            .commit_batch(&entries)
            .expect("commit searchable rows");
        store
    }

    fn fresh(store: &SqliteStore) -> McpServer<'_> {
        McpServer {
            store,
            initialized: false,
            initialize_seen: false,
        }
    }

    fn ready(store: &SqliteStore) -> McpServer<'_> {
        McpServer {
            store,
            initialized: true,
            initialize_seen: true,
        }
    }

    fn parse(frame: &str) -> Value {
        serde_json::from_str(frame).expect("frame must be valid JSON")
    }

    fn request(id: u64, method: &str, params: Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }

    fn respond(server: &mut McpServer<'_>, line: &str) -> Value {
        let frame = server
            .handle_line(line)
            .expect("request must get a response");
        parse(&frame)
    }

    fn call(server: &mut McpServer<'_>, name: &str, arguments: Value) -> Value {
        respond(
            server,
            &request(
                7,
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            ),
        )
    }

    #[test]
    fn negotiate_version_echoes_each_supported_version() {
        for supported in SUPPORTED_PROTOCOL_VERSIONS {
            assert_eq!(negotiate_version(Some(supported)), supported);
        }
    }

    #[test]
    fn negotiate_version_falls_back_to_latest_when_unsupported_or_absent() {
        assert_eq!(
            negotiate_version(Some("9999-01-01")),
            LATEST_PROTOCOL_VERSION
        );
        assert_eq!(negotiate_version(None), LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn initialize_reports_negotiated_version_and_server_info() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = fresh(&store);
        let v = respond(
            &mut server,
            &request(
                1,
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "test-client", "version": "0" },
                }),
            ),
        );
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(v["result"]["serverInfo"]["name"], "agent-session-grep");
        assert_eq!(
            v["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(v["result"]["capabilities"]["tools"].is_object());
        // 不支持的版本诚实回落钉住的最新版，绝不回显谎报。
        let v = respond(
            &mut server,
            &request(2, "initialize", json!({ "protocolVersion": "9999-01-01" })),
        );
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn ping_is_allowed_before_initialization() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = fresh(&store);
        let v = respond(&mut server, &request(3, "ping", json!({})));
        assert_eq!(v["result"], json!({}));
    }

    #[test]
    fn requests_before_initialized_notification_are_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = fresh(&store);
        let v = respond(&mut server, &request(2, "tools/list", json!({})));
        assert_eq!(v["error"]["code"], -32600);
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message")
                .contains("not initialized")
        );
        // 门闩优先于方法分发：未初始化时未知方法同样 -32600（design §0.7）。
        let v = respond(&mut server, &request(3, "foo/bar", json!({})));
        assert_eq!(v["error"]["code"], -32600);
    }

    #[test]
    fn initialized_notification_is_silent_and_opens_the_gate_after_handshake() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = fresh(&store);
        // 握手先行：initialize 请求之后，initialized 通知才开门闩（Minor-8）。
        let v = respond(&mut server, &request(1, "initialize", json!({})));
        assert!(v["result"]["protocolVersion"].is_string());
        let silent = server.handle_line(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
        );
        assert!(silent.is_none(), "notification must not get a response");
        let v = respond(&mut server, &request(4, "tools/list", json!({})));
        assert!(v["result"]["tools"].is_array());
    }

    #[test]
    fn initialized_notification_without_handshake_does_not_open_gate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = fresh(&store);
        // 未发过 initialize 请求就发 initialized 通知：门闩保持关闭。
        let silent = server.handle_line(
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
        );
        assert!(silent.is_none(), "notification must not get a response");
        let v = respond(&mut server, &request(4, "tools/list", json!({})));
        assert_eq!(v["error"]["code"], -32600);
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message")
                .contains("not initialized")
        );
    }

    #[test]
    fn notification_without_string_method_is_invalid_request() {
        // 缺 method / method 非字符串的不是合法 notification，必须回 -32600
        // 而非静默丢弃（JSON-RPC 对无效消息的拒绝义务，Minor-8）。
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        for line in [
            json!({ "jsonrpc": "2.0" }).to_string(),
            json!({ "jsonrpc": "2.0", "method": 42 }).to_string(),
        ] {
            let frame = server
                .handle_line(&line)
                .unwrap_or_else(|| panic!("invalid notification must get a response: {line}"));
            let v = parse(&frame);
            assert_eq!(v["error"]["code"], -32600, "{line}");
            assert!(v["id"].is_null(), "{line}");
        }
    }

    #[test]
    fn initialize_with_non_object_params_is_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = fresh(&store);
        for params in [json!(42), json!(null), json!("x")] {
            let v = respond(&mut server, &request(1, "initialize", params.clone()));
            assert_eq!(v["error"]["code"], -32602, "{params}");
            assert_eq!(v["error"]["data"]["canonical_code"], "invalid_request");
        }
        // params 缺失（键缺席）仍合法：协议版本回落钉住的最新版。
        let v = respond(
            &mut server,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize" }).to_string(),
        );
        assert_eq!(v["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn notifications_never_get_a_response() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        for method in ["notifications/cancelled", "totally/unknown", "tools/list"] {
            let silent =
                server.handle_line(&json!({ "jsonrpc": "2.0", "method": method }).to_string());
            assert!(silent.is_none(), "{method} without id must stay silent");
        }
    }

    #[test]
    fn request_with_explicit_null_id_is_answered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = fresh(&store);
        let v = respond(
            &mut server,
            &json!({ "jsonrpc": "2.0", "id": null, "method": "ping" }).to_string(),
        );
        assert!(v["id"].is_null());
        assert_eq!(v["result"], json!({}));
    }

    #[test]
    fn malformed_json_yields_parse_error_with_null_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = respond(&mut server, "{ not json");
        assert_eq!(v["error"]["code"], -32700);
        assert!(v["id"].is_null());
    }

    #[test]
    fn non_object_and_batch_inputs_are_invalid_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let batch = format!("[{}]", request(1, "ping", json!({})));
        for line in ["[]", "42", "\"frame\"", batch.as_str()] {
            let v = respond(&mut server, line);
            assert_eq!(v["error"]["code"], -32600, "{line}");
            assert!(v["id"].is_null(), "{line}");
        }
    }

    #[test]
    fn unknown_request_method_yields_method_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = respond(&mut server, &request(5, "foo/bar", json!({})));
        assert_eq!(v["error"]["code"], -32601);
    }

    #[test]
    fn tools_list_exposes_exactly_the_six_contract_tools() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = respond(&mut server, &request(6, "tools/list", json!({})));
        let tools = v["result"]["tools"].as_array().expect("tools array");
        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect();
        assert_eq!(
            names,
            [
                "search_sessions",
                "get_session_context",
                "list_sessions",
                "list_providers",
                "get_status",
                "doctor",
            ]
        );
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object", "{}", tool["name"]);
            assert_eq!(
                tool["inputSchema"]["additionalProperties"], false,
                "{}",
                tool["name"]
            );
            assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
        }
    }

    #[test]
    fn unknown_tool_yields_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = call(&mut server, "frobnicate", json!({}));
        assert_eq!(v["error"]["code"], -32602);
        assert_eq!(v["error"]["data"]["canonical_code"], "invalid_request");
    }

    #[test]
    fn missing_required_query_is_invalid_params_with_canonical_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = call(&mut server, "search_sessions", json!({}));
        assert_eq!(v["error"]["code"], -32602);
        assert_eq!(v["error"]["data"]["canonical_code"], "invalid_request");
        assert_eq!(v["error"]["data"]["retryable"], false);
    }

    #[test]
    fn out_of_schema_parameter_values_are_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let cases = [
            json!({ "query": 5 }),
            json!({ "query": "x", "cursor": 5 }),
            json!({ "query": "x", "max_items": -1 }),
            json!({ "query": "x", "max_items": 1.5 }),
            json!({ "query": "x", "limit": 0 }),
            json!({ "query": "x", "max_items": 0 }),
        ];
        for arguments in &cases {
            let v = call(&mut server, "search_sessions", arguments.clone());
            assert_eq!(v["error"]["code"], -32602, "{arguments}");
            assert_eq!(
                v["error"]["data"]["canonical_code"], "invalid_request",
                "{arguments}"
            );
        }
    }

    #[test]
    fn unknown_extra_property_is_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = call(
            &mut server,
            "search_sessions",
            json!({ "query": "x", "surprise": true }),
        );
        assert_eq!(v["error"]["code"], -32602);
    }

    #[test]
    fn list_sessions_rejects_zero_limit_and_max_items_at_protocol_layer() {
        // 分层一致性（Minor-7）：与 search_sessions 一样，list_sessions 的
        // limit:0 / max_items:0 在协议层拒（-32602），不落成 App 层 isError 业务帧。
        let dir = tempfile::tempdir().expect("tempdir");
        let store = seeded_store(&dir);
        let mut server = ready(&store);
        for arguments in [json!({ "limit": 0 }), json!({ "max_items": 0 })] {
            let v = call(&mut server, "list_sessions", arguments.clone());
            assert_eq!(v["error"]["code"], -32602, "{arguments}");
            assert_eq!(
                v["error"]["data"]["canonical_code"], "invalid_request",
                "{arguments}"
            );
            assert!(v["result"].is_null(), "{arguments}");
        }
        // 合法下限（1）不受影响：仍走正常业务帧。
        let v = call(&mut server, "list_sessions", json!({ "limit": 1 }));
        assert_eq!(v["result"]["isError"], false);
    }

    #[test]
    fn tool_schema_floors_match_runtime_budget_validation() {
        // 发布 schema 的下限必须与 ResponseBudget::validate 的运行时下限一致
        // （1 / 4096），不允许声明 0 又让运行时拒绝（Minor-6）。
        let tools = tool_catalog().as_array().expect("tools").clone();
        let floors: [(&str, &str, u64); 5] = [
            ("search_sessions", "limit", 1),
            ("search_sessions", "max_items", 1),
            ("search_sessions", "max_bytes", 4096),
            ("list_sessions", "limit", 1),
            ("list_sessions", "max_items", 1),
        ];
        for (tool_name, param, floor) in floors {
            let tool = tools
                .iter()
                .find(|tool| tool["name"] == tool_name)
                .unwrap_or_else(|| panic!("missing tool {tool_name}"));
            assert_eq!(
                tool["inputSchema"]["properties"][param]["minimum"], floor,
                "{tool_name}.{param} 下限必须与运行时一致"
            );
        }
        let context = tools
            .iter()
            .find(|tool| tool["name"] == "get_session_context")
            .expect("get_session_context");
        assert_eq!(
            context["inputSchema"]["properties"]["max_messages"]["minimum"], 1,
            "get_session_context.max_messages 下限必须与运行时一致"
        );
        assert_eq!(
            context["inputSchema"]["properties"]["max_bytes"]["minimum"], 4096,
            "get_session_context.max_bytes 下限必须与运行时一致"
        );
    }

    #[test]
    fn non_object_params_or_arguments_are_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = respond(&mut server, &request(8, "tools/call", json!(null)));
        assert_eq!(v["error"]["code"], -32602);
        let v = respond(
            &mut server,
            &request(
                9,
                "tools/call",
                json!({ "name": "get_status", "arguments": 5 }),
            ),
        );
        assert_eq!(v["error"]["code"], -32602);
    }

    #[test]
    fn invalid_session_wire_id_is_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = call(
            &mut server,
            "get_session_context",
            json!({ "session_id": "not-a-wire-id" }),
        );
        assert_eq!(v["error"]["code"], -32602);
        assert_eq!(v["error"]["data"]["canonical_code"], "invalid_request");
    }

    #[test]
    fn out_of_enum_policy_is_invalid_params() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = call(
            &mut server,
            "get_session_context",
            json!({ "session_id": "ses_v1_nope", "policy": "weird" }),
        );
        assert_eq!(v["error"]["code"], -32602);
    }

    #[test]
    fn garbage_cursor_is_a_business_error_with_cursor_invalid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = seeded_store(&dir);
        let mut server = ready(&store);
        let v = call(
            &mut server,
            "search_sessions",
            json!({ "query": "hello", "cursor": "garbage" }),
        );
        let result = &v["result"];
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["structuredContent"]["error"]["canonical_code"],
            "cursor_invalid"
        );
        assert_eq!(result["structuredContent"]["error"]["retryable"], false);
        assert!(result["content"][0]["text"].as_str().is_some());
    }

    #[test]
    fn unknown_session_with_valid_wire_id_is_business_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = call(
            &mut server,
            "get_session_context",
            json!({ "session_id": "ses_v1_nope" }),
        );
        assert_eq!(v["result"]["isError"], true);
        assert_eq!(
            v["result"]["structuredContent"]["error"]["canonical_code"],
            "not_found"
        );
    }

    #[test]
    fn search_round_trip_projects_the_shared_render_payload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = seeded_store(&dir);
        let mut server = ready(&store);
        let v = call(&mut server, "search_sessions", json!({ "query": "world" }));
        let result = &v["result"];
        assert_eq!(result["isError"], false);
        let payload = &result["structuredContent"];
        assert_eq!(payload["outcome"], "success");
        let hits = payload["data"]["hits"].as_array().expect("hits");
        assert_eq!(hits.len(), 1);
        assert!(
            hits[0]["id"]
                .as_str()
                .expect("hit id")
                .starts_with("msg_v1_")
        );
        assert_eq!(payload["page"]["next_cursor"], Value::Null);
        assert_eq!(payload["page"]["has_more"], false);
        // content.text 与 structuredContent 必须是同一 payload 的两种载体。
        assert_eq!(result["content"][0]["type"], "text");
        let text = result["content"][0]["text"].as_str().expect("text content");
        assert_eq!(parse(text), *payload);
    }

    #[test]
    fn search_paginates_with_cursor_across_disjoint_pages() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = seeded_store(&dir);
        let mut server = ready(&store);
        let first = call(
            &mut server,
            "search_sessions",
            json!({ "query": "hello", "max_items": 1 }),
        );
        let first_payload = &first["result"]["structuredContent"];
        let first_hits = first_payload["data"]["hits"].as_array().expect("hits");
        assert_eq!(first_hits.len(), 1);
        assert_eq!(first_payload["page"]["has_more"], true);
        let cursor = first_payload["page"]["next_cursor"]
            .as_str()
            .expect("next_cursor")
            .to_string();

        let second = call(
            &mut server,
            "search_sessions",
            json!({ "query": "hello", "max_items": 1, "cursor": cursor }),
        );
        let second_payload = &second["result"]["structuredContent"];
        let second_hits = second_payload["data"]["hits"].as_array().expect("hits");
        assert_eq!(second_hits.len(), 1);
        assert_ne!(
            first_hits[0]["id"], second_hits[0]["id"],
            "pages must be disjoint"
        );
    }

    #[test]
    fn get_status_reports_real_catalog_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = seeded_store(&dir);
        let mut server = ready(&store);
        let v = call(&mut server, "get_status", json!({}));
        let payload = &v["result"]["structuredContent"];
        assert_eq!(payload["outcome"], "success");
        assert_eq!(payload["data"]["catalog_count"], 2);
        assert!(payload["data"]["generation"].is_number());
    }

    #[test]
    fn doctor_reports_db_ok_with_store_facts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = call(&mut server, "doctor", json!({}));
        let data = &v["result"]["structuredContent"]["data"];
        assert_eq!(data["tool"], env!("CARGO_PKG_NAME"));
        assert_eq!(data["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(data["db"], "ok");
        assert!(data["schema"].is_number());
        assert!(data["generation"].is_number());
        assert_eq!(data["interrupted_batches"], 0);
    }

    #[test]
    fn list_providers_enumerates_the_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir);
        let mut server = ready(&store);
        let v = call(&mut server, "list_providers", json!({}));
        let payload = &v["result"]["structuredContent"];
        assert_eq!(payload["outcome"], "success");
        assert_eq!(payload["page"]["next_cursor"], Value::Null);
        let providers = payload["data"]["providers"].as_array().expect("providers");
        let ids: Vec<&str> = providers
            .iter()
            .map(|provider| provider["id"].as_str().expect("provider id"))
            .collect();
        assert!(ids.contains(&"claude-code"), "{ids:?}");
        assert!(ids.contains(&"codex"), "{ids:?}");
    }
}
